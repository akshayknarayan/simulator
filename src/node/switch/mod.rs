use std::vec::Vec;
use std::fmt::Debug;

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
    fn peek(&self) -> Option<&Packet>;
    fn headroom(&self) -> u32;
    fn is_active(&self) -> bool;
    fn set_active(&mut self, a: bool);
    fn is_paused(&self) -> bool;
    fn set_paused(&mut self, a: bool);
}

pub mod drop_tail_queue;

pub trait Switch: Debug {
    fn id(&self) -> u32;
    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>>;
    fn exec(&mut self, time: Nanos) -> Result<Vec<Box<Event>>>;
    fn reactivate(&mut self, l: Link);
    fn is_active(&self) -> bool;
}

impl<S: Switch> Node for S {
    fn id(&self) -> u32 {
        self.id()
    }

    fn receive(&mut self, p: Packet, time: Nanos) -> Result<Vec<Box<Event>>> {
        self.receive(p, time)
    }

    fn exec(&mut self, time: Nanos) -> Result<Vec<Box<Event>>> {
        self.exec(time)
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
