#[macro_use]
extern crate failure;
extern crate itertools;
#[macro_use]
extern crate slog;
extern crate slog_bunyan;
extern crate slog_term;

use failure::Error;
type Result<T> = std::result::Result<T, Error>;
pub type Nanos = u64;

pub mod event;
pub mod topology;
pub mod packet;
pub mod node;
pub mod flow;
pub mod congcontrol;

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;
    use slog;
    use super::topology::{Topology, TopologyStrategy};
    use super::topology::one_big_switch::{OneBigSwitch, OneBigSwitchPFC, OneBigSwitchNacks};
    use super::event::Executor;
    use super::node::switch::{Switch, lossy_switch::LossySwitch};
    use super::packet::{Packet, PacketHeader};
    use super::flow::{FlowArrivalEvent, FlowInfo};
    use super::congcontrol::ConstCwnd;

    /// Make a standard instance of `slog::Logger`.
    fn make_logger() -> slog::Logger {
        use std::sync::Mutex;
        use slog::Drain;
        use slog_bunyan;
        use slog_term;

        //let decorator = slog_term::PlainSyncDecorator::new(slog_term::TestStdoutWriter);
        //let drain = slog_term::FullFormat::new(decorator).build().filter_level(slog::Level::Debug).fuse();
        let drain = Mutex::new(slog_bunyan::default(slog_term::TestStdoutWriter)).fuse();
        slog::Logger::root(drain, o!())
    }

    fn setup_test() -> Executor<LossySwitch> {
        let t = OneBigSwitch::make_topology(2, 15_000, 1_000_000, 1_000_000);
        Executor::new(t, None)
    }
    
    #[test]
    fn send_one_packet() {
        let mut e = setup_test();

        {
            let pkt = Packet::Data{
                hdr: PacketHeader{
                    flow: 0,
                    from: 0,
                    to: 1,
                },
                seq: 0,
                length: 1460,
            };

            let topo = e.components().1;
            topo.lookup_host(0).unwrap().push_pkt(pkt);
        }

        let e = e.execute().unwrap();
        assert_eq!(e.current_time(), 26000000);
    }

    #[test]
    fn send_one_flow() {
        let mut e = setup_test();

        let flowinfo = FlowInfo{
            flow_id: 1,
            sender_id: 0,
            dest_id: 1,
            length_bytes: 4380, // 3 packet flow
            max_packet_length: 1460,
        };

        // starts at t = 1.0s
        let flow_arrival = Box::new(FlowArrivalEvent(flowinfo, 1_000_000_000, PhantomData::<ConstCwnd>)); 
        e.push(flow_arrival);
        let e = e.execute().unwrap();
        assert_eq!(e.current_time(), 1052640000);
    }
    
    #[test]
    fn two_flows_lossy() {
        let t = OneBigSwitch::make_topology(3, 15_000, 1_000_000, 1_000_000);
        two_flows_scenario(t)
    }

    #[test]
    fn two_flows_pfc() {
        let t = OneBigSwitchPFC::make_topology(3, 15_000, 1_000_000, 1_000_000);
        two_flows_scenario(t)
    }

    fn two_flows_scenario<S: Switch>(t: Topology<S>) {
        let mut e = Executor::new(t, None);

        let flow1 = FlowInfo{
            flow_id: 1,
            sender_id: 1,
            dest_id: 0,
            length_bytes: 43800, // 30 packet flow
            max_packet_length: 1460,
        };
        
        let flow2 = FlowInfo{
            flow_id: 2,
            sender_id: 2,
            dest_id: 0,
            length_bytes: 43800, // 30 packet flow
            max_packet_length: 1460,
        };

        // starts at t = 1.0s
        let flow_arrival = Box::new(FlowArrivalEvent(flow1, 1_000_000_000, PhantomData::<ConstCwnd>)); 
        e.push(flow_arrival);
        let flow_arrival = Box::new(FlowArrivalEvent(flow2, 1_000_000_000, PhantomData::<ConstCwnd>)); 
        e.push(flow_arrival);

        let mut e = e.execute().unwrap();
        assert!(e.components().1.all_flows().all(|f| f.completion_time().is_some()));
    }

    #[test]
    fn victim_flow_lossy() {
        let t = OneBigSwitch::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    #[test]
    fn victim_flow_pfc() {
        let t = OneBigSwitchPFC::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    #[test]
    fn victim_flow_nacks() {
        let t = OneBigSwitchNacks::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    fn victim_flow_scenario<S: Switch>(t: Topology<S>) {
        let mut e = Executor::new(t, make_logger());

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

        let mut e = e.execute().unwrap();
        assert!(e.components().1.all_flows().all(|f| f.completion_time().is_some()));
    }
}
