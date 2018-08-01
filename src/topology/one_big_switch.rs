use std::collections::VecDeque;
use std::marker::PhantomData;
use Nanos;
use node::{Host, Link};
use node::switch::{Switch, Queue};
use node::switch::drop_tail_queue::DropTailQueue;

use super::{Topology, TopologyStrategy};

fn switch_links(
    num_hosts: u32,
    queue_length_bytes: u32,
    access_link_bandwidth: u64,
    per_link_propagation_delay: Nanos,
    pfc_enabled: bool,
) -> impl Iterator<Item=Box<Queue + 'static>> {
    (0..num_hosts).map(move |id| {
        Box::new(DropTailQueue::new(
            queue_length_bytes,
            Link{
                propagation_delay: per_link_propagation_delay,
                bandwidth_bps: access_link_bandwidth,
                pfc_enabled,
                from: num_hosts,
                to: id,
            },
        )) as Box<Queue>
    })
}

fn hosts(
    num_hosts: u32,
    access_link_bandwidth: u64,
    per_link_propagation_delay: Nanos,
    pfc_enabled: bool,
) -> impl Iterator<Item=Host> {
    (0..num_hosts).map(move |id| {
        Host{
            id,
            active: true,
            paused: false,
            link: Link{
                propagation_delay: per_link_propagation_delay,
                bandwidth_bps: access_link_bandwidth,
                pfc_enabled,
                from: id,
                to: num_hosts,
            },
            to_send: VecDeque::new(),
            active_flows: vec![],
        }
    })
}

fn topology<S: Switch>(
    num_hosts: u32,
    access_link_bandwidth: u64,
    per_link_propagation_delay: Nanos,
    pfc_enabled: bool,
    big_switch: S,
) -> Topology<S> {
    Topology{
        hosts: hosts(
            num_hosts, 
            access_link_bandwidth,
            per_link_propagation_delay,
            pfc_enabled,
        ).collect(),
        switches: vec![big_switch],
    }
}

pub struct OneBigSwitch<S: Switch>(PhantomData<S>);
impl<S: Switch> TopologyStrategy<S> for OneBigSwitch<S> {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<S> {
        let big_switch = S::new(
            num_hosts, 
            switch_links(
                num_hosts, 
                queue_length_bytes,
                access_link_bandwidth,
                per_link_propagation_delay,
                false,
            ),
        );

        topology(
            num_hosts, 
            access_link_bandwidth,
            per_link_propagation_delay,
            false,
            big_switch,
        )
    }
}

pub struct OneBigSwitchPFC<S: Switch>(PhantomData<S>);
impl<S: Switch> TopologyStrategy<S> for OneBigSwitchPFC<S> {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<S> {
        let big_switch = S::new(
            num_hosts, 
            switch_links(
                num_hosts, 
                queue_length_bytes,
                access_link_bandwidth,
                per_link_propagation_delay,
                true,
            ),
        );

        topology(
            num_hosts, 
            access_link_bandwidth,
            per_link_propagation_delay,
            true,
            big_switch,
        )
    }
}
