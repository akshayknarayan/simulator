use std::vec::Vec;
use std::fmt::Debug;

use ::{Nanos, Result};
use event::Event;
use node::{Node, NodeTransmitEvent, Link};
use packet::Packet;
   
/// Queues are tied to a specfic link.
pub trait Queue : Debug {
    fn link(&self) -> Link;
    fn enqueue(&mut self, p: Packet) -> Option<()>;
    fn dequeue(&mut self) -> Option<Packet>;
    fn peek(&self) -> Option<&Packet>;
    fn headroom(&self) -> u32;
    fn is_active(&self) -> bool;
    fn set_active(&mut self, a: bool);
    fn is_paused(&self) -> bool;
    fn set_paused(&mut self, a: bool);
}

pub mod drop_tail_queue;

#[derive(Default, Debug)]
pub struct Switch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<Box<Queue>>,
    pub core: Vec<Box<Queue>>,
}

impl Switch {
    pub fn reactivate(&mut self, l: Link) {
        assert_eq!(l.from, self.id);
        self.rack.iter_mut()
            .chain(self.core.iter_mut())
            .find(|ref q| {
                q.link().to == l.to
            })
            .map_or_else(|| unimplemented!(), |link_queue| {
                link_queue.set_active(true);
            });
    }

    fn pause_incoming(&mut self) {
        let id = self.id;
        // send pauses to upstream queues
        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
            .filter(|q| {
                q.link().from != id
            })
        .for_each(|q| {
            q.enqueue(Packet::Pause(id)).unwrap();
        });
    }

    fn resume_incoming(&mut self) {
        let id = self.id;
                
        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
            .filter(|q| {
                q.link().from != id
            })
            .for_each(|q| {
                q.enqueue(Packet::Resume(id));
            });

    }
}

impl Node for Switch {
    fn id(&self) -> u32 {
        self.id
    }

    fn receive(&mut self, p: Packet, _time: Nanos) -> Result<Vec<Box<Event>>> {
        self.active = true;
        let id = self.id;
        println!("{:?} got pkt: {:?}", id, p);
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
                        rack_link_queue.set_paused(false);
                    });

                self.pause_incoming();
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
                        rack_link_queue.set_paused(true);
                    });

                self.resume_incoming();
                Ok(vec![])
			},
            Packet::Nack{hdr, ..} |
            Packet::Ack{hdr, ..} |
            Packet::Data{hdr, ..} => {
                let mut should_pause = false;
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
                        }

                        if rack_link_queue.headroom() < rack_link_queue.link().pfc_pause_threshold() {
                            // outgoing queue has filled up
                            should_pause = true;
                        }
					});
                
                if should_pause {
                    self.pause_incoming();
                }

                Ok(vec![])
            }
        }
    }

    fn exec(&mut self, _time: Nanos) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let mut should_resume = false;
        let evs = self.rack.iter_mut().chain(self.core.iter_mut())
            .filter(|q| {
                q.is_active()
            })
            .filter_map(|q| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
                    // check if queue is sufficiently empty
                    if q.is_paused() && q.headroom() > q.link().pfc_resume_threshold() {
                        should_resume = true;
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

        if should_resume {
            self.resume_incoming();
        }

        Ok(evs)
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
