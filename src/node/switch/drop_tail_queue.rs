use std::collections::VecDeque;

use node::Link;
use node::switch::Queue;
use packet::Packet;

#[derive(Debug)]
pub struct DropTailQueue{
    limit_bytes: u32,
    link: Link,
    pkts: VecDeque<Packet>,
    forced_next: Option<Packet>,
    active: bool,
    paused: bool,
}

impl DropTailQueue {
    pub fn new(limit_bytes: u32, link: Link) -> Self {
        DropTailQueue{
            limit_bytes,
            link,
            pkts: VecDeque::new(),
            forced_next: None,
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

    fn headroom(&self) -> u32 {
        self.limit_bytes - self.occupancy_bytes()
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
    
    fn force_tx_next(&mut self, p: Packet) -> Option<()> {
        self.forced_next = Some(p);
        self.set_active(true);
        Some(())
    }

    fn dequeue(&mut self) -> Option<Packet> {
        if let None = self.forced_next {
            if self.pkts.len() == 1 {
                self.set_active(false);
            }

            self.pkts.pop_front()
        } else {
            self.forced_next.take()
        }
    }

    fn discard_matching(&mut self, mut should_discard: Box<FnMut(Packet) -> bool>) -> usize {
        let pkts = &mut self.pkts;
        let after_pkts = pkts.iter().filter(|&&p| !should_discard(p)).map(|p| p.clone()).collect::<VecDeque<Packet>>();
        let dropped = pkts.len() - after_pkts.len();
        *pkts = after_pkts;
        dropped
    }

    fn count_matching(&self, mut counter: Box<FnMut(Packet) -> bool>) -> usize {
        self.pkts.iter().filter(|&&p| counter(p)).count()
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
}

#[cfg(test)]
mod tests {
    use node::{Link, switch::Queue};
    use packet::{Packet, PacketHeader};
    use super::DropTailQueue;

    #[test]
    fn check_discard_matching() {
        let mut q = DropTailQueue::new(15_000, Link{propagation_delay: 0, bandwidth_bps: 0, pfc_enabled: false, from: 0, to: 1});
        let mut pkts = (0..).map(|seq| {
            Packet::Data{
                hdr: PacketHeader{
                    flow: 0,
                    from: 0,
                    to: 1,
                },
                seq,
                length: 1460,
            }
        });

        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        q.enqueue(pkts.next().unwrap()).unwrap();
        assert_eq!(q.headroom(), 1500 * 2);

        let dropped = q.discard_matching(Box::new(|p| match p {
            Packet::Data{seq, ..} => {
                seq > 5
            }
            _ => unreachable!(),
        }));
        assert_eq!(dropped, 2);
        assert_eq!(q.headroom(), 1500 * 4);
    }
}
