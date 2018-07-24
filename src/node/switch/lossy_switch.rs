use std::vec::Vec;

use ::{Nanos, Result};
use event::Event;
use node::{NodeTransmitEvent, Link};
use packet::Packet;
use super::{Switch, Queue};

#[derive(Default, Debug)]
pub struct LossySwitch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<Box<Queue>>,
    pub core: Vec<Box<Queue>>,
}

impl Switch for LossySwitch {
    fn id(&self) -> u32 {
        self.id
    }

    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>> {
        self.active = true;
        let id = self.id;
        println!("[{:?}] {:?} got pkt: {:?}", time, self.id, p);
        // switches are output queued
        match p {
            Packet::Pause(from) => {
				self.rack
					.iter_mut()
					.find(|ref q| {
						let link_src = q.link().from;
						link_src == from
					})
					.map_or_else(|| unimplemented!(), |rack_link_queue| {
                        rack_link_queue.set_paused(true);
                    });

                Ok(vec![])
			}
			Packet::Resume(from) => {
				self.rack
					.iter_mut()
					.find(|ref q| {
						let link_src = q.link().from;
						link_src == from
					})
					.map_or_else(|| unimplemented!(), |rack_link_queue| {
                        rack_link_queue.set_paused(false);
                    });

                Ok(vec![])
			},
            Packet::Nack{hdr, ..} |
            Packet::Ack{hdr, ..} |
            Packet::Data{hdr, ..} => {
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