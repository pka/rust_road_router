use super::*;
use crate::util::in_range_option::InRangeOption;

/// An enum for what might make up a TD-CCH edge at a given point in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShortcutSource {
    Shortcut(EdgeId, EdgeId), // shortcut over lower triangle
    OriginalEdge(EdgeId),     // original edge with corresponding id in the original graph
    None,                     // an infinity edge
}

impl ShortcutSource {
    /// Evaluate travel time of this shortcut for a given point in time and a completely customized graph.
    /// Will unpack and recurse if this is a real shortcut.
    /// The callback `f` can be used to do early returns if we reach a node that already has a better tentative distance.
    pub(super) fn evaluate_opt<F>(&self, t: Timestamp, customized_graph: &CustomizedGraph, f: &mut F) -> FlWeight
    where
        F: (FnMut(bool, EdgeId, Timestamp) -> bool),
    {
        match *self {
            // recursively eval down edge, then up edge
            ShortcutSource::Shortcut(down, up) => {
                if !f(false, down, t) {
                    return FlWeight::INFINITY;
                }
                let first_val = customized_graph.incoming.evaluate(down, t, customized_graph, f);
                debug_assert!(first_val >= FlWeight::zero());
                let t_mid = t + first_val;
                if !f(true, up, t_mid) {
                    return FlWeight::INFINITY;
                }
                let second_val = customized_graph.outgoing.evaluate(up, t_mid, customized_graph, f);
                debug_assert!(second_val >= FlWeight::zero());
                first_val + second_val
            }
            ShortcutSource::OriginalEdge(edge) => {
                let res = customized_graph.original_graph.travel_time_function(edge).evaluate(t);
                debug_assert!(res >= FlWeight::zero());
                res
            }
            ShortcutSource::None => FlWeight::INFINITY,
        }
    }

