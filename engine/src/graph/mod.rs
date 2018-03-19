use std;
use std::ops::Range;

pub mod first_out_graph;
pub mod link_id_to_tail_mapper;

pub use self::first_out_graph::{OwnedGraph, FirstOutGraph};

pub type NodeId = u32;
pub type EdgeId = u32;
pub type Weight = u32;
pub const INFINITY: u32 = std::u32::MAX / 2;

pub trait Link {
    fn head(&self) -> NodeId;
}

pub trait LinkWithStaticWeight: Link {
    fn weight(&self) -> Weight;
}

pub trait LinkWithMutStaticWeight: LinkWithStaticWeight {
    fn weight_mut(&self) -> &mut Weight;
}

#[derive(Debug, Copy, Clone)]
pub struct LinkData {
    pub node: NodeId,
    pub weight: Weight
}

impl Link for LinkData {
    fn head(&self) -> NodeId {
        self.node
    }
}

impl LinkWithStaticWeight for LinkData {
    fn weight(&self) -> Weight {
        self.weight
    }
}

pub trait Graph {
    fn num_nodes(&self) -> usize;
}

pub trait LinkIterGraph<'a>: Graph {
    type Link: Link;
    type Iter: Iterator<Item = Self::Link> + 'a; // fix with https://github.com/rust-lang/rfcs/pull/1598

    fn neighbor_iter(&'a self, node: NodeId) -> Self::Iter;

    fn reverse(&'a self) -> OwnedGraph {
        // vector of adjacency lists for the reverse graph
        let mut reversed: Vec<Vec<LinkData>> = (0..self.num_nodes()).map(|_| Vec::<LinkData>::new() ).collect();

        // iterate over all edges and insert them in the reversed structure
        for node in 0..(self.num_nodes() as NodeId) {
            for LinkData { node: neighbor, weight } in self.neighbor_iter(node) {
                reversed[neighbor as usize].push(LinkData { node, weight });
            }
        }

        OwnedGraph::from_adjancecy_lists(reversed)
    }

    fn ch_split(&'a self, node_ranks: &Vec<u32>) -> (OwnedGraph, OwnedGraph) {
        let mut up: Vec<Vec<LinkData>> = (0..self.num_nodes()).map(|_| Vec::<LinkData>::new() ).collect();
        let mut down: Vec<Vec<LinkData>> = (0..self.num_nodes()).map(|_| Vec::<LinkData>::new() ).collect();

        // iterate over all edges and insert them in the reversed structure
        for node in 0..(self.num_nodes() as NodeId) {
            for LinkData { node: neighbor, weight } in self.neighbor_iter(node) {
                if node_ranks[node as usize] < node_ranks[neighbor as usize] {
                    up[node as usize].push(LinkData { node: neighbor, weight });
                } else {
                    down[neighbor as usize].push(LinkData { node, weight });
                }
            }
        }

        (OwnedGraph::from_adjancecy_lists(up), OwnedGraph::from_adjancecy_lists(down))
    }
}

pub trait MutWeightLinkIterGraph<'a>: Graph {
    type Iter: Iterator<Item = (&'a NodeId, &'a mut Weight)> + 'a;
    fn mut_weight_link_iter(&'a mut self, node: NodeId) -> Self::Iter;
}

pub trait RandomLinkAccessGraph {
    fn link(&self, edge_id: EdgeId) -> LinkData;
    fn edge_index(&self, from: NodeId, to: NodeId) -> Option<EdgeId>;
    fn neighbor_edge_indices(&self, node: NodeId) -> Range<EdgeId>;

    fn neighbor_edge_indices_usize(&self, node: NodeId) -> Range<usize> {
        let range = self.neighbor_edge_indices(node);
        Range { start: range.start as usize, end: range.end as usize }
    }
}
