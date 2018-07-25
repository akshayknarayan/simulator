use std::collections::VecDeque;
use Nanos;
use node::{Host, Link};
use node::switch::{Switch, Queue};
use node::switch::lossy_switch::LossySwitch;
use node::switch::pfc_switch::PFCSwitch;
use node::switch::nack_switch::NackSwitch;
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

pub struct OneBigSwitch;
impl TopologyStrategy<LossySwitch> for OneBigSwitch {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<LossySwitch> {
        let big_switch = LossySwitch::new(
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

pub struct OneBigSwitchPFC;
impl TopologyStrategy<PFCSwitch> for OneBigSwitchPFC {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<PFCSwitch> {
        let big_switch = PFCSwitch::new(
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

pub struct OneBigSwitchNacks;
impl TopologyStrategy<NackSwitch> for OneBigSwitchNacks {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<NackSwitch> {
        let big_switch = NackSwitch::new(
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