    pub(super) fn evaluate<'g>(&self, t: Timestamp, shortcut_graph: &'g impl ShortcutGraphTrt<'g>) -> FlWeight {
        match *self {
            // recursively eval down edge, then up edge
            ShortcutSource::Shortcut(down, up) => {
                let first_val = shortcut_graph.evaluate(ShortcutId::Incoming(down), t);
                debug_assert!(first_val >= FlWeight::zero());
                let t_mid = t + first_val;
                let second_val = shortcut_graph.evaluate(ShortcutId::Outgoing(up), t_mid);
                debug_assert!(second_val >= FlWeight::zero());
                first_val + second_val
            }
            ShortcutSource::OriginalEdge(edge) => {
                let res = shortcut_graph.original_graph().travel_time_function(edge).evaluate(t);
                debug_assert!(res >= FlWeight::zero());
                res
            }
            ShortcutSource::None => FlWeight::INFINITY,
        }
    }

    /// Recursively unpack this source and append the path to `result`.
    /// The timestamp is just needed for the recursion.
    pub(super) fn unpack_at<'g>(&self, t: Timestamp, shortcut_graph: &'g impl ShortcutGraphTrt<'g>, result: &mut Vec<(EdgeId, Timestamp)>) {
        match *self {
            ShortcutSource::Shortcut(down, up) => {
                shortcut_graph.unpack_at(ShortcutId::Incoming(down), t, result);
                let t_mid = result.last().unwrap().1;
                shortcut_graph.unpack_at(ShortcutId::Outgoing(up), t_mid, result);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let arr = t + shortcut_graph.original_graph().travel_time_function(edge).evaluate(t);
                result.push((edge, arr))
            }
            ShortcutSource::None => {
                panic!("can't unpack None source");
            }
        }
    }

    /// (Recursively) calculate the exact PLF for this source in a given time range.
    // Use two `ReusablePLFStorage`s to reduce allocations.
    // One storage will contain the functions of `up` and `down` - the other the result function.
    // That means when recursing, we need to use the two storages with flipped roles.
    pub(super) fn exact_ttf_for<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph.exact_ttf_for(ShortcutId::Incoming(down), start, end, &mut first_target, target.storage_mut());
                debug_assert!(!first_target.last().unwrap().at.fuzzy_lt(end));
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                if second_start.fuzzy_eq(second_end) {
                    debug_assert_eq!(first_target.len(), 2);
                    let second_val = shortcut_graph.evaluate(ShortcutId::Outgoing(up), second_start);
                    for p in &first_target[..] {
                        target.push(TTFPoint {
                            at: p.at,
                            val: p.val + second_val,
                        });
                    }
                } else {
                    let mut second_target = first_target.storage_mut().push_plf();
                    shortcut_graph.exact_ttf_for(ShortcutId::Outgoing(up), second_start, second_end, &mut second_target, target.storage_mut());

                    let (first, second) = second_target.storage().top_plfs();
                    PartialPiecewiseLinearFunction::new(first).link(&PartialPiecewiseLinearFunction::new(second), start, end, target);

                    debug_assert!(
                        !target.last().unwrap().at.fuzzy_lt(end),
                        "{:?}",
                        dbg_each!(
                            self,
                            target.last(),
                            start,
                            end,
                            first.first(),
                            first.last(),
                            first.len(),
                            second.first(),
                            second.last(),
                            second.len()
                        )
                    );
                }
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                ttf.append_range(start, end, target);
            }
            ShortcutSource::None => {
                panic!("can't fetch ttf for None source");
            }
        }
    }

    pub(super) fn partial_lower_bound<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g, ApproxTTF = ApproxTTF<'g>>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Incoming(down))
                    .bound_plfs()
                    .0
                    .append_range(start, end, &mut first_target);

                debug_assert!(!first_target.last().unwrap().at.fuzzy_lt(end));
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                let mut second_target = first_target.storage_mut().push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Outgoing(up))
                    .bound_plfs()
                    .0
                    .append_range(second_start, second_end, &mut second_target);

                let (first, second) = second_target.storage().top_plfs();
                PartialPiecewiseLinearFunction::new(first).link(&PartialPiecewiseLinearFunction::new(second), start, end, target);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                ttf.append_range(start, end, target);
            }
            ShortcutSource::None => {
                panic!("can't fetch ttf for None source");
            }
        }
    }

    pub(super) fn partial_upper_bound<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g, ApproxTTF = ApproxTTF<'g>>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Incoming(down))
                    .bound_plfs()
                    .1
                    .append_range(start, end, &mut first_target);

                debug_assert!(!first_target.last().unwrap().at.fuzzy_lt(end));
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                let mut second_target = first_target.storage_mut().push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Outgoing(up))
                    .bound_plfs()
                    .1
                    .append_range(second_start, second_end, &mut second_target);

                let (first, second) = second_target.storage().top_plfs();
                PartialPiecewiseLinearFunction::new(first).link(&PartialPiecewiseLinearFunction::new(second), start, end, target);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                ttf.append_range(start, end, target);
            }
            ShortcutSource::None => {
                panic!("can't fetch ttf for None source");
            }
        }
    }

    pub(super) fn partial_lower_bound_from_partial<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g, ApproxTTF = ApproxPartialTTF<'g>>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Incoming(down))
                    .bound_plfs()
                    .0
                    .sub_plf(start, end)
                    .append(start, &mut first_target);

                debug_assert!(!first_target.last().unwrap().at.fuzzy_lt(end));
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                let mut second_target = first_target.storage_mut().push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Outgoing(up))
                    .bound_plfs()
                    .0
                    .sub_plf(second_start, second_end)
                    .append(second_start, &mut second_target);

                let (first, second) = second_target.storage().top_plfs();
                PartialPiecewiseLinearFunction::new(first).link(&PartialPiecewiseLinearFunction::new(second), start, end, target);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                ttf.append_range(start, end, target);
            }
            ShortcutSource::None => {
                panic!("can't fetch ttf for None source");
            }
        }
    }

    pub(super) fn partial_upper_bound_from_partial<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g, ApproxTTF = ApproxPartialTTF<'g>>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Incoming(down))
                    .bound_plfs()
                    .1
                    .sub_plf(start, end)
                    .append(start, &mut first_target);

                debug_assert!(!first_target.last().unwrap().at.fuzzy_lt(end));
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                let mut second_target = first_target.storage_mut().push_plf();
                shortcut_graph
                    .ttf(ShortcutId::Outgoing(up))
                    .bound_plfs()
                    .1
                    .sub_plf(second_start, second_end)
                    .append(second_start, &mut second_target);

                let (first, second) = second_target.storage().top_plfs();
                PartialPiecewiseLinearFunction::new(first).link(&PartialPiecewiseLinearFunction::new(second), start, end, target);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                ttf.append_range(start, end, target);
            }
            ShortcutSource::None => {
                panic!("can't fetch ttf for None source");
            }
        }
    }

    /// Check if this edge is actually necessary for correctness of the CH or if it could possibly be removed (or set to infinity)
    pub(super) fn required(&self, shortcut_graph: &PartialShortcutGraph) -> bool {
        match *self {
            ShortcutSource::Shortcut(down, up) => shortcut_graph.get_incoming(down).required && shortcut_graph.get_outgoing(up).required,
            ShortcutSource::OriginalEdge(_) => true,
            ShortcutSource::None => false,
        }
    }

    pub fn get_switchpoints<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
    ) -> (Vec<(Timestamp, Vec<EdgeId>, FlWeight)>, FlWeight) {
        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let (first_switchpoints, first_end) = shortcut_graph.get_switchpoints(ShortcutId::Incoming(down), start, end);
                let (second_switchpoints, second_end) =
                    shortcut_graph.get_switchpoints(ShortcutId::Outgoing(up), start + first_switchpoints[0].2, end + first_end);

                let mut switchpoints = Vec::new();
                let mut first_iter = first_switchpoints.into_iter().peekable();
                let mut second_iter = second_switchpoints.into_iter().peekable();
                let mut first_cur = first_iter.next().unwrap();
                let mut second_cur = second_iter.next().unwrap();
                switchpoints.push((
                    start,
                    first_cur.1.iter().copied().chain(second_cur.1.iter().copied()).collect(),
                    first_cur.2 + second_cur.2,
                ));
                loop {
                    match (first_iter.peek(), second_iter.peek()) {
                        (Some(first), Some(second)) => {
                            let second_dt = shortcut_graph.original_graph().inverse_evaluate_path(&first_cur.1, second.0);
                            if first.0.fuzzy_lt(second_dt) {
                                switchpoints.push((
                                    first.0,
                                    first.1.iter().copied().chain(second_cur.1.iter().copied()).collect(),
                                    first.2 + shortcut_graph.original_graph().evaluate_path(&second_cur.1, first.0 + first.2),
                                ));
                                first_cur = first_iter.next().unwrap();
                            } else if second_dt.fuzzy_lt(first.0) {
                                switchpoints.push((
                                    second_dt,
                                    first_cur.1.iter().copied().chain(second.1.iter().copied()).collect(),
                                    second.0 + second.2 - second_dt,
                                ));
                                second_cur = second_iter.next().unwrap();
                            } else {
                                switchpoints.push((
                                    first.0,
                                    first.1.iter().copied().chain(second.1.iter().copied()).collect(),
                                    second.0 + second.2 - second_dt,
                                ));
                                first_cur = first_iter.next().unwrap();
                                second_cur = second_iter.next().unwrap();
                            }
                        }
                        (Some(first), None) => {
                            switchpoints.push((
                                first.0,
                                first.1.iter().copied().chain(second_cur.1.iter().copied()).collect(),
                                first.2 + shortcut_graph.original_graph().evaluate_path(&second_cur.1, first.0 + first.2),
                            ));
                            first_iter.next();
                        }
                        (None, Some(second)) => {
                            let second_dt = shortcut_graph.original_graph().inverse_evaluate_path(&first_cur.1, second.0);
                            switchpoints.push((
                                second_dt,
                                first_cur.1.iter().copied().chain(second.1.iter().copied()).collect(),
                                second.0 + second.2 - second_dt,
                            ));
                            second_iter.next();
                        }
                        (None, None) => break,
                    }
                }

                (switchpoints, first_end + second_end)
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph().travel_time_function(edge);
                (vec![(start, vec![edge], ttf.evaluate(start))], ttf.evaluate(end))
            }
            ShortcutSource::None => {
                panic!("can't compute switchpoints for None source");
            }
        }
    }
}

