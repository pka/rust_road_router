use super::*;

#[derive(Debug, Clone, Copy)]
pub struct Linked {
    first: EdgeId,
    second: EdgeId
}

impl Linked {
    pub fn new(first: EdgeId, second: EdgeId) -> Linked {
        Linked { first, second }
    }

    pub fn evaluate(&self, departure: Timestamp, shortcut_graph: &ShortcutGraph) -> Weight {
        debug_assert!(departure < shortcut_graph.period());
        let first_edge = shortcut_graph.get_downward(self.first);
        let second_edge = shortcut_graph.get_upward(self.second);
        let first_edge_value = first_edge.evaluate(departure, shortcut_graph);
        first_edge_value + second_edge.evaluate((departure + first_edge_value) % shortcut_graph.period(), shortcut_graph)
    }

    pub fn bounds(&self, shortcut_graph: &ShortcutGraph) -> (Weight, Weight) {
        let (first_min, first_max) = shortcut_graph.get_downward(self.first).bounds(shortcut_graph);
        let (second_min, second_max) = shortcut_graph.get_upward(self.second).bounds(shortcut_graph);
        // INFINITY????
        (first_min + second_min, first_max + second_max)
    }

    pub fn ipp_iter<'a>(&self, range: WrappingRange<Timestamp>, shortcut_graph: &'a ShortcutGraph) -> Iter<'a> {
        let first_edge = shortcut_graph.get_downward(self.first);
        let second_edge = shortcut_graph.get_upward(self.second);
        Iter::new(first_edge, second_edge, range, shortcut_graph)
    }

    pub fn as_shortcut_data(&self) -> ShortcutData {
        ShortcutData::new(ShortcutSource::Shortcut(self.first, self.second))
    }

    pub fn is_valid_path(&self, shortcut_graph: &ShortcutGraph) -> bool {
        shortcut_graph.get_downward(self.first).is_valid_path() && shortcut_graph.get_upward(self.second).is_valid_path()
    }

    pub fn debug_to_s<'a>(&self, shortcut_graph: &'a ShortcutGraph, indent: usize) -> String {
        let first_edge = shortcut_graph.get_downward(self.first);
        let second_edge = shortcut_graph.get_upward(self.second);
        format!("Linked:\n{}first: {}\n{}second: {}", String::from(" ").repeat(indent * 2), first_edge.debug_to_s(shortcut_graph, indent + 1), String::from(" ").repeat(indent * 2), second_edge.debug_to_s(shortcut_graph, indent + 1))
    }
}

pub struct Iter<'a> {
    first_edge: &'a Shortcut,
    second_edge: &'a Shortcut,
    first_iter: shortcut::Iter<'a, 'a>,
    second_iter: std::iter::Peekable<shortcut::Iter<'a, 'a>>,
    range: WrappingRange<Timestamp>,
    prev_ipp_at: Timestamp,

    shortcut_graph: &'a ShortcutGraph<'a>,

    first_edge_prev_ipp: Option<TTIpp>,
    first_edge_next_ipp: Option<TTIpp>,
    second_edge_prev_ipp: Option<TTIpp>
}

