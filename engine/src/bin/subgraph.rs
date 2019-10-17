use std::env;
use std::path::Path;

use bmw_routing_engine::{
    graph::*,
    io::*,
    rank_select_map::*,
};

fn main() {
    let mut args = env::args();
    args.next();

    let arg = &args.next().expect("No directory arg given");
    let path = Path::new(arg);

    let min_lat = args.next().map(|s| s.parse::<f32>().unwrap_or_else(|_| panic!("could not parse {} as lat coord", s))).unwrap();
    let min_lon = args.next().map(|s| s.parse::<f32>().unwrap_or_else(|_| panic!("could not parse {} as lon coord", s))).unwrap();
    let max_lat = args.next().map(|s| s.parse::<f32>().unwrap_or_else(|_| panic!("could not parse {} as lat coord", s))).unwrap();
    let max_lon = args.next().map(|s| s.parse::<f32>().unwrap_or_else(|_| panic!("could not parse {} as lon coord", s))).unwrap();

    let arg = &args.next().expect("No out directory arg given");
    let out_path = Path::new(arg);

    let first_out = Vec::load_from(path.join("first_out").to_str().unwrap()).expect("could not read first_out");
    let head = Vec::load_from(path.join("head").to_str().unwrap()).expect("could not read head");
    let travel_time = Vec::load_from(path.join("travel_time").to_str().unwrap()).expect("could not read travel_time");
    let lat = Vec::<f32>::load_from(path.join("latitude").to_str().unwrap()).expect("could not read latitude");
    let lng = Vec::<f32>::load_from(path.join("longitude").to_str().unwrap()).expect("could not read longitude");

    let mut new_first_out = Vec::<u32>::new();
    let mut new_head = Vec::<u32>::new();
    let mut new_travel_time = Vec::new();
    let mut new_lat = Vec::new();
    let mut new_lng = Vec::new();

    new_first_out.push(0);

    let in_bounding_box = |node| {
        lat[node] >= min_lat && lat[node] <= max_lat && lng[node] >= min_lon && lng[node] <= max_lon
    };

    let graph = FirstOutGraph::new(&first_out[..], &head[..], &travel_time[..]);
    let mut new_nodes = BitVec::new(graph.num_nodes());

    for node in 0..graph.num_nodes() {
        if in_bounding_box(node) {
            new_nodes.set(node);
        }
    }

    let id_map = RankSelectMap::new(new_nodes);

    for node in 0..graph.num_nodes() {
        if in_bounding_box(node) {
            new_first_out.push(*new_first_out.last().unwrap());
            new_lat.push(lat[node]);
            new_lng.push(lng[node]);

            for link in graph.neighbor_iter(node as NodeId) {
                if in_bounding_box(link.node as usize) {
                    *new_first_out.last_mut().unwrap() += 1;
                    new_head.push(id_map.get(link.node as usize).unwrap() as u32);
                    new_travel_time.push(link.weight);
                }
            }
        }
    }

    new_first_out.write_to(out_path.join("first_out").to_str().unwrap()).expect("could not write first_out");
    new_head.write_to(out_path.join("head").to_str().unwrap()).expect("could not write head");
    new_travel_time.write_to(out_path.join("travel_time").to_str().unwrap()).expect("could not write travel_time");
    new_lat.write_to(out_path.join("latitude").to_str().unwrap()).expect("could not write latitude");
    new_lng.write_to(out_path.join("longitude").to_str().unwrap()).expect("could not write longitude");
}