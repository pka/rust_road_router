#![feature(plugin)]
#![feature(custom_derive)]
#![plugin(rocket_codegen)]

#[macro_use]
extern crate serde_derive;

extern crate rocket;
extern crate rocket_contrib;
extern crate bmw_routing_engine;

use std::path::{Path, PathBuf};
use std::env;
use std::iter::once;

use std::thread;
use std::sync::mpsc;
use mpsc::Sender;
use std::sync::Mutex;

use std::cmp::Ordering;
use rocket::response::NamedFile;
use rocket::State;
use rocket_contrib::Json;

use bmw_routing_engine::*;
use graph::*;
use rank_select_map::*;
use import::here::link_id_mapper::*;
use shortest_path::customizable_contraction_hierarchy;
use shortest_path::node_order::NodeOrder;
use shortest_path::query::customizable_contraction_hierarchy::Server;
use io::*;
use bmw_routing_engine::benchmark::measure;
use graph::link_id_to_tail_mapper::*;

#[derive(PartialEq,PartialOrd)]
struct NonNan(f32);

impl NonNan {
    fn new(val: f32) -> Option<NonNan> {
        if val.is_nan() {
            None
        } else {
            Some(NonNan(val))
        }
    }
}

impl Eq for NonNan {}

impl Ord for NonNan {
    fn cmp(&self, other: &NonNan) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[derive(Debug, FromForm, Copy, Clone)]
struct GeoQuery {
    from_lat: f32,
    from_lng: f32,
    to_lat: f32,
    to_lng: f32
}

#[derive(Debug, Serialize, Deserialize)]
struct GeoResponse {
    distance: Weight,
    path: Vec<(f32, f32)>
}

#[derive(Debug, FromForm, Copy, Clone)]
struct HereQuery {
    from_link_id: u64,
    from_direction: bool,
    from_link_fraction: f32,
    to_link_id: u64,
    to_direction: bool,
    to_link_fraction: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct HereResponse {
    distance: Weight,
    path: Vec<u64>
}

#[derive(Debug)]
enum Query {
    Geo((GeoQuery, Sender<Option<GeoResponse>>)),
    Here((HereQuery, Sender<Option<HereResponse>>)),
}

#[get("/")]
fn index() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/index.html")).ok()
}

#[get("/<file..>")]
fn files(file: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join(file)).ok()
}

#[get("/query?<query_params>", format = "application/json")]
fn query(query_params: GeoQuery, state: State<Mutex<Sender<Query>>>) -> Json<Option<GeoResponse>> {
    let result = measure("Total Query Request Time", || {
        println!("Received Query: {:?}", query_params);

        let tx_query = state.lock().unwrap();
        let (tx_result, rx_result) = mpsc::channel::<Option<GeoResponse>>();

        tx_query.send(Query::Geo((query_params, tx_result))).unwrap();
        rx_result.recv().expect("routing engine crashed or hung up")
    });

    println!("");
    Json(result)
}

#[get("/here_query?<query_params>", format = "application/json")]
fn here_query(query_params: HereQuery, state: State<Mutex<Sender<Query>>>) -> Json<Option<HereResponse>> {
    let result = measure("Total Query Request Time", || {
        println!("Received Query: {:?}", query_params);

        let tx_query = state.lock().unwrap();
        let (tx_result, rx_result) = mpsc::channel::<Option<HereResponse>>();

        tx_query.send(Query::Here((query_params, tx_result))).unwrap();
        rx_result.recv().expect("routing engine crashed or hung up")
    });

    println!("");
    Json(result)
}