impl<'a> Iter<'a> {
    fn new(first_edge: &'a Shortcut, second_edge: &'a Shortcut, range: WrappingRange<Timestamp>, shortcut_graph: &'a ShortcutGraph) -> Iter<'a> {
        let mut first_iter = first_edge.ipp_iter(range.clone(), shortcut_graph);
        let first_edge_next_ipp = first_iter.next();

        let first_edge_initial_value = if first_edge_next_ipp.is_some() && first_edge_next_ipp.unwrap().at == range.start() {
            first_edge_next_ipp.unwrap().val
        } else {
            first_edge.evaluate(range.start(), shortcut_graph)
        };
        let first_edge_prev_ipp = if first_edge_next_ipp.is_some() && first_edge_next_ipp.unwrap().at == range.start() {
            None
        } else {
            Some(TTIpp::new(range.start(), first_edge_initial_value))
        };

        let second_edge_range_begin = (range.start() + first_edge_initial_value) % shortcut_graph.period();
        let second_iter = second_edge.ipp_iter(WrappingRange::new(Range { start: second_edge_range_begin, end: second_edge_range_begin }, shortcut_graph.period()), shortcut_graph).peekable();

        Iter {
            first_edge, second_edge, first_iter, second_iter, shortcut_graph,
            range,
            prev_ipp_at: shortcut_graph.period(),
            first_edge_prev_ipp,
            first_edge_next_ipp,
            second_edge_prev_ipp: None
        }
    }

    fn get_next(&mut self) -> Option<<Self as Iterator>::Item> {
        // println!("");
        // println!("next");
        // println!("second iter peek: {:?}", self.second_iter.peek());
        // println!("first_edge_prev_ipp {:?}", self.first_edge_prev_ipp);
        // println!("first_edge_next_ipp {:?}", self.first_edge_next_ipp);
        match self.second_iter.peek().cloned() {
            Some(second_edge_ipp) => {
                if self.first_edge_prev_ipp.is_some() && self.first_edge_target_range_to_next().contains(second_edge_ipp.at) {
                    // println!("target range {:?}", self.first_edge_target_range_to_next());
                    // println!("before next first edge ipp");
                    // println!("first edge next or end ipp {:?}", self.first_edge_next_ipp_or_end());
                    let ipp = invert(self.first_edge_prev_ipp.unwrap().as_tuple(), self.first_edge_next_ipp_or_end().as_tuple(), second_edge_ipp.at, self.shortcut_graph.period());
                    // println!("inverted {}", ipp);
                    self.second_edge_prev_ipp = Some(second_edge_ipp);
                    self.second_iter.next();
                    Some(TTIpp::new(ipp, (self.range.wrap_around() + second_edge_ipp.at - ipp + second_edge_ipp.val) % self.range.wrap_around()))
                } else if let Some(first_edge_next_ipp) = self.first_edge_next_ipp {
                    debug_assert!(abs_diff(first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph)) < TOLERANCE, "at: {} was: {} but should have been: {}. Shortcut {}", first_edge_next_ipp.at, first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph), self.first_edge.debug_to_s(self.shortcut_graph, 0));
                    self.first_edge_prev_ipp = self.first_edge_next_ipp;
                    let second_edge_value = self.eval_second_edge(first_edge_next_ipp.at + first_edge_next_ipp.val);
                    self.first_edge_next_ipp = self.first_iter.next();
                    Some(TTIpp::new(first_edge_next_ipp.at, first_edge_next_ipp.val + second_edge_value))
                } else {
                    None
                }
            },
            None => {
                // println!("next first edge ipp: {:?}", self.first_edge_next_ipp);
                if let Some(first_edge_next_ipp) = self.first_edge_next_ipp {
                    debug_assert!(abs_diff(first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph)) < TOLERANCE, "at: {} was: {} but should have been: {}. Shortcut {}", first_edge_next_ipp.at, first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph), self.first_edge.debug_to_s(self.shortcut_graph, 0));
                    self.first_edge_prev_ipp = self.first_edge_next_ipp;
                    let second_edge_value = self.eval_second_edge(first_edge_next_ipp.at + first_edge_next_ipp.val);
                    self.first_edge_next_ipp = self.first_iter.next();
                    Some(TTIpp::new(first_edge_next_ipp.at, first_edge_next_ipp.val + second_edge_value))
                } else {
                    None
                }
            },
        }
    }

    fn first_edge_target_range_to_next(&self) -> WrappingRange<Timestamp> {
        let first_edge_next_ipp = self.first_edge_next_ipp_or_end();
        debug_assert!(abs_diff(first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph)) < TOLERANCE, "at: {} was: {} but should have been: {}. Shortcut {}", first_edge_next_ipp.at, first_edge_next_ipp.val, self.first_edge.evaluate(first_edge_next_ipp.at, self.shortcut_graph), self.first_edge.debug_to_s(self.shortcut_graph, 0));
        let wrap = self.range.wrap_around();
        let first_edge_prev_ipp = self.first_edge_prev_ipp.unwrap();
        debug_assert!(abs_diff(first_edge_prev_ipp.val, self.first_edge.evaluate(first_edge_prev_ipp.at, self.shortcut_graph)) < TOLERANCE, "at: {} was: {} but should have been: {}. Shortcut {}", first_edge_prev_ipp.at, first_edge_prev_ipp.val, self.first_edge.evaluate(first_edge_prev_ipp.at, self.shortcut_graph), self.first_edge.debug_to_s(self.shortcut_graph, 0));
        if first_edge_prev_ipp.at > first_edge_next_ipp.at {
            debug_assert!(first_edge_prev_ipp.at + first_edge_prev_ipp.val <= first_edge_next_ipp.at + first_edge_next_ipp.val + wrap);
        } else {
            debug_assert!(first_edge_prev_ipp.at + first_edge_prev_ipp.val <= first_edge_next_ipp.at + first_edge_next_ipp.val);
        }
        let start = (first_edge_prev_ipp.at + first_edge_prev_ipp.val) % wrap;
        let mut end = (first_edge_next_ipp.at + first_edge_next_ipp.val) % wrap;
        if start == end && first_edge_prev_ipp.at != first_edge_next_ipp.at { end += 1; } // happens in the case of ascent -1
        WrappingRange::new(Range { start , end }, wrap)
    }

    // TODO memoize?
    fn first_edge_next_ipp_or_end(&self) -> TTIpp {
        self.first_edge_next_ipp.unwrap_or_else(|| TTIpp::new(self.range.end(), self.first_edge.evaluate(self.range.end(), self.shortcut_graph)))
    }

    fn eval_second_edge(&mut self, mut at: Timestamp) -> Weight {
        if let Some(second_edge_prev_ipp) = self.second_edge_prev_ipp {
            if let Some(second_edge_next_ipp) = self.second_iter.peek() {
                let lf = Line::new(second_edge_prev_ipp, *second_edge_next_ipp);
                if at < lf.from.at {
                    at += self.shortcut_graph.period();
                }
                return lf.into_monotone_at_line(self.shortcut_graph.period()).interpolate_tt(at)
            }
        }
        self.second_edge.evaluate(at % self.shortcut_graph.period(), self.shortcut_graph)
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = TTIpp;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.get_next();
        if let Some(next) = next {
            if next.at != self.prev_ipp_at {
                self.prev_ipp_at = next.at;
                Some(next)
            } else {
                self.next()
            }
        } else {
            next
        }
    }
}

