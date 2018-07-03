#[macro_use]
extern crate failure;

use failure::Error;
type Result<T> = std::result::Result<T, Error>;
pub type Nanos = u64;

pub mod event;
pub mod topology;
pub mod packet;
pub mod node;

#[cfg(test)]
mod tests {
    use super::topology;
    use super::topology::TopologyStrategy;
    use super::event::Executor;
    use super::packet::{Packet, PacketHeader};
    
    #[test]
    fn send_one_packet() {
        let t = topology::OneBigSwitch::make_topology(2, 15_000, 1_000_000, 1_000);
        let mut e = Executor::new(t);

        {
            let pkt = Packet::Data{
                hdr: PacketHeader{
                    id: 0,
                    from: 0,
                    to: 1,
                },
                seq: 0,
                length: 1460,
            };

            let topo = e.topology();
            topo.lookup_host(0).push_pkt(pkt);
        }

        e.execute();
    }
}
