use std::fs::File;

extern crate rdma_sim;
use rdma_sim::node::switch::{Switch, nack_switch::NackSwitch, pfc_switch::{PFCSwitch, IngressPFCSwitch}, lossy_switch::LossySwitch};
use rdma_sim::flow::FlowSide;
use rdma_sim::{Scenario, SharedIngressVictimFlowScenario, IndependentVictimFlowScenario};

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

fn do_args() -> (String, String) {
    use clap::App;
    use clap::Arg;

    let matches = App::new("Scenario")
        .version("0.1")
        .author("Akshay Narayan <akshayn@mit.edu")
        .arg(Arg::with_name("switch_type")
            .help("Type of switch to use")
            .long("switch-type")
            .short("t")
            .takes_value(true)
            .possible_values(&["pfc", "ingresspfc", "nacks", "lossy"])
            .required(true))
        .arg(Arg::with_name("scenario")
            .help("Name of the scenario to run")
            .long("scenario")
            .short("r")
            .takes_value(true)
            .possible_values(&["shared_ingress_victim", "independent_victim"])
            .required(true))
        .get_matches();

    (
        matches.value_of("switch_type").unwrap().to_string(),
        matches.value_of("scenario").unwrap().to_string(),
    )
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

fn run_scenario_switch<C: Scenario, S: Switch>(logger: slog::Logger) {
    let e = C::make::<S>(Some(logger.clone()));
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

fn run_scenario<C: Scenario>(switch: &str, logger: slog::Logger) {
    match switch {
        "pfc" => run_scenario_switch::<C, PFCSwitch>(logger),
        "ingresspfc" => run_scenario_switch::<C, IngressPFCSwitch>(logger),
        "nacks" => run_scenario_switch::<C, NackSwitch>(logger),
        "lossy" => run_scenario_switch::<C, LossySwitch>(logger),
        _ => unreachable!(),
    }
}



fn main() {
    let (switch, scenario) = do_args();
    let slug = format!("{}-{}", scenario, switch);

    let logger = make_logger(slug.as_str());
    log_commit_hash(logger.clone());

    match scenario.as_str() {
        "shared_ingress_victim" => run_scenario::<SharedIngressVictimFlowScenario>(switch.as_str(), logger),
        "independent_victim" => run_scenario::<IndependentVictimFlowScenario>(switch.as_str(), logger),
        _ => unreachable!(),
    }

    viz::plot_log(slug.as_str(), 0).unwrap();
}
