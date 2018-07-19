use std::collections::VecDeque;

use node::Link;
use node::switch::Queue;
use packet::Packet;

#[derive(Debug)]
pub struct DropTailQueue{
    limit_bytes: u32,
    link: Link,
    pkts: VecDeque<Packet>,
    active: bool,
    paused: bool,
}

impl DropTailQueue {
    pub fn new(limit_bytes: u32, link: Link) -> Self {
        DropTailQueue{
            limit_bytes,
            link,
            pkts: VecDeque::new(),
            active: false,
            paused: false,
        }
    }

    fn occupancy_bytes(&self) -> u32 {
        self.pkts.iter().map(|p| p.get_size_bytes()).sum()
    }
}

impl Queue for DropTailQueue {
    fn link(&self) -> Link {
        self.link
    }

    fn is_active(&self) -> bool {
        self.active && !self.paused
    }

    fn set_active(&mut self, a: bool) {
        self.active = a;
    }

    fn is_paused(&self) -> bool {
        self.paused
    }

    fn set_paused(&mut self, a: bool) {
        self.paused = a;
    }
    
    fn enqueue(&mut self, p: Packet) -> Option<()> {
        let occupancy_bytes = self.occupancy_bytes();
        if occupancy_bytes + p.get_size_bytes() > self.limit_bytes {
            // we have to drop this packet
            return None;
        }

        self.pkts.push_back(p);
        self.set_active(true);
        Some(())
    }

    fn dequeue(&mut self) -> Option<Packet> {
        if self.pkts.len() == 1 {
            self.set_active(false);
        }

        self.pkts.pop_front()
    }
    
    fn peek(&self) -> Option<&Packet> {
        self.pkts.front()
    }
}
