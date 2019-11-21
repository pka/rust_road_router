use std::{env, error::Error, path::Path};

use bmw_routing_engine::{
    cli::CliErr,
    graph::time_dependent::*,
    io::Load,
    shortest_path::{customizable_contraction_hierarchy, node_order::NodeOrder, query::time_dependent_sampling::Server},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    args.next();

    let arg = &args.next().ok_or(CliErr("No directory arg given"))?;
    let path = Path::new(arg);

    let first_out = Vec::load_from(path.join("first_out").to_str().unwrap())?;
    let head = Vec::load_from(path.join("head").to_str().unwrap())?;
    let first_ipp_of_arc = Vec::load_from(path.join("first_ipp_of_arc").to_str().unwrap())?;
    let ipp_departure_time = Vec::load_from(path.join("ipp_departure_time").to_str().unwrap())?;
    let ipp_travel_time = Vec::load_from(path.join("ipp_travel_time").to_str().unwrap())?;

    println!("nodes: {}, arcs: {}, ipps: {}", first_out.len() - 1, head.len(), ipp_departure_time.len());

    let graph = TDGraph::new(first_out, head, first_ipp_of_arc, ipp_departure_time, ipp_travel_time);
    let cch_order = Vec::load_from(path.join("cch_perm").to_str().unwrap())?;

    let cch = customizable_contraction_hierarchy::contract(&graph, NodeOrder::from_node_order(cch_order));
    let mut server = Server::new(graph, &cch);
    println!("{:?}", server.distance(0, 1, 42));

    Ok(())
}
