use std::vec::Vec;
use std::collections::VecDeque;
use std::fmt::Debug;

use super::{Nanos, Result};
use super::packet::Packet;
use super::event::{Event, EventTime};
use super::topology::{Topology, TopologyNode};

use super::flow::Flow;

pub mod switch;

/// A Node is an entity that can receive Packets.
pub trait Node : Debug {
    fn id(&self) -> u32;
    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>>;
    fn exec(&mut self, time: Nanos) -> Result<Vec<Box<Event>>>;
    fn is_active(&self) -> bool;
}

/// Links are unidirectional
#[derive(Clone, Copy, Default, Debug)]
pub struct Link {
    pub propagation_delay: Nanos,
    pub bandwidth_bps: u64,
    pub from: u32,
    pub to: u32,
}

#[derive(Default, Debug)]
pub struct Host {
    pub id: u32,
    pub active: bool,
    pub paused: bool,
    pub link: Link, // host does not need a Queue locally since it controls its own packet transmissions
    pub active_flows: Vec<Box<Flow>>,
    pub to_send: VecDeque<Packet>,
}

impl Host {
    pub fn push_pkt(&mut self, p: Packet) {
        println!("pushing packet onto {:?}", self.id);
        self.to_send.push_back(p)
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

    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>> {
        println!("{:?} got packet: {:?}", self.id, p);
        let active_flows = &mut self.active_flows;
        let pkts_to_send = &mut self.to_send;
        match p.clone() {
            Packet::Data{hdr, ..} | Packet::Ack{hdr, ..} | Packet::Nack{hdr, ..} => {
                let flow_id = hdr.id;
                if let Some(f) = active_flows.iter_mut().find(|f| f.flow_info().flow_id == flow_id) {
                    f.receive(time, p).map(|pkts| { pkts_to_send.extend(pkts); })?;
                    self.active = true;
                } else {
                    println!("got isolated packet {:?}", p);
                }
            }
            Packet::Pause(_) => {
                assert_eq!(self.paused, false);
                self.paused = true;
            }
            Packet::Resume(_) => {
                assert_eq!(self.paused, true);
                self.paused = false;
            }
        }

        Ok(vec![])
    }

    fn exec(&mut self, time: Nanos) -> Result<Vec<Box<Event>>> {
        let flows = &mut self.active_flows;
        let active = &mut self.active;
        let link = self.link;

        if self.paused { 
            return Ok(vec![]);
        }

        let new_pkts = flows.iter_mut().flat_map(|f| f.exec(time).unwrap().into_iter());
        let pkts = &mut self.to_send;
        pkts.extend(new_pkts);
        *active = false;
        pkts.pop_front().map_or_else(|| {
            Err(format_err!("no more pending outgoing packets"))
        }, |pkt| {
            Ok(vec![Box::new(NodeTransmitEvent(link, pkt)) as Box<Event>])
        })
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

    fn exec<'a>(&mut self, time: Nanos, topo: &mut Topology) -> Result<Vec<Box<Event>>> {
        let to = self.0.to;
        match topo.lookup_node(to)? {
            TopologyNode::Host(n) => n.receive(self.1.clone(), time),
            TopologyNode::Switch(n) => n.receive(self.1.clone(), time),
        }
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

    fn exec<'a>(&mut self, _time: Nanos, topo: &mut Topology) -> Result<Vec<Box<Event>>> {
        let from = self.0.from;
        topo.lookup_node(from).map(|tn| {
            match tn {
                TopologyNode::Host(h) => { h.active = true; }
                TopologyNode::Switch(s) => {
                    s.reactivate(self.0);
                }
            }
        })
        .unwrap_or_else(|_| ()); // throw away failure (not host)
            
        println!("{:?} transmitted {:?}", from, self.1);

        Ok(vec![
            Box::new(
                LinkTransmitEvent(self.0, self.1)
            )
        ])
    }
}
