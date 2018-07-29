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
        use node::{NodeTransmitEvent, Link};
        use packet::{Packet, PacketHeader};
        use node::switch::{Switch, Queue};
        use slog;

        #[derive(Default, Debug)]
        pub struct NackTestSwitch {
            id: u32,
            active: bool,
            count: usize,
            rack: Vec<Box<Queue>>,
        }

        impl Switch for NackTestSwitch {
            fn new(
                switch_id: u32,
                links: impl Iterator<Item=Box<Queue>>,
            ) -> Self {
                NackTestSwitch{
                    id: switch_id,
                    active: false,
                    count: 0,
                    rack: links.collect::<Vec<Box<Queue>>>(),
                }
            }

            fn id(&self) -> u32 {
                self.id
            }

            fn receive(&mut self, p: Packet, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
                self.active = true;
                if let Some(log) = logger {
                    debug!(log, "rx";
                        "time" => time,
                        "node" => self.id,
                        "packet" => ?p,
                    );
                }
                
                match p {
                    Packet::Pause(_) | Packet::Resume(_) => Ok(vec![]),
                    Packet::Nack{hdr, ..} |
                    Packet::Ack{hdr, ..} => {
                        self.rack
                            .iter_mut()
                            .find(|ref q| {
                                let link_dst = q.link().to;
                                link_dst == hdr.to
                            })
                            .map_or_else(|| unimplemented!(), |rack_link_queue| {
                                rack_link_queue.enqueue(p).unwrap();
                            });
                        
                        Ok(vec![])
                    }
                    Packet::Data{hdr, seq, ..} => {
                        let count = &mut self.count;
                        let nack_pkt = self.rack
                            .iter_mut()
                            .find(|ref q| {
                                let link_dst = q.link().to;
                                link_dst == hdr.to
                            })
                            .map_or_else(|| unimplemented!(), |rack_link_queue| {
                                *count += 1;
                                // send packet out on rack_link_queue
                                if *count == 3 {
                                    // drop 3rd packet and send NACK
                                    Some(Packet::Nack{
                                        hdr: PacketHeader{
                                            flow: hdr.flow,
                                            from: hdr.to,
                                            to: hdr.from,
                                        },
                                        nacked_seq: seq,
                                    })
                                } else {
                                    rack_link_queue.enqueue(p).unwrap();
                                    None
                                }
                            });

                        if let Some(nack) = nack_pkt {
                            let q = self.rack
                                .iter_mut()
                                .find(|ref q| {
                                    let link_dst = q.link().to;
                                    match nack {
                                        Packet::Nack{hdr, ..} => link_dst == hdr.to,
                                        _ => unreachable!(),
                                    }
                                })
                                .unwrap();
                            q.enqueue(nack).unwrap();
                        }
                        
                        Ok(vec![])
                    }
                }
            }

            fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
                let id = self.id;
                // step all queues forward
                let evs = self.rack.iter_mut()
                    .filter(|q| {
                        q.is_active()
                    })
                    .filter_map(|q| {
                        q.set_active(false);
                        if let Some(pkt) = q.dequeue() {
                            if let Some(log) = logger {
                                debug!(log, "tx";
                                    "time" => time,
                                    "node" => id,
                                    "packet" => ?pkt,
                                );
                            }

                            Some(
                                Box::new(
                                    NodeTransmitEvent(q.link(), pkt)
                                ) as Box<Event>,
                            )
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<Box<Event>>>();

                Ok(evs)
            }
            
            fn reactivate(&mut self, l: Link) {
                assert_eq!(l.from, self.id);
                self.rack.iter_mut()
                    .find(|q| {
                        q.link().to == l.to
                    })
                    .map_or_else(|| unimplemented!(), |link_queue| {
                        link_queue.set_active(true);
                    });
            }

            fn is_active(&self) -> bool {
                self.active
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
