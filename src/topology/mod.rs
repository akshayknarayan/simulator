use super::{Nanos, Result};
use super::node::{Node, Host};
use super::node::switch::Switch;

pub trait TopologyStrategy {
    fn make_topology(
        num_hosts: u32, 
        queue_length_bytes: u32,
        access_link_bandwidth: u64, 
        per_link_propagation_delay: Nanos,
    ) -> Topology;
}

pub mod one_big_switch;

pub enum TopologyNode<'a> {
    Host(&'a mut Host),
    Switch(&'a mut Switch),
}

#[derive(Debug)]
pub struct Topology {
    pub hosts: Vec<Host>,
    pub switches: Vec<Switch>,
}

impl Topology {
    pub fn active_nodes(&mut self) -> impl Iterator<Item=&mut Node> {
        self.hosts.iter_mut()
            .map(|h| h as &mut Node)
            .chain(self.switches.iter_mut().map(|s| s as &mut Node))
            .filter(|h| h.is_active())
    }

    pub fn lookup_host(&mut self, id: u32) -> Result<&mut Host> {
        if (id as usize) < self.hosts.len() {
            Ok(&mut self.hosts[id as usize])
        } else {
            bail!("Invalid host id: {:?}", id)
        }
    }

    pub fn lookup_node<'a>(&'a mut self, id: u32) -> Result<TopologyNode> {
        if (id as usize) < self.hosts.len() {
            self.lookup_host(id).map(|h| TopologyNode::Host(h))
        } else if ((id as usize) - self.hosts.len()) < self.switches.len() {
            Ok(TopologyNode::Switch(&mut self.switches[(id as usize) - self.hosts.len()]))
        } else {
            bail!("Invalid node id: {:?}", id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TopologyStrategy;
    use super::one_big_switch::OneBigSwitch;
    #[test]
    fn make() {
        let _t = OneBigSwitch::make_topology(2, 15_000, 1_000_000, 1_000);
    }
}
