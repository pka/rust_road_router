use graph::time_dependent::*;
use shortest_path::td_stepped_dijkstra::QueryProgress as OtherQueryProgress;
use std::cmp::min;
use shortest_path::td_stepped_dijkstra::TDSteppedDijkstra;
use super::*;
use super::td_stepped_elimination_tree::*;
use ::shortest_path::customizable_contraction_hierarchy::cch_graph::CCHGraph;
// use ::in_range_option::InRangeOption;
use self::td_stepped_elimination_tree::QueryProgress;
use rank_select_map::BitVec;

#[derive(Debug)]
pub struct Server<'a> {
    forward: TDSteppedEliminationTree<'a, 'a>,
    backward: TDSteppedEliminationTree<'a, 'a>,
    td_dijkstra: TDSteppedDijkstra,
    cch_graph: &'a CCHGraph,
    shortcut_graph: &'a ShortcutGraph<'a>,
    tentative_distance: (Weight, Weight),
    meeting_nodes: Vec<(NodeId, Weight)>,
    forward_tree_path: Vec<NodeId>,
    backward_tree_path: Vec<NodeId>,
}

impl<'a> Server<'a> {
    pub fn new(cch_graph: &'a CCHGraph, shortcut_graph: &'a ShortcutGraph<'a>) -> Self {
        Self {
            forward: TDSteppedEliminationTree::new(shortcut_graph.upward_graph(), cch_graph.elimination_tree()),
            backward: TDSteppedEliminationTree::new(shortcut_graph.downward_graph(), cch_graph.elimination_tree()),
            td_dijkstra: TDSteppedDijkstra::new(shortcut_graph.original_graph().clone()), // TODO fix clone
            cch_graph,
            meeting_nodes: Vec::new(),
            tentative_distance: (INFINITY, INFINITY),
            shortcut_graph,
            forward_tree_path: Vec::new(),
            backward_tree_path: Vec::new(),
        }
    }

    pub fn distance(&mut self, from: NodeId, to: NodeId, departure_time: Timestamp) -> Option<Weight> {
        // initialize
        self.tentative_distance = (INFINITY, INFINITY);
        self.meeting_nodes.clear();
        self.forward.initialize_query(self.cch_graph.node_order().rank(from));
        self.backward.initialize_query(self.cch_graph.node_order().rank(to));

        // TODO get rid of reinit
        self.forward_tree_path.clear();
        self.backward_tree_path.clear();
        let mut forward_tree_mask = BitVec::new(self.shortcut_graph.original_graph().num_nodes());
        let mut backward_tree_mask = BitVec::new(self.shortcut_graph.original_graph().num_nodes());

        // forward up
        while let QueryProgress::Progress(node) = self.forward.next_step() {
            self.forward_tree_path.push(node);
        }

        // backward up
        while let QueryProgress::Progress(node) = self.backward.next_step() {
            self.backward_tree_path.push(node);
            let lower_bound = self.forward.node_data(node).lower_bound + self.backward.node_data(node).lower_bound;
            if lower_bound < self.tentative_distance.1 {
                self.tentative_distance.0 = min(self.tentative_distance.0, lower_bound);
                self.tentative_distance.1 = min(self.tentative_distance.1, self.forward.node_data(node).upper_bound + self.backward.node_data(node).upper_bound);
                debug_assert!(self.tentative_distance.0 <= self.tentative_distance.1);
                self.meeting_nodes.push((node, lower_bound));
            }
        }

        let tentative_upper_bound = self.tentative_distance.1;
        self.meeting_nodes.retain(|&(_, lower_bound)| lower_bound < tentative_upper_bound);

        for &(node, _) in &self.meeting_nodes {
            forward_tree_mask.set(node as usize);
            backward_tree_mask.set(node as usize);
        }

        let mut original_edges = BitVec::new(self.shortcut_graph.original_graph().num_arcs()); // TODO get rid of reinit
        let mut shortcuts = BitVec::new(2 * self.cch_graph.num_arcs()); // TODO get rid of reinit

        while let Some(node) = self.forward_tree_path.pop() {
            if forward_tree_mask.get(node as usize) {
                for label in &self.forward.node_data(node).labels {
                    if !shortcuts.get(label.shortcut_id as usize * 2 + 1) {
                        shortcuts.set(label.shortcut_id as usize * 2 + 1);
                        self.shortcut_graph.get_upward(label.shortcut_id).unpack(self.shortcut_graph, &mut shortcuts, &mut original_edges);
                    }
                    forward_tree_mask.set(label.parent as usize);
                }
            }
        }

        while let Some(node) = self.backward_tree_path.pop() {
            if backward_tree_mask.get(node as usize) {
                for label in &self.backward.node_data(node).labels {
                    if !shortcuts.get(label.shortcut_id as usize * 2) {
                        shortcuts.set(label.shortcut_id as usize * 2);
                        self.shortcut_graph.get_downward(label.shortcut_id).unpack(self.shortcut_graph, &mut shortcuts, &mut original_edges);
                    }
                    backward_tree_mask.set(label.parent as usize);
                }
            }
        }

        self.td_dijkstra.initialize_query(TDQuery { from, to, departure_time });
        loop {
            match self.td_dijkstra.next_step(|edge_id| original_edges.get(edge_id as usize)) {
                OtherQueryProgress::Progress(_) => continue,
                OtherQueryProgress::Done(result) => return result
            }
        }
    }
}