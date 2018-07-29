use std::marker::PhantomData;
use std::fs::File;

extern crate rdma_sim;
use rdma_sim::topology::{Topology, TopologyStrategy};
use rdma_sim::topology::one_big_switch::OneBigSwitch;
use rdma_sim::event::Executor;
use rdma_sim::node::switch::{Switch, nack_switch::NackSwitch};
use rdma_sim::flow::{FlowArrivalEvent, FlowInfo};
use rdma_sim::congcontrol::ConstCwnd;

extern crate viz;
use viz::{SlogJSONReader, VizWriter, TikzWriter}; 

extern crate failure;

#[macro_use] extern crate slog;
extern crate slog_bunyan;
    
/// Make a standard instance of `slog::Logger`.
fn make_logger(filename: &str) -> slog::Logger {
    use std::sync::Mutex;
    use slog::Drain;
    use slog_bunyan;

    let f = File::create(filename).unwrap();
    let drain = Mutex::new(slog_bunyan::default(f)).fuse();
    slog::Logger::root(drain, o!())
}

fn victim_flow_scenario<S: Switch>(t: Topology<S>, logfile: &str) {
    let mut e = Executor::new(t, make_logger(logfile));

    let flow = FlowInfo{
        flow_id: 0,
        sender_id: 0,
        dest_id: 1,
        length_bytes: 43800, // 30 packet flow
        max_packet_length: 1460,
    };

    // starts at t = 1.1s
    let flow_arrival = Box::new(FlowArrivalEvent(flow, 1_100_000_000, PhantomData::<ConstCwnd>));
    e.push(flow_arrival);

    let flow = FlowInfo{
        flow_id: 1,
        sender_id: 2,
        dest_id: 0,
        length_bytes: 43800, // 30 packet flow
        max_packet_length: 1460,
    };

    // starts at t = 1.0s
    let flow_arrival = Box::new(FlowArrivalEvent(flow, 1_000_000_000, PhantomData::<ConstCwnd>));
    e.push(flow_arrival);

    let flow = FlowInfo{
        flow_id: 2,
        sender_id: 3,
        dest_id: 0,
        length_bytes: 43800, // 30 packet flow
        max_packet_length: 1460,
    };

    // starts at t = 1.0s
    let flow_arrival = Box::new(FlowArrivalEvent(flow, 1_000_000_000, PhantomData::<ConstCwnd>));
    e.push(flow_arrival);

    e.execute().unwrap();
}

fn plot_log(logfile: &str, outfile: &str) -> Result<(), failure::Error> {
    let logfile = File::open(logfile)?;
    let outfile = File::create(outfile)?;
    let reader = std::io::BufReader::new(logfile);
    let reader = SlogJSONReader::new(reader);
    let mut writer = TikzWriter::new(outfile);
    writer.dump_events(
        reader
            .get_events()
            .filter(|e| {
                e.annotation().as_str().starts_with("1")
            }),
    )?;
    Ok(())
}

fn compile_viz(outfile: &str) -> Result<(), failure::Error> {
    use std::process::Command;

    Command::new("pdflatex")
        .arg(outfile)
        .spawn()?
        .wait()?;

    Ok(())
}

fn main() {
    let t = OneBigSwitch::<NackSwitch>::make_topology(4, 15_000, 1_000_000, 1_000_000);
    let logfile = "victimflow-nacks.tr";
    let outfile = "victimflow-nacks.tex";
    victim_flow_scenario(t, logfile);
    plot_log(logfile, outfile).unwrap();
    compile_viz(outfile).unwrap();
}
