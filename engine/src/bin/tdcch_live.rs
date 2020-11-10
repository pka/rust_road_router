// Metric dependent part of CATCHUp preprocessing - the customization - with reporting for experiments.
// Takes as input one directory arg which should contain the all data and to which results will be written.

use csv::ReaderBuilder;
use std::{env, error::Error, fs::File, path::Path};

#[macro_use]
extern crate rust_road_router;
use rust_road_router::{
    algo::customizable_contraction_hierarchy::*,
    cli::CliErr,
    datastr::{
        graph::{floating_time_dependent::*, *},
        node_order::NodeOrder,
    },
    io::*,
    report::*,
};

fn main() -> Result<(), Box<dyn Error>> {
    let _reporter = enable_reporting();

    report!("program", "tdcch");
    report!("start_time", format!("{}", time::now_utc().rfc822()));
    report!("args", env::args().collect::<Vec<String>>());
    report!("num_threads", rayon::current_num_threads());

    let mut args = env::args();
    args.next();

    let arg = &args.next().ok_or(CliErr("No directory arg given"))?;
    let path = Path::new(arg);

    let first_out = Vec::load_from(path.join("first_out"))?;
    let head = Vec::load_from(path.join("head"))?;
    let first_ipp_of_arc = Vec::load_from(path.join("first_ipp_of_arc"))?;
    let ipp_departure_time = Vec::<u32>::load_from(path.join("ipp_departure_time"))?;
    let ipp_travel_time = Vec::<u32>::load_from(path.join("ipp_travel_time"))?;

    report!("unprocessed_graph", { "num_nodes": first_out.len() - 1, "num_arcs": head.len(), "num_ipps": ipp_departure_time.len() });

    let graph = TDGraph::new(first_out, head, first_ipp_of_arc, ipp_departure_time, ipp_travel_time);

    report!("graph", { "num_nodes": graph.num_nodes(), "num_arcs": graph.num_arcs(), "num_ipps": graph.num_ipps(), "num_constant_ttfs": graph.num_constant() });

    let mut algo_runs_ctxt = push_collection_context("algo_runs".to_string());

    let cch_folder = path.join("cch");
    let node_order = NodeOrder::reconstruct_from(&cch_folder)?;
    let cch = CCHReconstrctor {
        original_graph: &graph,
        node_order,
    }
    .reconstruct_from(&cch_folder)?;

    let file = File::open(args.next().unwrap()).unwrap();
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b';')
        .quoting(false)
        .double_quote(false)
        .escape(None)
        .from_reader(file);

    let mut live = vec![None; graph.num_arcs()];
    let t_live = (7 * 3600 + 47 * 60) * 1000;

    for line in reader.records() {
        let record = line?;
        let from = record[0].parse()?;
        let to = record[1].parse()?;
        let speed: u32 = record[2].parse()?;
        let distance: u32 = record[3].parse()?;
        let duration: u32 = record[4].parse()?;

        if speed == 0 || duration > 3600 * 5 {
            continue;
        }
        if let Some(edge_idx) = graph.edge_index(from, to) {
            let edge_idx = edge_idx as usize;

            let new_tt = 100 * 36 * distance / speed;
            live[edge_idx] = Some((new_tt, t_live + duration * 1000))
        }
    }

    let live_graph = LiveGraph::new(graph, Timestamp::new(f64::from(t_live / 1000)), &live);

    let _cch_customization_ctxt = algo_runs_ctxt.push_collection_item();
    ftd_cch::customize_live(&cch, &live_graph);

    Ok(())
}
