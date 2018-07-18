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
    fn is_active(&self) -> bool;
    fn set_active(&mut self, a: bool);
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
            Packet::Pause | Packet::Resume => unimplemented!(),
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
                        }
                        Ok(vec![])
					})
            }
        }
    }

    fn exec(&mut self, _time: Nanos) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let evs = self.rack.iter_mut().chain(self.core.iter_mut())
            .filter(|q| {
                q.is_active()
            })
            .filter_map(|q| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
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

    fn is_active(&self) -> bool {
        self.active
    }
}
