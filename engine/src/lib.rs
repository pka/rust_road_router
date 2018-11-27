#![feature(const_fn)]
#![feature(allocator_api)]
#![feature(uniform_paths)]

pub mod shortest_path;
pub mod graph;
pub mod io;
pub mod benchmark;
pub mod rank_select_map;
pub mod import;
pub mod link_speed_estimates;
pub mod export;

mod index_heap;
mod in_range_option;
mod as_slice;
mod as_mut_slice;
