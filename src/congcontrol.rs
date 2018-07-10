use std::fmt::Debug;
use super::Nanos;

pub enum ReductionType {
    Drop,
    Ecn,
}

pub trait CongAlg: Clone + Debug {
    fn new() -> Self;
    fn cwnd(&self) -> u32;
    fn on_packet(&mut self, acked: u32, rtt: Nanos) -> u32;
    fn reduction(&mut self, reduction: ReductionType) -> u32;
}

#[derive(Clone, Debug)]
pub struct ConstCwnd(u32);

impl CongAlg for ConstCwnd {
    fn new() -> Self {
        ConstCwnd(10)
    }

    fn cwnd(&self) -> u32 { self.0 }

    fn on_packet(&mut self, _: u32, _: Nanos) -> u32 {
        self.0
    }

    fn reduction(&mut self, _: ReductionType) -> u32 {
        self.0
    }
}
