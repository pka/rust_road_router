use super::*;

use std::ops::Deref;
use std::collections::LinkedList;
use std::ops::{Generator, GeneratorState};

#[derive(Debug)]
pub struct Server<C: Deref<Target = Graph>> {
    dijkstra: SteppedDijkstra<Graph, C>,
}

impl<C: Deref<Target = Graph>> Server<C> {
    pub fn new(graph: C) -> Server<C> {
        Server {
            dijkstra: SteppedDijkstra::new(graph)
        }
    }

    pub fn distance(&mut self, from: NodeId, to: NodeId) -> Option<Weight> {
        let mut coroutine = self.dijkstra.query_generator(Query { from, to });
        loop {
            match coroutine.resume() {
                GeneratorState::Yielded(_) => continue,
                GeneratorState::Complete(result) => return result
            }
        }
    }

    pub fn is_in_searchspace(&self, node: NodeId) -> bool {
        self.dijkstra.tentative_distance(node) < INFINITY
    }

    pub fn path(&self) -> LinkedList<NodeId> {
        let mut path = LinkedList::new();
        // TODO
        // path.push_front(self.dijkstra.query().to);

        // while *path.front().unwrap() != self.dijkstra.query().from {
        //     let next = self.dijkstra.predecessor(*path.front().unwrap());
        //     path.push_front(next);
        // }

        path
    }
}
