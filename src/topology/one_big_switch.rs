use std::collections::VecDeque;
use Nanos;
use node::{Host, Link};
use node::switch::{Switch, Queue};
use node::switch::drop_tail_queue::DropTailQueue;

use super::{Topology, TopologyStrategy};

fn one_big_switch(
    num_hosts: u32,
    queue_length_bytes: u32,
    access_link_bandwidth: u64,
    per_link_propagation_delay: Nanos,
    pfc_enabled: bool,
) -> Topology {
    let big_switch = Switch{
        id: num_hosts,
        active: false,
        rack: (0..num_hosts).map(|id| {
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
        }).collect::<Vec<Box<Queue>>>(),
        core: vec![],
    };

    let hosts = (0..num_hosts).map(|id| {
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
    }).collect::<Vec<Host>>();

    Topology{
        hosts,
        switches: vec![big_switch],
    }
}

pub struct OneBigSwitch;
impl TopologyStrategy for OneBigSwitch {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology {
        one_big_switch(
            num_hosts,
            queue_length_bytes,
            access_link_bandwidth,
            per_link_propagation_delay,
            false,
        )
    }
}

pub struct OneBigSwitchPFC;
impl TopologyStrategy for OneBigSwitchPFC {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology {
        one_big_switch(
            num_hosts,
            queue_length_bytes,
            access_link_bandwidth,
            per_link_propagation_delay,
            true,
        )
    }
}