/// More compact struct to actually store `ShortcutSource`s in memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShortcutSourceData {
    down_arc: InRangeOption<EdgeId>,
    up_arc: InRangeOption<EdgeId>,
}

impl From<ShortcutSource> for ShortcutSourceData {
    fn from(source: ShortcutSource) -> Self {
        match source {
            ShortcutSource::Shortcut(down, up) => ShortcutSourceData {
                down_arc: InRangeOption::new(Some(down)),
                up_arc: InRangeOption::new(Some(up)),
            },
            ShortcutSource::OriginalEdge(edge) => ShortcutSourceData {
                down_arc: InRangeOption::new(None),
                up_arc: InRangeOption::new(Some(edge)),
            },
            ShortcutSource::None => ShortcutSourceData {
                down_arc: InRangeOption::new(None),
                up_arc: InRangeOption::new(None),
            },
        }
    }
}

impl From<ShortcutSourceData> for ShortcutSource {
    fn from(data: ShortcutSourceData) -> Self {
        match data.down_arc.value() {
            Some(down_shortcut_id) => ShortcutSource::Shortcut(down_shortcut_id, data.up_arc.value().unwrap()),
            None => match data.up_arc.value() {
                Some(up_arc) => ShortcutSource::OriginalEdge(up_arc),
                None => ShortcutSource::None,
            },
        }
    }
}