fn main() {
    let (tx_query, rx_query) = mpsc::channel::<Query>();

    thread::spawn(move || {
        let mut args = env::args();
        args.next();

        let arg = &args.next().expect("No directory arg given");
        let path = Path::new(arg);

        let first_out = Vec::load_from(path.join("first_out").to_str().unwrap()).expect("could not read first_out");
        let head = Vec::load_from(path.join("head").to_str().unwrap()).expect("could not read head");
        let travel_time = Vec::load_from(path.join("travel_time").to_str().unwrap()).expect("could not read travel_time");

        let lat = Vec::load_from(path.join("latitude").to_str().unwrap()).expect("could not read latitude");
        let lng = Vec::load_from(path.join("longitude").to_str().unwrap()).expect("could not read longitude");

        let link_id_mapping = BitVec::load_from(path.join("link_id_mapping").to_str().unwrap()).expect("could not read link_id_mapping");
        let link_id_mapping = InvertableRankSelectMap::new(RankSelectMap::new(link_id_mapping));
        let here_rank_to_link_id = Vec::load_from(path.join("here_rank_to_link_id").to_str().unwrap()).expect("could not read here_rank_to_link_id");
        let id_mapper = LinkIdMapper::new(link_id_mapping, here_rank_to_link_id, head.len());

        let graph = FirstOutGraph::new(first_out, head, travel_time);
        let cch_order = Vec::load_from(path.join("cch_perm").to_str().unwrap()).expect("could not read cch_perm");

        let link_id_to_tail_mapper = LinkIdToTailMapper::new(&graph);

        let cch = customizable_contraction_hierarchy::contract(&graph, NodeOrder::from_node_order(cch_order));

        let mut server = Server::new(&cch, &graph);

        let coords = |node: NodeId| -> (f32, f32) {
            (lat[node as usize], lng[node as usize])
        };

        let closest_node = |(p_lat, p_lng): (f32, f32)| -> NodeId {
            lat.iter().zip(lng.iter()).enumerate().min_by_key(|&(_, (lat, lon))| {
                NonNan::new(((lat - p_lat) * (lat - p_lat) + (lon - p_lng) * (lon - p_lng)).sqrt())
            }).map(|(id, _)| id).unwrap() as NodeId
        };

        for query_params in rx_query {
            match query_params {
                Query::Geo((GeoQuery { from_lat, from_lng, to_lat, to_lng }, tx_result)) => {
                    let (from, to) = measure("match nodes", || {
                        (closest_node((from_lat, from_lng)), closest_node((to_lat, to_lng)))
                    });

                    let result = measure("cch query", || {
                        server.distance(from, to).map(|distance| {
                            let path = server.path().iter().map(|&node| coords(node)).collect();
                            GeoResponse { distance, path }
                        })
                    });

                    tx_result.send(result).unwrap();
                },
                Query::Here((HereQuery { from_link_id, from_direction, from_link_fraction, to_link_id, to_direction, to_link_fraction }, tx_result)) => {
                    let from_link_direction = if from_direction { LinkDirection::FromRef } else { LinkDirection::ToRef };
                    let from_link_local_id = id_mapper.here_to_local_link_id(from_link_id, from_link_direction).expect("non existing link");
                    let from_link = graph.link(from_link_local_id);
                    let from = from_link.node;

                    let to_link_direction = if to_direction { LinkDirection::FromRef } else { LinkDirection::ToRef };
                    let to_link_local_id = id_mapper.here_to_local_link_id(to_link_id, to_link_direction).expect("non existing link");
                    let to_link = graph.link(to_link_local_id);
                    let to = link_id_to_tail_mapper.link_id_to_tail(to_link_local_id);

                    let result = measure("cch query", || {
                        server.distance(from, to).map(|distance| {
                            let path = server.path();
                            let path_iter = path.iter();
                            let mut second_node_iter = path_iter.clone();
                            second_node_iter.next();

                            let path = once(from_link_id).chain(path_iter.zip(second_node_iter).map(|(first_node, second_node)| {
                                graph.edge_index(*first_node, *second_node).unwrap()
                            }).map(|link_id| {
                                id_mapper.local_to_here_link_id(link_id)
                            })).chain(once(to_link_id)).collect();

                            let distance = distance + (from_link_fraction * from_link.weight as f32) as u32 + (to_link_fraction * to_link.weight as f32) as u32;
                            HereResponse { distance, path }
                        })
                    });

                    tx_result.send(result).unwrap();
                },
            }
        }
    });

    let config = rocket::config::Config::build(rocket::config::Environment::Staging)
        .port(8888)
        .finalize().expect("Could not create config");

    rocket::custom(config, false)
        .mount("/", routes![index, files, query, here_query])
        .manage(Mutex::new(tx_query))
        .launch();
}
