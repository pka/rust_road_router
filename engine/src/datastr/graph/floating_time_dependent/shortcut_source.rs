use super::*;
use crate::util::in_range_option::InRangeOption;

/// An enum for what might make up a TD-CCH edge at a given point in time.
#[derive(Debug, Clone, Copy)]
pub enum ShortcutSource {
    Shortcut(EdgeId, EdgeId), // shortcut over lower triangle
    OriginalEdge(EdgeId),     // original edge with corresponding id in the original graph
    None,                     // an infinity edge
}

impl ShortcutSource {
    /// Evaluate travel time of this shortcut for a given point in time and a completely customized graph.
    /// Will unpack and recurse if this is a real shortcut.
    /// The callback `f` can be used to do early returns if we reach a node that already has a better tentative distance.
    pub(super) fn evaluate<F>(&self, t: Timestamp, customized_graph: &CustomizedGraph, f: &mut F) -> FlWeight
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

    /// Recursively unpack this source and append the path to `result`.
    /// The timestamp is just needed for the recursion.
    pub(super) fn unpack_at(&self, t: Timestamp, customized_graph: &CustomizedGraph, result: &mut Vec<(EdgeId, Timestamp)>) {
        match *self {
            ShortcutSource::Shortcut(down, up) => {
                customized_graph.incoming.unpack_at(down, t, customized_graph, result);
                let t_mid = result.last().unwrap().1;
                customized_graph.outgoing.unpack_at(up, t_mid, customized_graph, result);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let arr = t + customized_graph.original_graph.travel_time_function(edge).evaluate(t);
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
    pub(super) fn exact_ttf_for(
        &self,
        start: Timestamp,
        end: Timestamp,
        shortcut_graph: &PartialShortcutGraph,
        target: &mut MutTopPLF,
        tmp: &mut ReusablePLFStorage,
    ) {
        debug_assert!(start.fuzzy_lt(end), "{:?} - {:?}", start, end);

        match *self {
            ShortcutSource::Shortcut(down, up) => {
                let mut first_target = tmp.push_plf();
                shortcut_graph
                    .get_incoming(down)
                    .exact_ttf_for(start, end, shortcut_graph, &mut first_target, target.storage_mut());
                // for `up` PLF we need to shift the time range
                let second_start = start + interpolate_linear(&first_target[0], &first_target[1], start);
                let second_end = end + interpolate_linear(&first_target[first_target.len() - 2], &first_target[first_target.len() - 1], end);

                let mut second_target = first_target.storage_mut().push_plf();
                shortcut_graph
                    .get_outgoing(up)
                    .exact_ttf_for(second_start, second_end, shortcut_graph, &mut second_target, target.storage_mut());

                let (first, second) = second_target.storage().top_plfs();
                PiecewiseLinearFunction::link_partials(first, second, start, end, target);
            }
            ShortcutSource::OriginalEdge(edge) => {
                let ttf = shortcut_graph.original_graph.travel_time_function(edge);
                ttf.copy_range(start, end, target);
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