impl Default for ShortcutSourceData {
    fn default() -> Self {
        Self {
            down_arc: InRangeOption::new(None),
            up_arc: InRangeOption::new(None),
        }
    }
}

pub trait Sources {
    fn exact_ttf_for<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    );

    fn get_switchpoints<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
    ) -> (Vec<(Timestamp, Vec<EdgeId>, FlWeight)>, FlWeight);

    fn edge_source_at(&self, t: Timestamp) -> Option<&ShortcutSourceData>;

    fn wrapping_iter(&self, start: Timestamp, end: Timestamp) -> WrappingSourceIter;
}

use std::cmp::{max, min};
impl Sources for [(Timestamp, ShortcutSourceData)] {
    fn exact_ttf_for<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        // when we have multiple source, we need to do unpacking (and append the results) for all sources which are relevant for the given time range.
        let mut c = SourceCursor::valid_at(self, start);
        while c.cur().0.fuzzy_lt(end) {
            let mut inner_target = tmp.push_plf();
            ShortcutSource::from(c.cur().1).exact_ttf_for(
                max(start, c.cur().0),
                min(end, c.next().0),
                shortcut_graph,
                &mut inner_target,
                target.storage_mut(),
            );
            PartialPiecewiseLinearFunction::new(&inner_target).append(max(start, c.cur().0), target);

            c.advance();
        }

        for points in target.windows(2) {
            debug_assert!(points[0].at.fuzzy_lt(points[1].at));
        }

