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
    use super::topology::one_big_switch::OneBigSwitch;
    use super::event::Executor;
    use super::node::switch::{Switch, lossy_switch::LossySwitch, nack_switch::NackSwitch, pfc_switch::PFCSwitch};
    use super::packet::{Packet, PacketHeader};
    use super::flow::{FlowArrivalEvent, FlowInfo};
    use super::congcontrol::ConstCwnd;

    /// Make a standard instance of `slog::Logger`.
    fn make_logger(logfile: Option<&str>) -> slog::Logger {
        use std::sync::Mutex;
        use std::fs::File;
        use slog::Drain;
        use slog_bunyan;
        use slog_term;

        if let Some(fln) = logfile {
            let f = File::create(fln).unwrap();
            let json_drain = Mutex::new(slog_bunyan::default(f)).fuse();
            slog::Logger::root(json_drain, o!())
        } else {
            let decorator = slog_term::PlainSyncDecorator::new(slog_term::TestStdoutWriter);
            let human_drain = slog_term::FullFormat::new(decorator).build().filter_level(slog::Level::Debug).fuse();
            slog::Logger::root(human_drain, o!())
        }
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

    mod nack_test_switch {
        use ::{Nanos, Result};
        use event::Event;
        use node::{Link};
        use packet::{Packet, PacketHeader};
        use node::switch::{Switch, Queue, nack_switch::NackSwitch};
        use slog;

        #[derive(Default, Debug)]
        pub struct NackTestSwitch(NackSwitch, usize);

        impl Switch for NackTestSwitch {
            fn new(
                switch_id: u32,
                links: impl Iterator<Item=Box<Queue>>,
            ) -> Self {
                NackTestSwitch(NackSwitch::new(switch_id, links), 0)
            }

            fn id(&self) -> u32 {
                self.0.id()
            }

            fn receive(&mut self, p: Packet, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
                match p {
                    Packet::Data{hdr,seq,..} => {
                        self.1 += 1;

                        if self.1 == 3 {
                            if let Some(log) = logger {
                                debug!(log, "rx";
                                    "time" => time,
                                    "node" => self.0.id(),
                                    "packet" => ?p,
                                );
                            }

                            // drop 3rd packet and send NACK
                            let nack_pkt = Packet::Nack{
                                hdr: PacketHeader{
                                    flow: hdr.flow,
                                    from: hdr.to,
                                    to: hdr.from,
                                },
                                nacked_seq: seq,
                            };
                            
                            self.0.blocked_flows.insert(hdr.flow, seq);

                            let q = self.0.rack
                                .iter_mut()
                                .find(|ref q| {
                                    let link_dst = q.link().to;
                                    match nack_pkt {
                                        Packet::Nack{hdr, ..} => link_dst == hdr.to,
                                        _ => unreachable!(),
                                    }
                                })
                                .unwrap();
                            q.enqueue(nack_pkt).unwrap();
                            return Ok(vec![]);
                        }
                    }
                    _ => (),
                };

                self.0.receive(p, time, logger)
            }

            fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
                self.0.exec(time, logger)
            }
            
            fn reactivate(&mut self, l: Link) {
                self.0.reactivate(l)
            }

            fn is_active(&self) -> bool {
                self.0.is_active()
            }
        }
    }

    #[test]
    fn one_flow_with_nack() {
        let t = OneBigSwitch::<nack_test_switch::NackTestSwitch>::make_topology(2, 15_000, 1_000_000, 1_000_000);
        let mut e = Executor::new(t, make_logger(Some("oneflow_nack.tr")));

        let flowinfo = FlowInfo{
            flow_id: 1,
            sender_id: 0,
            dest_id: 1,
            length_bytes: 14600, // 10 packet flow
            max_packet_length: 1460,
        };

        // starts at t = 1.0s
        let flow_arrival = Box::new(FlowArrivalEvent(flowinfo, 1_000_000_000, PhantomData::<ConstCwnd>)); 
        e.push(flow_arrival);
        let e = e.execute().unwrap();

        // H0 - Switch - H1
        // 1ms propagation delay, 
        // 1Mbps link
        // 1500Byte packets -> 12m transmission delay
        // 40Byte ACKs -> 320us transmission delay
        //
        // 36ms: H0 finishes tx D3, starts tx D4
        // 37ms: D3 rx switch, is dropped, NACK3 tx
        // 38ms: NACK3 rx at H0
        // 48ms: H0 finishes tx D4 (will be dropped at switch), start tx D3
        // 60ms: H0 finishes tx D3
        // ...
        // 144ms: H0 finishes tx D10
        // 157ms: Switch finishes rx D10, starts tx D10 to H1
        // 158ms: H1 rx D10
        // 159ms + 320us: Switch tx A10
        // 160ms + 640us: H0 rx A10
        assert_eq!(e.current_time(), 1_160_640_000);
    }
    
    #[test]
    fn two_flows_lossy() {
        let t = OneBigSwitch::<LossySwitch>::make_topology(3, 15_000, 1_000_000, 1_000_000);
        two_flows_scenario(t)
    }

    #[test]
    fn two_flows_pfc() {
        let t = OneBigSwitch::<PFCSwitch>::make_topology(3, 15_000, 1_000_000, 1_000_000);
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
        let t = OneBigSwitch::<LossySwitch>::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    #[test]
    fn victim_flow_pfc() {
        let t = OneBigSwitch::<PFCSwitch>::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    #[test]
    fn victim_flow_nacks() {
        let t = OneBigSwitch::<NackSwitch>::make_topology(4, 15_000, 1_000_000, 1_000_000);
        victim_flow_scenario(t);
    }

    fn victim_flow_scenario<S: Switch>(t: Topology<S>) {
        let mut e = Executor::new(t, make_logger(None));

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
