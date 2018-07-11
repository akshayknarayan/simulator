#[macro_use]
extern crate failure;

use failure::Error;
type Result<T> = std::result::Result<T, Error>;
pub type Nanos = u64;

pub mod event;
pub mod topology;
pub mod packet;
pub mod node;
pub mod flow;
pub mod congcontrol;

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;
    use super::topology;
    use super::topology::TopologyStrategy;
    use super::event::Executor;
    use super::packet::{Packet, PacketHeader};
    use super::flow::{FlowArrivalEvent, FlowInfo};
    use super::congcontrol::ConstCwnd;

    fn setup_test() -> Executor {
        let t = topology::OneBigSwitch::make_topology(2, 15_000, 1_000_000, 1_000_000);
        Executor::new(t)
    }
    
    #[test]
    fn send_one_packet() {
        let mut e = setup_test();

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
            topo.lookup_host(0).unwrap().push_pkt(pkt);
        }

        let e = e.execute();
        assert_eq!(e.current_time(), 26000000);
    }

    #[test]
    fn send_one_flow() {
        let mut e = setup_test();

        let flowinfo = FlowInfo{
            flow_id: 1,
            sender_id: 0,
            dest_id: 1,
            length_bytes: 4380, // 3 packet flow
            max_packet_length: 1460,
        };

        // starts at t = 1.0s
        let flow_arrival = Box::new(FlowArrivalEvent(flowinfo, 1_000_000_000, PhantomData::<ConstCwnd>)); 
        e.push(flow_arrival);
        let e = e.execute();
        assert_eq!(e.current_time(), 1052640000);
    }
}