        debug_assert!(!target.last().unwrap().at.fuzzy_lt(end), "{:?}", dbg_each!(self, start, end));
    }

    fn get_switchpoints<'g>(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &'g impl ShortcutGraphTrt<'g>,
    ) -> (Vec<(Timestamp, Vec<EdgeId>, FlWeight)>, FlWeight) {
        let mut c = SourceCursor::valid_at(self, start);

        let (mut switchpoints, mut last_weight) = ShortcutSource::from(c.cur().1).get_switchpoints(max(start, c.cur().0), min(end, c.next().0), shortcut_graph);

        c.advance();

        while c.cur().0.fuzzy_lt(end) {
            let (mut cur_switchpoints, end_weight) =
                ShortcutSource::from(c.cur().1).get_switchpoints(max(start, c.cur().0), min(end, c.next().0), shortcut_graph);

            if switchpoints.last().unwrap().1 == cur_switchpoints.first().unwrap().1 {
                cur_switchpoints.first_mut().unwrap().0 = switchpoints.last().unwrap().0;
                switchpoints.pop();
            }
            switchpoints.append(&mut cur_switchpoints);
            last_weight = end_weight;

            c.advance();
        }

        (switchpoints, last_weight)
    }

    fn edge_source_at(&self, t: Timestamp) -> Option<&ShortcutSourceData> {
        if self.is_empty() {
            return None;
        }
        if self.len() == 1 {
            return Some(&self[0].1);
        }

        let (_, t_period) = t.split_of_period();
        debug_assert!(self.first().map(|&(t, _)| t == Timestamp::zero()).unwrap_or(true), "{:?}", self);
        match self.binary_search_by_key(&t_period, |(t, _)| *t) {
            Ok(i) => self.get(i),
            Err(i) => {
                debug_assert!(self.get(i - 1).map(|&(t, _)| t < t_period).unwrap_or(true));
                if i < self.len() {
                    debug_assert!(t_period < self[i].0);
                }
                self.get(i - 1)
            }
        }
        .map(|(_, s)| s)
    }

    fn wrapping_iter(&self, start: Timestamp, end: Timestamp) -> WrappingSourceIter {
        WrappingSourceIter {
            cursor: SourceCursor::valid_at(&self, start),
            end,
        }
    }
}

// Helper struct to iterate over sources.
// Allows to get sources valid for times > period().
// Handles all the ugly wraparound logic.
#[derive(Debug)]
pub struct SourceCursor<'a> {
    sources: &'a [(Timestamp, ShortcutSourceData)],
    current_index: usize,
    offset: FlWeight,
}

impl<'a> SourceCursor<'a> {
    pub fn valid_at(sources: &'a [(Timestamp, ShortcutSourceData)], t: Timestamp) -> Self {
        let (times_period, t) = t.split_of_period();
        let offset = times_period * FlWeight::from(period());

        let pos = sources.binary_search_by(|p| {
            use std::cmp::Ordering;
            if p.0.fuzzy_eq(t) {
                Ordering::Equal
            } else if p.0 < t {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        });

        match pos {
            Ok(i) => Self {
                sources,
                current_index: i,
                offset,
            },
            Err(i) => Self {
                sources,
                current_index: i - 1,
                offset,
            },
        }
    }

    pub fn cur(&self) -> (Timestamp, ShortcutSourceData) {
        (self.sources[self.current_index].0 + self.offset, self.sources[self.current_index].1)
    }

    pub fn next(&self) -> (Timestamp, ShortcutSourceData) {
        if self.current_index + 1 == self.sources.len() {
            (self.sources[0].0 + self.offset + FlWeight::from(period()), self.sources[0].1)
        } else {
            (self.sources[self.current_index + 1].0 + self.offset, self.sources[self.current_index + 1].1)
        }
    }

    pub fn advance(&mut self) {
        self.current_index += 1;
        if self.current_index == self.sources.len() {
            self.offset = self.offset + FlWeight::from(period());
            self.current_index = 0;
        }
    }
}

#[derive(Debug)]
pub struct WrappingSourceIter<'a> {
    pub cursor: SourceCursor<'a>,
    pub end: Timestamp,
}

impl<'a> Iterator for WrappingSourceIter<'a> {
    type Item = (Timestamp, ShortcutSourceData);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.cur().0.fuzzy_lt(self.end) {
            let res = Some(self.cursor.cur());
            self.cursor.advance();
            res
        } else {
            None
        }
    }
}
