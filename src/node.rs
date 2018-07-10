use std::vec::Vec;
use std::collections::VecDeque;
use std::fmt::Debug;

use super::{Nanos, Result};
use super::packet::Packet;
use super::event::{Event, EventTime};
use super::topology::Topology;

use super::flow::Flow;

/// A Node is an entity that can receive Packets.
pub trait Node : Debug {
    fn id(&self) -> u32;
    fn receive(&mut self, p: Packet) -> Result<Vec<Box<Event>>>;
    fn exec(&mut self) -> Result<Vec<Box<Event>>>;
    fn is_active(&self) -> bool;
}

/// Links are unidirectional
#[derive(Clone, Copy, Default, Debug)]
pub struct Link {
    pub propagation_delay: Nanos,
    pub bandwidth_bps: u64,
    pub to: u32,
}

#[derive(Default, Debug)]
pub struct Host {
    pub id: u32,
    pub active: bool,
    pub link: Link, // host does not need a Queue locally since it controls its own packet transmissions
    pub active_flows: Vec<Box<Flow>>,
    pub to_send: Vec<Packet>,
}

impl Host {
    pub fn push_pkt(&mut self, p: Packet) {
        println!("pushing packet onto {:?}", self.id);
        self.to_send.push(p)
    }

    pub fn flow_arrival(&mut self, f: Box<Flow>) {
        self.active_flows.push(f);
        self.active = true;
    }
}

impl Node for Host {
    fn id(&self) -> u32 {
        self.id
    }

    fn receive(&mut self, p: Packet) -> Result<Vec<Box<Event>>> {
        println!("{:?} got packet: {:?}", self.id, p);
        let active_flows = &mut self.active_flows;
        let pkts_to_send = &mut self.to_send;
        match p.clone() {
            Packet::Data{hdr, ..} | Packet::Ack{hdr, ..} | Packet::Nack{hdr, ..} => {
                let flow_id = hdr.id;
                if let Some(f) = active_flows.iter_mut().find(|f| f.flow_info().flow_id == flow_id) {
                    f.receive(p)
                        .map(|pkts| {
                            pkts_to_send.extend(pkts);
                        })?;
                    self.active = true;
                } else {
                    println!("got isolated packet {:?}", p);
                }
            }
            _ => unimplemented!(),
        }

        Ok(vec![])
    }

    fn exec(&mut self) -> Result<Vec<Box<Event>>> {
        let flows = &mut self.active_flows;
        let active = &mut self.active;
        let link = self.link;
        let id = self.id;

        let new_pkts = flows.iter_mut().flat_map(|f| f.exec().unwrap().into_iter());
        let pkts = &mut self.to_send;
        pkts.extend(new_pkts);
        pkts.pop().map_or_else(|| {
            *active = false;
            println!("now inactive {:?}", id);
            Err(format_err!("foo"))
        }, |pkt| {
            println!("transmitting pkt {:?}", pkt);
            Ok(vec![Box::new(LinkTransmitEvent(
                link,
                pkt,
            )) as Box<Event>])
        })
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
   
/// Queues are tied to a specfic link.
pub trait Queue : Debug {
    fn link(&self) -> Link;
    fn enqueue(&mut self, p: Packet);
    fn dequeue(&mut self) -> Option<Packet>;
    fn peek(&self) -> Option<&Packet>;
    fn is_active(&self) -> bool;
    fn set_active(&mut self, a: bool);
}

#[derive(Debug)]
pub struct DropTailQueue{
    limit_bytes: u32,
    link: Link,
    pkts: VecDeque<Packet>,
    active: bool,
}

impl DropTailQueue {
    pub fn new(limit_bytes: u32, link: Link) -> Self {
        DropTailQueue{
            limit_bytes,
            link,
            pkts: VecDeque::new(),
            active: false,
        }
    }

    fn occupancy_bytes(&self) -> u32 {
        self.pkts.iter().map(|p| p.get_size_bytes()).sum()
    }
}

impl Queue for DropTailQueue {
    fn link(&self) -> Link {
        self.link
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn set_active(&mut self, a: bool) {
        self.active = a;
    }
    
    fn enqueue(&mut self, p: Packet) {
        let occupancy_bytes = self.occupancy_bytes();
        if occupancy_bytes + p.get_size_bytes() > self.limit_bytes {
            // we have to drop this packet
            return;
        }

        self.pkts.push_back(p);
        self.set_active(true);
    }

    fn dequeue(&mut self) -> Option<Packet> {
        if self.pkts.len() == 1 {
            self.set_active(false);
        }

        self.pkts.pop_front()
    }
    
    fn peek(&self) -> Option<&Packet> {
        self.pkts.front()
    }
}

#[derive(Default, Debug)]
pub struct Switch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<Box<Queue>>,
    pub core: Vec<Box<Queue>>,
}

impl Node for Switch {
    fn id(&self) -> u32 {
        self.id
    }

    fn receive(&mut self, p: Packet) -> Result<Vec<Box<Event>>> {
        self.active = true;
        println!("switch {:?} got pkt: {:?}", self.id, p);
        // switches are output queued
        match p {
            Packet::Pause | Packet::Resume => unimplemented!(),
            Packet::Nack{..} => unimplemented!(),
            Packet::Ack{hdr, ..} |
            Packet::Data{hdr, ..} => {
				self.rack
                    .iter_mut()
                    .find(|ref mut q| {
                        let link_dst = q.link().to;
                        link_dst == hdr.to
                    })
					.map_or_else(|| unimplemented!(), |rack_link_queue| {
						// send packet out on rack_link_queue
						rack_link_queue.enqueue(p); // could result in a drop
                        Ok(vec![])
					})
            }
        }
    }

    fn exec(&mut self) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let evs = self.rack.iter_mut().chain(self.core.iter_mut())
            .filter(|q| {
                q.is_active()
            })
            .filter_map(|q| {
                if let Some(pkt) = q.dequeue() {
                    Some(
                        Box::new(
                            NodeTransmitEvent(q.link(), pkt)
                        ) as Box<Event>,
                    )
                } else {
                    q.set_active(false);
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

#[derive(Debug)]
pub struct LinkTransmitEvent(Link, Packet);

impl Event for LinkTransmitEvent {
    fn time(&self) -> EventTime {
        EventTime::Delta(self.0.propagation_delay)
    }

    fn exec<'a>(&mut self, topo: &mut Topology) -> Result<Vec<Box<Event>>> {
        let to = self.0.to;
        topo.lookup_node(to)?.receive(self.1.clone())
    }
}

#[derive(Debug)]
pub struct NodeTransmitEvent(Link, Packet);

impl Event for NodeTransmitEvent {
    fn time(&self) -> EventTime {
        let transmission_delay = self.1.get_size_bytes() as u64
            * 8 // to bits
            * 1_000_000_000 // to ns * bits / sec
            / self.0.bandwidth_bps; // to ns
        EventTime::Delta(transmission_delay)
    }

    fn exec<'a>(&mut self, _: &mut Topology) -> Result<Vec<Box<Event>>> {
        Ok(vec![
            Box::new(
                LinkTransmitEvent(self.0, self.1.clone())
            )
        ])
    }
}
