use std::vec::Vec;
use std::collections::VecDeque;
use std::fmt::Debug;

use slog;

use super::{Nanos, Result};
use super::packet::Packet;
use super::event::{Event, EventTime};

use super::flow::Flow;

pub mod switch;

/// A Node is an entity that can receive Packets.
pub trait Node : Debug {
    fn id(&self) -> u32;
    fn receive(&mut self, p: Packet, l: Link, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>>;
    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>>;
    fn reactivate(&mut self, l: Link);
    fn flow_arrival(&mut self, f: Box<Flow>);
    fn is_active(&self) -> bool;
}

/// Links are unidirectional
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Link {
    pub propagation_delay: Nanos,
    pub bandwidth_bps: u64,
    pub pfc_enabled: bool,
    pub from: u32,
    pub to: u32,
}

impl Link {
    // The minimum amount of queue we must reserve for incoming bytes before
    // the PAUSE we send takes effect.
    // Must be conservative to prevent loss in the worst case!
    // https://www.cisco.com/c/en/us/products/collateral/switches/nexus-7000-series-switches/white_paper_c11-542809.html
    // 1. Link BDP: Packets transmitted before the PAUSE arrives at sender
    //  = propagation_delay * link_bw
    // 2. Delay in transmitting PAUSE packet (just transmitted first byte of packet before
    //    PAUSE)
    //  = 1 MTU = 1500 Bytes
    // 3. Extra transmission at sender (just transmitted first byte as PAUSE received)
    //  = 1 MTU = 1500 Bytes
    //
    // to disable PFC (allow drops): return 0
    fn pfc_pause_threshold(&self) -> u32 {
        if !self.pfc_enabled {
            0
        } else {
            let bdp = self.propagation_delay * self.bandwidth_bps / 1_000_000_000 / 8; // bytes
            bdp as u32 + 1500 + 1500
        }
    }

    // resume once there are 2 MTUs of space before the PFC threshold 
    fn pfc_resume_threshold(&self) -> u32 {
        self.pfc_pause_threshold() + 2 * 1500
    }
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
        self.to_send.push_back(p)
    }
}

impl Node for Host {
    fn id(&self) -> u32 {
        self.id
    }

    fn receive(
        &mut self, 
        p: Packet, 
        _l: Link, 
        time: Nanos, 
        logger: Option<&slog::Logger>,
    ) -> Result<Vec<Box<Event>>> {
        if let Some(log) = logger {
            debug!(log, "rx";
                "time" => time,
                "node" => self.id,
                "packet" => ?p,
            );
        }
        let active_flows = &mut self.active_flows;
        let pkts_to_send = &mut self.to_send;
        let was_empty = pkts_to_send.is_empty();
        match p.clone() {
            Packet::Data{hdr, ..} | Packet::Ack{hdr, ..} | Packet::Nack{hdr, ..} => {
                let flow_id = hdr.flow;
                if let Some(f) = active_flows.iter_mut().find(|f| f.flow_info().flow_id == flow_id) {
                    f.receive(time, p, logger).map(|(pkts, should_clear)| { 
                        if should_clear {
                            pkts_to_send.retain(|p| match p {
                                Packet::Data{hdr, ..} => hdr.flow != flow_id,
                                _ => true,
                            })
                        }

                        pkts_to_send.extend(pkts); 
                    })?;

                    if was_empty {
                        self.active = true;
                    }
                } else if let Some(log) = logger {
                    warn!(log, "got isolated packet";
                        "packet" => ?p,
                    );
                }
            }
            Packet::Pause(_) => {
                self.paused = true;
                if let Some(log) = logger {
                    debug!(log, "pausing";
                        "node" => self.id,
                    );
                }
            }
            Packet::Resume(_) => {
                self.paused = false;
                if let Some(log) = logger {
                    debug!(log, "resuming";
                        "node" => self.id,
                    );
                }
            }
        }

        Ok(vec![])
    }

    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        let flows = &mut self.active_flows;
        let active = &mut self.active;
        let link = self.link;
        let id = self.id;

        if self.paused { 
            return Ok(vec![]);
        }

        let pkts = &mut self.to_send;
        let (new_pkts, flows_to_clear): (Vec<_>, Vec<_>) = flows.iter_mut()
            .map(|f| {
                let (ps, should_clear) = f.exec(time, logger).unwrap();
                (ps, (f.flow_info().flow_id, should_clear))
            })
            .unzip();
        for fid in flows_to_clear.into_iter()
            .filter(|(_, clr)| *clr)
            .map(|(f, _)| f) {
            pkts.retain(|p| match p {
                Packet::Data{hdr, ..} => hdr.flow != fid,
                _ => true,
            })
        }

        let new_pkts = new_pkts.into_iter().flat_map(|ps| ps);
        pkts.extend(new_pkts);
        *active = false;
        pkts.pop_front().map_or_else(|| {
            Err(format_err!("no more pending outgoing packets"))
        }, |pkt| {
            if let Some(log) = logger {
                debug!(log, "tx";
                    "time" => time,
                    "node" => id,
                    "packet" => ?pkt,
                );
            }

            Ok(vec![Box::new(NodeTransmitEvent(link, pkt)) as Box<Event>])
        })
    }

    fn reactivate(&mut self, l: Link) {
        assert_eq!(self.link, l);
        self.active = true;
    }

    fn flow_arrival(&mut self, f: Box<Flow>) {
        self.active_flows.push(f);
        self.active = true;
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

#[derive(Debug)]
pub struct LinkTransmitEvent(pub Link, pub Packet);

impl Event for LinkTransmitEvent {
    fn time(&self) -> EventTime {
        EventTime::Delta(self.0.propagation_delay)
    }

    fn affected_node_ids(&self) -> Vec<u32> {
        vec![self.0.to]
    }

    fn exec(&mut self, time: Nanos, nodes: &mut [&mut Node], logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        nodes[0].receive(self.1.clone(), self.0, time, logger)
    }
}

#[derive(Debug)]
pub struct NodeTransmitEvent(pub Link, pub Packet);

impl Event for NodeTransmitEvent {
    fn time(&self) -> EventTime {
        let transmission_delay = self.1.get_size_bytes() as u64
            * 8 // to bits
            * 1_000_000_000 // to ns * bits / sec
            / self.0.bandwidth_bps; // to ns
        EventTime::Delta(transmission_delay)
    }

    fn affected_node_ids(&self) -> Vec<u32> {
        vec![self.0.from]
    }

    fn exec(&mut self, _time: Nanos, nodes: &mut [&mut Node], _logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        nodes[0].reactivate(self.0);
        Ok(vec![
            Box::new(
                LinkTransmitEvent(self.0, self.1)
            )
        ])
    }
}
