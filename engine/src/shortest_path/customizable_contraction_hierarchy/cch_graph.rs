use super::*;
use shortest_path::node_order::NodeOrder;
use in_range_option::InRangeOption;
use benchmark::measure;
use self::first_out_graph::degrees_to_first_out;
use rank_select_map::*;

#[derive(Debug)]
#[allow(dead_code)]
pub struct CCHGraph {
    first_out: Vec<EdgeId>,
    head: Vec<NodeId>,
    node_order: NodeOrder,
    original_edge_to_ch_edge: Vec<EdgeId>,
    elimination_tree: Vec<InRangeOption<NodeId>>,
    edge_id_to_tail: RankSelectMap,
}

impl CCHGraph {
    pub(super) fn new<Graph: for<'a> LinkIterGraph<'a>>(contracted_graph: ContractedGraph<Graph>) -> CCHGraph {
        let elimination_tree = contracted_graph.elimination_tree();
        let ContractedGraph(contracted_graph) = contracted_graph;
        let node_order = contracted_graph.node_order;
        let original_graph = contracted_graph.original_graph;

        let graph = Self::adjancecy_lists_to_first_out_graph(contracted_graph.nodes);
        let n = graph.num_nodes() as NodeId;

        let original_edge_to_ch_edge = (0..n).flat_map(|node| {
            {
                let graph = &graph;
                let node_order = &node_order;

                original_graph.neighbor_iter(node).map(move |Link { node: neighbor, .. }| {
                    let node_rank = node_order.rank(node);
                    let neighbor_rank = node_order.rank(neighbor);
                    if node_rank < neighbor_rank {
                        graph.edge_index(node_rank, neighbor_rank).unwrap()
                    } else {
                        graph.edge_index(neighbor_rank, node_rank).unwrap()
                    }
                })
            }
        }).collect();

        let mut first_out_bits = BitVec::new(graph.num_arcs() + 1);
        let (first_out, head, _) = graph.decompose();
        for &index in first_out.iter() {
            assert!(!first_out_bits.get(index as usize) || index as usize == head.len());
            first_out_bits.set(index as usize);
        }
        let edge_id_to_tail = RankSelectMap::new(first_out_bits);

        for (node_id, window) in first_out[..].windows(2).enumerate() {
            for edge_id in window[0]..window[1] {
                debug_assert_eq!(edge_id_to_tail.at_or_next_lower(edge_id as usize), node_id)
            }
        }

        CCHGraph {
            first_out,
            head,
            node_order,
            original_edge_to_ch_edge,
            elimination_tree,
            edge_id_to_tail,
        }
    }

    fn adjancecy_lists_to_first_out_graph(adjancecy_lists: Vec<Node>) -> OwnedGraph {
        let n = adjancecy_lists.len();

        let first_out: Vec<EdgeId> = {
            let degrees = adjancecy_lists.iter().map(|neighbors| neighbors.edges.len() as EdgeId);
            degrees_to_first_out(degrees).collect()
        };
        debug_assert_eq!(first_out.len(), n + 1);

        let head: Vec<NodeId> = adjancecy_lists
            .into_iter()
            .flat_map(|neighbors| neighbors.edges.into_iter())
            .collect();

        let m = head.len();
        OwnedGraph::new(first_out, head, vec![INFINITY; m])
    }

