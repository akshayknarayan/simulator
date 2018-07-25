use std::vec::Vec;
use std::collections::HashMap;

use ::{Nanos, Result};
use event::Event;
use node::{NodeTransmitEvent, Link};
use packet::{Packet, PacketHeader};
use super::{Switch, Queue};

#[derive(Default, Debug)]
pub struct NackSwitch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<Box<Queue>>,
    pub core: Vec<Box<Queue>>,
    pub blocked_flows: HashMap<u32, u32>, // flow id -> expected seqno
}

impl Switch for NackSwitch {
    fn new(
        switch_id: u32,
        links: impl Iterator<Item=Box<Queue>>,
    ) -> Self {
        NackSwitch{
            id: switch_id,
            active: false,
            rack: links.collect::<Vec<Box<Queue>>>(),
            core: vec![],
            blocked_flows: HashMap::new(),
        }
    }

    fn id(&self) -> u32 {
        self.id
    }

    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>> {
        self.active = true;
        let id = self.id;
        println!("[{:?}] {:?} got pkt: {:?}", time, self.id, p);
        // switches are output queued
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
						// send packet out on rack_link_queue
						if let None = rack_link_queue.enqueue(p) {
                            // packet was dropped
                            println!("{:?} dropping pkt: {:?}", id, p);
                            return;
                        }
					});
                
                Ok(vec![])
            }
            Packet::Data{hdr, seq, ..} => {
                let mut progress_flow = false;
                if let Some(next_expected_seq) = self.blocked_flows.get(&hdr.id) {
                    if seq == *next_expected_seq {
                        progress_flow = true;
                    } else {
                        // this packet is going to be retransmitted anyway. drop it
                        return Ok(vec![]);
                    }
                }

                if progress_flow {
                    self.blocked_flows.remove(&hdr.id);
                }

                let blocked = &mut self.blocked_flows;
				let nack_pkt = self.rack
                    .iter_mut()
                    .find(|ref q| {
                        let link_dst = q.link().to;
                        link_dst == hdr.to
                    })
					.map_or_else(|| unimplemented!(), |rack_link_queue| {
						// send packet out on rack_link_queue
						if let None = rack_link_queue.enqueue(p) {
                            // packet was dropped
                            println!("{:?} dropping pkt: {:?}", id, p);

                            // add this packet to the list of dropped flows
                            blocked.insert(hdr.id, seq);
                            // TODO remove all packets from this flow from this queue
                            
                            // send NACK back to source
                            Some(Packet::Nack{
                                hdr: PacketHeader{
                                    id: hdr.id,
                                    from: hdr.to,
                                    to: hdr.from,
                                },
                                nacked_seq: seq,
                            })
                        } else {
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

    fn exec(&mut self, time: Nanos) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let id = self.id;
        let evs = self.rack.iter_mut().chain(self.core.iter_mut())
            .filter(|q| {
                q.is_active()
            })
            .filter_map(|q| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
                    println!("[{:?}] {:?} transmitted {:?}", time, id, pkt);
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
            .chain(self.core.iter_mut())
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