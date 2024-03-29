use std::vec::Vec;
use std::fmt::Debug;

use slog;

use ::{Nanos, Result};
use event::Event;
use node::{Node, Link};
use packet::Packet;
use flow::Flow;
   
/// Queues are tied to a specfic link.
pub trait Queue : Debug {
    fn link(&self) -> Link;
    fn enqueue(&mut self, p: Packet) -> Option<()>;
    fn force_tx_next(&mut self, p: Packet) -> Option<()>;
    fn dequeue(&mut self) -> Option<Packet>;
    fn discard_matching(&mut self, Box<FnMut(Packet) -> bool>) -> usize;
    fn count_matching(&self, Box<FnMut(Packet) -> bool>) -> usize;
    fn headroom(&self) -> u32;
    fn is_active(&self) -> bool;
    fn set_active(&mut self, a: bool);
    fn is_paused(&self) -> bool;
    fn set_paused(&mut self, a: bool);
}

pub mod drop_tail_queue;

pub trait Switch: Debug {
    fn new(
        switch_id: u32, 
        links: impl Iterator<Item=Box<Queue>>,
    ) -> Self;
    fn id(&self) -> u32;
    fn receive(
        &mut self, 
        p: Packet, 
        l: Link, 
        time: Nanos, 
        logger: Option<&slog::Logger>,
    ) -> Result<Vec<Box<Event>>>;
    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>>;
    fn reactivate(&mut self, l: Link);
    fn is_active(&self) -> bool;
}

/// Marker trait that indicates to `TopologyStrategy` instances that the links
/// should have `pfc_enabled` set to `true` (`false` by default).
pub trait PFCSwitchFamily: Switch {}

impl<S: Switch> Node for S {
    fn id(&self) -> u32 {
        self.id()
    }

    fn receive(
        &mut self, 
        p: Packet, 
        l: Link, 
        time: Nanos, 
        logger: Option<&slog::Logger>,
    ) -> Result<Vec<Box<Event>>> {
        self.receive(p, l, time, logger)
    }

    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        self.exec(time, logger)
    }

    fn reactivate(&mut self, l: Link) {
        self.reactivate(l)
    }

    fn flow_arrival(&mut self, _: Box<Flow>) {
        unreachable!()
    }

    fn is_active(&self) -> bool {
        self.is_active()
    }
}

pub mod pfc_switch;
pub mod lossy_switch;
pub mod nack_switch;
