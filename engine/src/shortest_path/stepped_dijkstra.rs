use super::*;
use std::ops::Deref;
use index_heap::{IndexdMinHeap, Indexing};
use super::timestamped_vector::TimestampedVector;
use std::ops::Generator;

#[derive(Debug, Clone)]
pub enum QueryProgress {
    Progress(State),
    Done(Option<Weight>),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub struct State {
    pub distance: Weight,
    pub node: NodeId,
}

impl Indexing for State {
    fn as_index(&self) -> usize {
        self.node as usize
    }
}

#[derive(Debug)]
pub struct SteppedDijkstra<Graph: DijkstrableGraph, C: Deref<Target = Graph>> {
    graph: C,
    distances: TimestampedVector<Weight>,
    predecessors: Vec<NodeId>,
    closest_node_priority_queue: IndexdMinHeap<State>,
}

impl<Graph: DijkstrableGraph, C: Deref<Target = Graph>> SteppedDijkstra<Graph, C> {
    pub fn new(graph: C) -> SteppedDijkstra<Graph, C> {
        let n = graph.num_nodes();

        SteppedDijkstra {
            graph,
            // initialize tentative distances to INFINITY
            distances: TimestampedVector::new(n, INFINITY),
            predecessors: vec![n as NodeId; n],
            closest_node_priority_queue: IndexdMinHeap::new(n)
        }
    }

    pub fn query_generator<'a, 'b: 'a>(&'b mut self, query: Query) -> impl Generator<Yield=State, Return=Option<Weight>> + 'a {
        move || {
            self.closest_node_priority_queue.clear();
            self.distances.reset();

            // Start with origin
            self.distances.set(query.from as usize, 0);
            self.closest_node_priority_queue.push(State { distance: 0, node: query.from });

            // these are necessary because otherwise the borrow checker could not figure out
            // that we're only borrowing parts of self
            let closest_node_priority_queue = &mut self.closest_node_priority_queue;
            let distances = &mut self.distances;
            let predecessors = &mut self.predecessors;
            let graph = &self.graph;

            // Examine the frontier with lower distance nodes first (min-heap)
            loop {
                let next = closest_node_priority_queue.pop();

                if let Some(State { distance, node }) = next {
                    // Alternatively we could have continued to find all shortest paths
                    if node == query.to {
                        return Some(distance);
                    }


                    // For each node we can reach, see if we can find a way with
                    // a lower distance going through this node
                    graph.for_each_neighbor(node, &mut |edge: Link| {
                        let next = State { distance: distance + edge.weight, node: edge.node };

                        // If so, add it to the frontier and continue
                        if next.distance < distances[next.node as usize] {
                            // Relaxation, we have now found a better way
                            distances.set(next.node as usize, next.distance);
                            predecessors[next.node as usize] = node;
                            if closest_node_priority_queue.contains_index(next.as_index()) {
                                closest_node_priority_queue.decrease_key(next);
                            } else {
                                closest_node_priority_queue.push(next);
                            }
                        }
                    });

                    yield State { distance, node };
                } else {
                    return None;
                }
            }
        }
    }

    pub fn tentative_distance(&self, node: NodeId) -> Weight {
        self.distances[node as usize]
    }

    pub fn distances_pointer(&self) -> *const TimestampedVector<Weight> {
        &self.distances
    }

    pub fn predecessor(&self, node: NodeId) -> NodeId {
        self.predecessors[node as usize]
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }
}