fn invert(first_ipp: (Timestamp, Timestamp), second_ipp: (Timestamp, Timestamp), target_time: Timestamp, period: Timestamp) -> Timestamp {
    if first_ipp.1 == second_ipp.1 {
        return (target_time + period - first_ipp.1) % period
    }
    let first_ipp = (first_ipp.0, first_ipp.0 + first_ipp.1);
    let second_ipp = if second_ipp.0 < first_ipp.0 {
        (second_ipp.0 + period, second_ipp.0 + period + second_ipp.1)
    } else {
        (second_ipp.0, second_ipp.0 + second_ipp.1)
    };
    let target_time = if target_time < first_ipp.1 { target_time + period } else { target_time };
    debug_assert!(target_time >= first_ipp.1, "{:?} {:?} {}", first_ipp, second_ipp, target_time);
    // debug_assert!(target_time <= second_ipp.1, "{:?} {:?} {}", first_ipp, second_ipp, target_time);

    let delta_x = second_ipp.0 - first_ipp.0;
    let delta_x = u64::from(delta_x);
    let delta_y = second_ipp.1 - first_ipp.1;
    let delta_y = u64::from(delta_y);

    if delta_y == 0 {
        debug_assert_eq!(first_ipp.1, target_time);
        return first_ipp.0
    }

    let delta_y_to_target = target_time - first_ipp.1;
    let delta_x_to_target = (delta_y - 1 + u64::from(delta_y_to_target) * delta_x) / delta_y;

    (first_ipp.0 + delta_x_to_target as u32) % period
}

