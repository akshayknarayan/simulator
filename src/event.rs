use std::cmp::Ordering;
use std::boxed::Box;
use std::collections::BinaryHeap;

use slog;

use super::{Nanos, Result};
use super::topology::Topology;
use super::node::Node;

/// Event driven simulator runtime model:
/// 1. A single event covers all the computation performed by a single node in a single step of
///    time.
/// 2. Calling exec() on an event can yield zero or more successive events.
/// 3. All events are ordered by time (`impl Ord`) and executed in this order.

#[derive(PartialEq, Eq)]
pub enum EventTime {
    Absolute(Nanos),
    Delta(Nanos),
}

pub trait Event {
    fn time(&self) -> EventTime; // when this should trigger
    fn affected_node_ids(&self) -> Vec<u32>;
    fn exec(&mut self, time: Nanos, affected_nodes: &mut [&mut Node], logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>>; // execute the event
}

struct EventContainer(Box<Event>, Nanos);

impl EventContainer {
    fn abs_time(&self, now: Nanos) -> Nanos {
        match self.0.time() {
            EventTime::Absolute(t) => t,
            EventTime::Delta(t) => now + t,
        }
    }
}

impl PartialEq for EventContainer {
    fn eq(&self, other: &EventContainer) -> bool {
        self.1 == other.1
    }
}

impl Eq for EventContainer{}

impl PartialOrd for EventContainer {
    fn partial_cmp(&self, other: &EventContainer) -> Option<Ordering> {
        other.1.partial_cmp(&self.1)
    }
}

impl Ord for EventContainer {
    fn cmp(&self, other: &EventContainer) -> Ordering {
        other.1.cmp(&self.1)
    }
}

use node::switch::Switch;

pub struct Executor<S: Switch> {
    events: BinaryHeap<EventContainer>,
    current_time: Nanos,
    topology: Topology<S>,
    logger: Option<slog::Logger>,
}

impl<S: Switch> Executor<S> {
    pub fn new(topology: Topology<S>, logger: impl Into<Option<slog::Logger>>) -> Self {
        Executor{
            events: BinaryHeap::new(),
            current_time: 0,
            topology,
            logger: logger.into(),
        }
    }

    pub fn components(&mut self) -> (Nanos, &mut Topology<S>, Option<&slog::Logger>) {
        (self.current_time, &mut self.topology, self.logger.as_ref())
    }

    pub fn push(&mut self, ev: Box<Event>) {
        push_onto(self.current_time, ev, &mut self.events)
    }

    fn poll_nodes(&mut self) {
        // advancing time
        // first, poll all active nodes
        let events_heap = &mut self.events;
        let logger = self.logger.as_ref();
        let now = self.current_time;
        let top = &mut self.topology;
        top
            .active_nodes()
            .filter_map(|n| n.exec(now, logger).ok())
            .flat_map(|i| i)
            .for_each(|new_ev| push_onto(now, new_ev, events_heap))
    }

    pub fn execute(mut self) -> Result<Self> {
        loop {
            match self.events.pop() {
                Some(evc) => {
                    assert!(
                        evc.1 >= self.current_time, 
                        "event time {:?} before current time {:?}", evc.1, self.current_time,
                    );

                    let evc = if evc.1 > self.current_time {
                        self.events.push(evc);
                        self.poll_nodes();
                        self.events.pop().unwrap() // guaranteed since we just pushed it back on
                    } else {
                        evc
                    };

                    self.current_time = evc.1;

                    let mut ev = evc.0;
                    let new_evs = {
                        let nds = &mut self.topology.lookup_nodes(&ev.affected_node_ids())?;
                        ev.exec(self.current_time, nds, self.logger.as_ref())?
                    };
                    for new_ev in new_evs {
                        self.push(new_ev);
                    }
                }
                None => {
                    self.poll_nodes(); // try to poll nodes one last time
                    if self.events.is_empty() {
                        if let Some(ref log) = self.logger {
                            info!(log, "exiting";
                                "time" => self.current_time,
                            );
                        }

                        return Ok(self);
                    }
                }
            }
        }
    }

    pub fn current_time(&self) -> Nanos {
        self.current_time
    }
}

fn push_onto(now: Nanos, ev: Box<Event>, heap: &mut BinaryHeap<EventContainer>) {
    let mut evc = EventContainer(ev, 0);
    evc.1 = evc.abs_time(now);
    heap.push(evc);
}
