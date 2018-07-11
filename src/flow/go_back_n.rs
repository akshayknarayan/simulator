use ::Result;
use ::congcontrol::{CongAlg, ReductionType};
use super::{Flow, FlowInfo};
use ::packet::{Packet, PacketHeader};

pub fn new<CC: CongAlg>(fi: FlowInfo) -> (Box<GoBackNSender<CC>>, Box<GoBackNReceiver>) {
    (
        Box::new(GoBackNSender {
            flow_info: fi,
            next_to_send: 0,
            cumulative_acked: 0,
            cong_control: CC::new(),
        }),
        Box::new(GoBackNReceiver {
            flow_info: fi,
            cumulative_received: 0,
        }),
    )
}

#[derive(Clone, Debug)]
pub struct GoBackNSender<CC: CongAlg> {
    flow_info: FlowInfo,

    // sending side
    next_to_send: u32,
    cumulative_acked: u32,
    cong_control: CC,
}

#[derive(Clone, Debug)]
pub struct GoBackNReceiver {
    flow_info: FlowInfo,
    cumulative_received: u32,
}

impl<CC: CongAlg> Flow for GoBackNSender<CC> {
    fn flow_info(&self) -> FlowInfo { self.flow_info }

    fn receive(&mut self, pkt: Packet) -> Result<Vec<Packet>> {
        match pkt {
            Packet::Data{..} => unreachable!(),
            Packet::Ack{..} | Packet::Nack{..} => self.got_ack(pkt),
            Packet::Pause | Packet::Resume => unreachable!(),
        }
    }
    
    fn exec(&mut self) -> Result<Vec<Packet>> {
        self.maybe_send_more()
    }
}

impl<CC: CongAlg> GoBackNSender<CC> {
    // sending side
    fn got_ack(&mut self, ack: Packet) -> Result<Vec<Packet>> {
        match ack {
            Packet::Ack{hdr, cumulative_acked_seq} => {
                assert_eq!(hdr.id, self.flow_info.flow_id);
                assert_eq!(hdr.from, self.flow_info.dest_id);
                assert_eq!(hdr.to, self.flow_info.sender_id);

                // 2 cases
                // in order ACK, all well
                // out of order ACK, must go back N
                if cumulative_acked_seq > self.cumulative_acked {
                    self.cong_control.on_packet(cumulative_acked_seq - self.cumulative_acked, 0 /* rtt, Nanos */);
                    self.cumulative_acked = cumulative_acked_seq;
                    if self.cumulative_acked == self.flow_info.length_bytes {
                        println!("flow {:?} done", self.flow_info.flow_id);
                        Ok(vec![])
                    } else {
                        self.maybe_send_more()
                    }
                } else {
                    self.cong_control.reduction(ReductionType::Drop);
                    let cumulative_acked = self.cumulative_acked;
                    self.go_back_n(cumulative_acked)
                }
            }
            Packet::Nack{hdr, nacked_seq} => {
                assert_eq!(hdr.id, self.flow_info.flow_id);
                assert_eq!(hdr.from, self.flow_info.dest_id);
                assert_eq!(hdr.to, self.flow_info.sender_id);
                self.cong_control.reduction(ReductionType::Drop);
                self.go_back_n(nacked_seq)
            }
            Packet::Data{..} | Packet::Pause | Packet::Resume => unreachable!(),
        }
    }

    fn maybe_send_more(&mut self) -> Result<Vec<Packet>> {
        let cwnd = self.cong_control.cwnd() * self.flow_info.max_packet_length;
        let mut pkts = vec![];
        loop {
            if self.next_to_send < self.cumulative_acked + cwnd {
                if self.next_to_send + self.flow_info.max_packet_length <= self.flow_info.length_bytes {
                    // send a full size packet and continue
                    let pkt = Packet::Data{
                        hdr: PacketHeader{
                            id: self.flow_info.flow_id,
                            from: self.flow_info.sender_id,
                            to: self.flow_info.dest_id,
                        },
                        seq: self.next_to_send,
                        length: self.flow_info.max_packet_length,
                    };

                    self.next_to_send += self.flow_info.max_packet_length;
                    pkts.push(pkt);
                } else if self.next_to_send < self.flow_info.length_bytes {
                    let pkt = Packet::Data{
                        hdr: PacketHeader{
                            id: self.flow_info.flow_id,
                            from: self.flow_info.sender_id,
                            to: self.flow_info.dest_id,
                        },
                        seq: self.next_to_send,
                        length: self.flow_info.length_bytes - self.next_to_send,
                    };

                    self.next_to_send += self.flow_info.length_bytes - self.next_to_send;
                    pkts.push(pkt);
                    break;
                } else {
                    break;
                }
            } else {
                break
            }
        }

        println!("sending {:?}", pkts);

        Ok(pkts)
    }

    fn go_back_n(&mut self, go_back_to: u32) -> Result<Vec<Packet>> {
        self.next_to_send = go_back_to;
        self.maybe_send_more()
    }
}

impl Flow for GoBackNReceiver {
    fn flow_info(&self) -> FlowInfo { self.flow_info }

    fn receive(&mut self, pkt: Packet) -> Result<Vec<Packet>> {
        match pkt {
            Packet::Data{..} => self.got_data(pkt),
            Packet::Ack{..} | Packet::Nack{..} => unreachable!(),
            Packet::Pause | Packet::Resume => unreachable!(),
        }
    }
    
    fn exec(&mut self) -> Result<Vec<Packet>> {
        Ok(vec![])
    }
}

impl GoBackNReceiver {
    // ack-ing side
    fn got_data(&mut self, data: Packet) -> Result<Vec<Packet>> {
        println!("GBN receiver {:?} got {:?}, expect {:?}", self.flow_info.flow_id, data, self.cumulative_received);
        match data {
            Packet::Data{hdr, seq, length} => {
                assert_eq!(hdr.id, self.flow_info.flow_id);
                assert_eq!(hdr.to, self.flow_info.dest_id);
                assert_eq!(hdr.from, self.flow_info.sender_id);
                if seq == self.cumulative_received {
                    self.cumulative_received += length;
                    // send ACK
                    Ok(vec![Packet::Ack{
                        hdr: PacketHeader{
                            id: hdr.id,
                            from: hdr.to,
                            to: hdr.from,
                        },
                        cumulative_acked_seq: self.cumulative_received,
                    }])
                } else {
                    // out of order packet
                    // send NACK
                    Ok(vec![Packet::Nack{
                        hdr: PacketHeader{
                            id: hdr.id,
                            from: hdr.to,
                            to: hdr.from,
                        },
                        nacked_seq: self.cumulative_received,
                    }])
                }
            }
            Packet::Ack{..} | Packet::Nack{..} | Packet::Pause | Packet::Resume => unreachable!(),
        }
    }
}