fn link_segments(first_segment: &TTFSeg, second_segment: &TTFSeg, period: Timestamp) -> TTFSeg {
    // TODO only invert if necessary, check with result range of first segment
    let start_of_second = invert(first_segment.line.from.as_tuple(), first_segment.line.to.as_tuple(), second_segment.valid.start, period);
    let start_of_range = if WrappingRange::new(first_segment.valid.clone(), period).contains(start_of_second) {
        start_of_second
    } else {
        first_segment.valid.start
    };
    let end_of_second = invert(first_segment.line.from.as_tuple(), first_segment.line.to.as_tuple(), second_segment.valid.end, period);
    let end_of_range = if WrappingRange::new(first_segment.valid.clone(), period).contains(end_of_second) {
        end_of_second
    } else {
        first_segment.valid.end
    };
    // ((start_of_range, first_segment.from.val + second_segment.from.val), (end_of_range, first_segment.to.val + second_segment.to.val))
    Segment {
        line: Line {
            from: Ipp::new(first_segment.line.from.at, first_segment.line.from.val + second_segment.line.from.val),
            to: Ipp::new(first_segment.line.to.at, first_segment.line.to.val + second_segment.line.to.val),
        },
        valid: Range { start: start_of_range, end: end_of_range }
    }
}

fn foo() {
    // in arrival time
    // calc linked steigung
    // reduce with gcd
    // anchor = eval second at first.first.val
    //  als bruch: (val - second.first.at) / (second.second.at - second.first.at)
    //      * (second.second.val - second.first.val)
    //      + second.first.val
    // anchor + linked steigung * x == ganze zahl?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linking_static_segments() {
        assert_eq!(link_segments(&TTFSeg::new((0, 2), (5, 2)), &TTFSeg::new((1, 2), (8, 2)), 10), TTFSeg::new((0, 4), (5, 4)));
    }

    #[test]
    fn test_linking_one_static_one_var_segments() {
        assert_eq!(link_segments(&TTFSeg::new((0, 2), (5, 2)), &TTFSeg::new((2, 2), (7, 3)), 10), TTFSeg::new((0, 4), (5, 5)));
        assert_eq!(link_segments(&TTFSeg::new((0, 2), (5, 3)), &TTFSeg::new((2, 2), (8, 2)), 10), TTFSeg::new((0, 4), (5, 5)));
    }

    #[test]
    #[ignore]
    fn test_linking_non_fitting_length_segments() {
        for s in 0..10 {
            assert_eq!(link_segments(&TTFSeg::new((s, 2), ((s + 5) % 10 , 2)), &TTFSeg::new(((s + 2) % 10, 2), ((s + 6) % 10, 3)), 10),
                TTFSeg::new((s, 4), ((s + 4) % 10, 5)));
            assert_eq!(link_segments(&TTFSeg::new((s, 2), ((s + 4) % 10 , 2)), &TTFSeg::new(((s + 3) % 10, 2), ((s + 5) % 10, 3)), 10),
                TTFSeg::new(((s + 1) % 10, 4), ((s + 3) % 10, 5)));
        }
    }

    #[test]
    fn test_linking_different_slopes() {
        assert_eq!(
            link_segments(&Segment { line: Line { from: Ipp::new(0, 2), to: Ipp::new(4, 4) }, valid: Range { start: 1, end: 3 } }, &Segment { line: Line { from: Ipp::new(2, 2), to: Ipp::new(8, 1) }, valid: Range { start: 3, end: 7 } }, 10),
            Segment { line: Line { from: Ipp::new(0, 4), to: Ipp::new(4, 5) }, valid: Range { start: 1, end: 3 } });
    }

    #[test]
    fn test_bounds() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![0, 5, 8,  1, 3, 9],
            vec![1, 2, 1,  2, 5, 3],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        assert_eq!(linked.bounds(&shortcut_graph), (3, 7));
    }

    #[test]
    fn test_eval_on_ipp() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![1, 3, 9,  0, 5, 8],
            vec![2, 5, 3,  1, 2, 1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        assert_eq!(linked.evaluate(0, &shortcut_graph), 3);
    }

    #[test]
    fn test_interpolating_eval() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![1, 3, 9,  0, 5, 8],
            vec![2, 5, 3,  1, 2, 1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        assert_eq!(linked.evaluate(1, &shortcut_graph), 4);
    }

    #[test]
    fn test_inversion() {
        assert_eq!(invert((0,1), (5,2), 1, 10), 0);
        assert_eq!(invert((0,1), (5,2), 3, 10), 2);
        assert_eq!(invert((0,2), (5,1), 2, 10), 0);
        assert_eq!(invert((0,2), (5,1), 3, 10), 2);
        assert_eq!(invert((0,1), (4,1), 3, 10), 2);
        assert_eq!(invert((9,1), (1,3), 2, 10), 0);
    }

    #[test]
    fn test_full_range_ipp_iter() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![1, 3, 9,  0, 5, 8],
            vec![2, 5, 3,  1, 2, 1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 0, end: 0 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(0,3), (2,6), (5,5), (8,4)]); // TODO ipp at 5,5 is correct, but its 5 on 3 and 4, too. Is that a problem???
    }

    #[test]
    fn test_wrapping_range_ipp_iter() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![1, 3, 9,  0, 5, 8],
            vec![2, 5, 3,  1, 2, 1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 1, end: 1 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(2,6), (5,5), (8,4), (0,3)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 2, end: 2 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(2,6), (5,5), (8,4), (0,3)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 3, end: 3 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(5,5), (8,4), (0,3), (2,6)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 4, end: 4 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(5,5), (8,4), (0,3), (2,6)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 5, end: 5 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(5,5), (8,4), (0,3), (2,6)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 6, end: 6 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(8,4), (0,3), (2,6), (5,5)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 7, end: 7 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(8,4), (0,3), (2,6), (5,5)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 8, end: 8 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(8,4), (0,3), (2,6), (5,5)]);
        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 9, end: 9 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(0,3), (2,6), (5,5), (8,4)]);
    }

    #[test]
    fn test_partial_range_ipp_iter() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 6],
            vec![1, 3, 9,  0, 5, 8],
            vec![2, 5, 3,  1, 2, 1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 4, end: 1 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(5,5), (8,4), (0,3)]);
    }

    #[test]
    fn test_linked_with_static_first_edge() {
        let graph = TDGraph::new(
            vec![0, 1, 2, 2],
            vec![2, 0],
            vec![0, 3, 4],
            vec![1, 3, 9,  0],
            vec![2, 5, 3,  1],
            10
        );

        let cch_first_out = vec![0, 1, 3, 3];
        let cch_head =      vec![2, 0, 2];

        let outgoing = vec![Shortcut::new(Some(0)), Shortcut::new(None), Shortcut::new(None)];
        let incoming = vec![Shortcut::new(None), Shortcut::new(Some(1)), Shortcut::new(None)];

        let shortcut_graph = ShortcutGraph::new(&graph, &cch_first_out, &cch_head, outgoing, incoming);
        let linked = Linked::new(1, 0);

        let all_ipps: Vec<(Timestamp, Weight)> = linked.ipp_iter(WrappingRange::new(Range { start: 0, end: 0 }, 10), &shortcut_graph).map(TTIpp::as_tuple).collect();
        assert_eq!(all_ipps, vec![(0,3), (2,6), (8,4)]);
    }
}