    pub fn customize<Graph: for<'a> LinkIterGraph<'a> + RandomLinkAccessGraph>(&self, metric: &Graph) ->
        (
            FirstOutGraph<&[EdgeId], &[NodeId], Vec<Weight>>,
            FirstOutGraph<&[EdgeId], &[NodeId], Vec<Weight>>,
            Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
            Vec<(InRangeOption<EdgeId>, InRangeOption<EdgeId>)>,
        )
    {
        let n = (self.first_out.len() - 1) as NodeId;
        let m = self.head.len();

        let mut upward_shortcut_expansions = vec![(InRangeOption::new(None), InRangeOption::new(None)); m];
        let mut downward_shortcut_expansions = vec![(InRangeOption::new(None), InRangeOption::new(None)); m];

        let mut upward_weights = vec![INFINITY; m];
        let mut downward_weights = vec![INFINITY; m];

        measure("CCH apply weights", || {
            for node in 0..n {
                for (edge_id, Link { node: neighbor, weight }) in metric.neighbor_edge_indices(node).zip(metric.neighbor_iter(node)) {
                    let ch_edge_id = self.original_edge_to_ch_edge[edge_id as usize];

                    if self.node_order.rank(node) < self.node_order.rank(neighbor) {
                        upward_weights[ch_edge_id as usize] = weight;
                    } else {
                        downward_weights[ch_edge_id as usize] = weight;
                    }
                }
            }
        });

        let mut upward = FirstOutGraph::new(&self.first_out[..], &self.head[..], upward_weights);
        let mut downward = FirstOutGraph::new(&self.first_out[..], &self.head[..], downward_weights);

        measure("CCH Customization", || {
            let mut node_outgoing_weights = vec![(INFINITY, InRangeOption::new(None)); n as usize];
            let mut node_incoming_weights = vec![(INFINITY, InRangeOption::new(None)); n as usize];

            for current_node in 0..n {
                for (Link { node, weight }, edge_id) in downward.neighbor_iter(current_node).zip(downward.neighbor_edge_indices(current_node)) {
                    node_incoming_weights[node as usize] = (weight, InRangeOption::new(Some(edge_id)));
                    debug_assert_eq!(downward.link(edge_id).node, node);
                }
                for (Link { node, weight }, edge_id) in upward.neighbor_iter(current_node).zip(upward.neighbor_edge_indices(current_node)) {
                    node_outgoing_weights[node as usize] = (weight, InRangeOption::new(Some(edge_id)));
                    debug_assert_eq!(upward.link(edge_id).node, node);
                }

                for (Link { node, weight }, edge_id) in downward.neighbor_iter(current_node).zip(downward.neighbor_edge_indices(current_node)) {
                    debug_assert_eq!(self.edge_id_to_tail(edge_id), current_node);
                    let shortcut_edge_ids = upward.neighbor_edge_indices(node);
                    for ((&target, shortcut_weight), shortcut_edge_id) in upward.mut_weight_link_iter(node).zip(shortcut_edge_ids) {
                        debug_assert_eq!(self.edge_id_to_tail(shortcut_edge_id), node);
                        if weight + node_outgoing_weights[target as usize].0 < *shortcut_weight {
                            *shortcut_weight = weight + node_outgoing_weights[target as usize].0;
                            debug_assert!(node_outgoing_weights[target as usize].1.value().is_some());
                            upward_shortcut_expansions[shortcut_edge_id as usize] = (InRangeOption::new(Some(edge_id)), node_outgoing_weights[target as usize].1)
                        }
                    }
                }
                for (Link { node, weight }, edge_id) in upward.neighbor_iter(current_node).zip(upward.neighbor_edge_indices(current_node)) {
                    debug_assert_eq!(self.edge_id_to_tail(edge_id), current_node);
                    let shortcut_edge_ids = downward.neighbor_edge_indices(node);
                    for ((&target, shortcut_weight), shortcut_edge_id) in downward.mut_weight_link_iter(node).zip(shortcut_edge_ids) {
                        debug_assert_eq!(self.edge_id_to_tail(shortcut_edge_id), node);
                        if weight + node_incoming_weights[target as usize].0 < *shortcut_weight {
                            *shortcut_weight = weight + node_incoming_weights[target as usize].0;
                            debug_assert!(node_incoming_weights[target as usize].1.value().is_some());
                            downward_shortcut_expansions[shortcut_edge_id as usize] = (node_incoming_weights[target as usize].1, InRangeOption::new(Some(edge_id)))
                        }
                    }
                }

                for Link { node, .. } in downward.neighbor_iter(current_node) {
                    node_incoming_weights[node as usize] = (INFINITY, InRangeOption::new(None));
                }
                for Link { node, .. } in upward.neighbor_iter(current_node) {
                    node_outgoing_weights[node as usize] = (INFINITY, InRangeOption::new(None));
                }
            }
        });

        (upward, downward, upward_shortcut_expansions, downward_shortcut_expansions)
    }

    pub fn node_order(&self) -> &NodeOrder {
        &self.node_order
    }

    pub fn elimination_tree(&self) -> &[InRangeOption<NodeId>] {
        &self.elimination_tree[..]
    }

    pub fn edge_id_to_tail(&self, edge_id: EdgeId) -> NodeId {
        self.edge_id_to_tail.at_or_next_lower(edge_id as usize) as NodeId
    }
}