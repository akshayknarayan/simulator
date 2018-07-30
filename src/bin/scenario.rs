use std::marker::PhantomData;
use std::fs::File;

extern crate rdma_sim;
use rdma_sim::topology::{Topology, TopologyStrategy};
use rdma_sim::topology::one_big_switch::OneBigSwitch;
use rdma_sim::event::Executor;
use rdma_sim::node::switch::{Switch, nack_switch::NackSwitch, pfc_switch::PFCSwitch, lossy_switch::LossySwitch};
use rdma_sim::flow::{FlowArrivalEvent, FlowInfo, FlowSide};
use rdma_sim::congcontrol::ConstCwnd;

extern crate viz;
extern crate clap;
extern crate failure;
#[macro_use] extern crate slog;
extern crate slog_bunyan;
extern crate slog_term;
    
/// Make a standard instance of `slog::Logger`.
fn make_logger(slug: &str) -> slog::Logger {
    use std::sync::Mutex;
    use slog::Drain;
    use slog_bunyan;
    use slog_term;

    let filename = format!("{}.tr", slug);
    let f = File::create(filename).unwrap();
    let json_drain = Mutex::new(slog_bunyan::default(f)).fuse();

    let decorator = slog_term::TermDecorator::new().build();
    let term_drain = slog_term::CompactFormat::new(decorator).build();
    let term_drain = std::sync::Mutex::new(term_drain).filter_level(slog::Level::Info).fuse();
    slog::Logger::root(slog::Duplicate::new(
        json_drain,
        term_drain,
    ).fuse(), o!())
}

fn do_args() -> String {
    use clap::App;
    use clap::Arg;

    let matches = App::new("Scenario")
        .version("0.1")
        .author("Akshay Narayan <akshayn@mit.edu")
        .arg(Arg::with_name("switch_type")
            .help("Type of switch to use")
            .long("switch-type")
            .short("s")
            .takes_value(true)
            .possible_values(&["pfc", "nacks", "lossy"])
            .required(true))
        .get_matches();

    matches.value_of("switch_type").unwrap().to_string()
}

fn log_commit_hash(logger: slog::Logger) {
    use std::process::Command;

    let cmd = Command::new("git")
        .arg("log")
        .arg("--no-decorate")
        .arg("--oneline")
        .args(&["-n", "1"])
        .output()
        .unwrap();
    let stdout = std::str::from_utf8(&cmd.stdout).unwrap();
    info!(logger, "commit";
          "log" => stdout.trim(),
    );
}

fn victim_flow_scenario<S: Switch>(t: Topology<S>, logger: slog::Logger) -> Executor<S> {
    let mut e = Executor::new(t, logger.clone());

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

    e
}

fn run_scenario<S: Switch>(e: Executor<S>, logger: slog::Logger) {
    let mut e = e.execute().unwrap();
    for f in e.components().1
        .all_flows()
        .filter(|f| match f.side() {
            FlowSide::Sender => true,
            _ => false,
        }) {

        info!(logger, "fct";
            "victim" => f.flow_info().flow_id == 0,
            "id" => f.flow_info().flow_id,
            "fct" => f.completion_time().unwrap(),
        );
    }
}

macro_rules! scenario {
    ($t: ty, $logger: expr) => {{
        let t = OneBigSwitch::<$t>::make_topology(4, 15_000, 1_000_000, 1_000_000);
        let scenario = victim_flow_scenario(t, $logger);
        run_scenario(scenario, $logger);
    }}
}

fn main() {
    let switch_opt = do_args();
    let slug = format!("victimflow-nacks-{}", switch_opt);

    let logger = make_logger(slug.as_str());
    log_commit_hash(logger.clone());

    match switch_opt.as_str() {
        "pfc" => scenario!(PFCSwitch, logger.clone()),
        "nacks" => scenario!(NackSwitch, logger.clone()),
        "lossy" => scenario!(LossySwitch, logger.clone()),
        _ => unreachable!(),
    }

    viz::plot_log(slug.as_str()).unwrap();
}
