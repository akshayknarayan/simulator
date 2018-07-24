use itertools::Itertools;
use itertools::EitherOrBoth::{Left, Right, Both};

use super::{Nanos, Result};
use super::node::{Node, Host};
use super::node::switch::Switch;
use super::flow::Flow;

pub trait TopologyStrategy<S: Switch> {
    fn make_topology(
        num_hosts: u32, 
        queue_length_bytes: u32,
        access_link_bandwidth: u64, 
        per_link_propagation_delay: Nanos,
    ) -> Topology<S>;
}

pub mod one_big_switch;

#[derive(Debug)]
pub struct Topology<S: Switch> {
    pub hosts: Vec<Host>,
    pub switches: Vec<S>,
}

impl<S: Switch> Topology<S> {
    pub fn active_nodes(&mut self) -> impl Iterator<Item=&mut Node> {
        self.hosts.iter_mut()
            .map(|h| h as &mut Node)
            .chain(self.switches.iter_mut().map(|s| s as &mut Node))
            .filter(|h| h.is_active())
    }

    pub fn all_flows(&self) -> impl Iterator<Item=&Box<Flow>> {
        self.hosts.iter()
            .flat_map(|h| h.active_flows.iter())
    }

    pub fn lookup_host(&mut self, id: u32) -> Result<&mut Host> {
        if (id as usize) < self.hosts.len() {
            Ok(&mut self.hosts[id as usize])
        } else {
            bail!("Invalid host id: {:?}", id)
        }
    }

    pub fn lookup_node<'a>(&'a mut self, id: u32) -> Result<&'a mut Node> {
        if (id as usize) < self.hosts.len() {
            Ok(self.lookup_host(id)?)
        } else if ((id as usize) - self.hosts.len()) < self.switches.len() {
            Ok(&mut self.switches[(id as usize) - self.hosts.len()])
        } else {
            bail!("Invalid node id: {:?}", id)
        }
    }

    pub fn lookup_nodes<'a>(&'a mut self, ids: &[u32]) -> Result<Vec<&'a mut Node>> {
        let hosts = &mut self.hosts;
        let switches = &mut self.switches;

        let (mut host_ids, mut sw_ids): (Vec<(usize, &u32)>, Vec<(usize, &u32)>) = ids
            .into_iter()
            .enumerate()
            .partition(|(_, &id)| (id as usize) < hosts.len());

        host_ids.sort_by_key(|(_, &id)| id);
        sw_ids.sort_by_key(|(_, &id)| id);

        let hs = merge_by_indices(
            hosts.iter_mut()
                .map(|h| h as &mut Node), 
            host_ids.into_iter()
                .map(|(a, &id)| (a, id)),
        );
        let sw = merge_by_indices(
            switches.iter_mut()
                .map(|s| s as &mut Node), 
            sw_ids.into_iter()
                .map(|(a, &id)| (a, id)),
        );

        Ok(hs.chain(sw)
            .collect::<Result<Vec<(usize, &mut Node)>>>()?
            .into_iter()
            .sorted_by_key(|x| x.0)
            .into_iter()
            .map(|x| x.1)
            .collect::<Vec<&mut Node>>())
    }
}

fn merge_by_indices<'a>(
    nodes: impl Iterator<Item=&'a mut Node>, 
    indices: impl Iterator<Item=(usize, u32)>,
) -> impl Iterator<Item=Result<(usize, &'a mut (Node + 'a))>> {
    nodes
        .merge_join_by(indices, |n, (_, wanted_id)| {
            n.id().cmp(&wanted_id)
        })
        .filter_map(|either| {
            match either {
                Left(_) => None,
                Right(x) => Some(Err(format_err!("Node {:?} not found", x))),
                Both(h, (wanted_idx, _)) => Some(Ok((wanted_idx, h as &mut Node))),
            }
        })
}

#[cfg(test)]
mod tests {
    use super::TopologyStrategy;
    use super::one_big_switch::OneBigSwitch;
    #[test]
    fn make() {
        let _t = OneBigSwitch::make_topology(2, 15_000, 1_000_000, 1_000);
    }

    #[test]
    fn lookup_node() {
        let mut t = OneBigSwitch::make_topology(5, 15_000, 1_000_000, 1_000);
        let nodes = t.lookup_nodes(&[2,4,5]).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].id(), 2);
        assert_eq!(nodes[1].id(), 4);
        assert_eq!(nodes[2].id(), 5);
    }

    #[test]
    fn lookup_nonexistent_node() {
        let mut t = OneBigSwitch::make_topology(5, 15_000, 1_000_000, 1_000);
        t.lookup_nodes(&[1, 3, 6]).unwrap_err();
    }
}
