use std::collections::VecDeque;
use Nanos;
use node::{Host, Link};
use node::switch::Queue;
use node::switch::lossy_switch::LossySwitch;
use node::switch::pfc_switch::PFCSwitch;
use node::switch::drop_tail_queue::DropTailQueue;

use super::{Topology, TopologyStrategy};

pub struct OneBigSwitch;
impl TopologyStrategy<LossySwitch> for OneBigSwitch {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<LossySwitch> {
        let big_switch = LossySwitch{
            id: num_hosts,
            active: false,
            rack: (0..num_hosts).map(|id| {
                Box::new(DropTailQueue::new(
                    queue_length_bytes,
                    Link{
                        propagation_delay: per_link_propagation_delay,
                        bandwidth_bps: access_link_bandwidth,
                        pfc_enabled: false,
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
                    pfc_enabled: false,
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
}

pub struct OneBigSwitchPFC;
impl TopologyStrategy<PFCSwitch> for OneBigSwitchPFC {
    fn make_topology(
        num_hosts: u32,
        queue_length_bytes: u32,
        access_link_bandwidth: u64,
        per_link_propagation_delay: Nanos,
    ) -> Topology<PFCSwitch> {
        let big_switch = PFCSwitch{
            id: num_hosts,
            active: false,
            rack: (0..num_hosts).map(|id| {(
                Box::new(DropTailQueue::new(
                    queue_length_bytes,
                    Link{
                        propagation_delay: per_link_propagation_delay,
                        bandwidth_bps: access_link_bandwidth,
                        pfc_enabled: true,
                        from: num_hosts,
                        to: id,
                    },
                )) as Box<Queue>,
                false,
            )}).collect::<Vec<(Box<Queue>, bool)>>(),
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
                    pfc_enabled: true,
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
}
