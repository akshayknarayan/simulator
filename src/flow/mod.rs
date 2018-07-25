use std::fmt::Debug;
use std::marker::PhantomData;
use super::{Nanos, Result};
use super::packet::Packet;
use super::event::{Event, EventTime};
use super::node::Node;
use congcontrol::CongAlg;

pub struct FlowArrivalEvent<CC: CongAlg + 'static>(pub FlowInfo, pub Nanos, pub PhantomData<CC>);

impl<CC: CongAlg> Event for FlowArrivalEvent<CC> {
    fn time(&self) -> EventTime {
        EventTime::Absolute(self.1)
    }

    fn affected_node_ids(&self) -> Vec<u32> {
        vec![self.0.sender_id, self.0.dest_id]
    }

    fn exec<'a>(&mut self, _time: Nanos, nodes: &mut [&mut Node]) -> Result<Vec<Box<Event>>> {
        let (f_send, f_recv) = go_back_n::new::<CC>(self.0);
        nodes[0].flow_arrival(f_send);
        nodes[1].flow_arrival(f_recv);
        Ok(vec![])
    }
}

#[derive(Clone,Copy,Debug)]
pub struct FlowInfo {
    pub flow_id: u32,
    pub sender_id: u32,
    pub dest_id: u32,
    pub length_bytes: u32,
    pub max_packet_length: u32,
}

pub enum FlowSide{
    Sender,
    Receiver,
}

pub trait Flow: Debug {
    fn flow_info(&self) -> FlowInfo;
    fn side(&self) -> FlowSide;

    fn completion_time(&self) -> Option<Nanos>;

    /// Process an incoming packet
    /// Return reaction outgoing packets.
    fn receive(&mut self, time: Nanos, pkt: Packet) -> Result<Vec<Packet>>;
    /// Return proactive outgoing packets.
    fn exec(&mut self, time: Nanos) -> Result<Vec<Packet>>;
}

pub mod go_back_n;
