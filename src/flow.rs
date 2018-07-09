use super::Result;
use super::packet::{Packet, PacketHeader};

use congcontrol::CongAlg;
use congcontrol::ReductionType;

pub trait Flow {
    fn flow_id(&self) -> u32;
    fn sender_id(&self) -> u32;
    fn dest_id(&self) -> u32;
    /// Process an incoming packet
    /// Return reaction outgoing packets.
    fn receive(&mut self, pkt: Packet) -> Result<Vec<Packet>>;
    /// Return proactive outgoing packets.
    fn exec(&mut self) -> Result<Vec<Packet>>;
}

pub struct GoBackN<CC: CongAlg> {
    flow_id: u32,
    sender_id: u32,
    dest_id: u32,
    length_bytes: u32,
    max_packet_length: u32,

    // sending side
    next_to_send: u32,
    cumulative_acked: u32,
    cong_control: CC,

    // ack-ing side
    cumulative_received: u32,
}

impl<CC: CongAlg> Flow for GoBackN<CC> {
    fn flow_id(&self) -> u32 { self.flow_id }
    fn sender_id(&self) -> u32 { self.sender_id }
    fn dest_id(&self) -> u32 { self.dest_id }

    fn receive(&mut self, pkt: Packet) -> Result<Vec<Packet>> {
        match pkt {
            Packet::Data{..} => self.got_data(pkt),
            Packet::Ack{..} | Packet::Nack{..} => self.got_ack(pkt),
            Packet::Pause | Packet::Resume => unreachable!(),
        }
    }
    
    fn exec(&mut self) -> Result<Vec<Packet>> {
        unimplemented!()
    }
}

impl<CC: CongAlg> GoBackN<CC> {
    pub fn new(flow_id: u32, sender_id: u32, dest_id: u32, length_bytes: u32) -> Self {
        GoBackN {
            flow_id,
            sender_id,
            dest_id,
            length_bytes,
            max_packet_length: 1460,
            next_to_send: 0,
            cumulative_acked: 0,
            cong_control: CC::new(),
            cumulative_received: 0,
        }
    }

    // ack-ing side
    fn got_data(&mut self, data: Packet) -> Result<Vec<Packet>> {
        match data {
            Packet::Data{hdr, seq, length} => {
                assert_eq!(hdr.id, self.flow_id);
                assert_eq!(hdr.to, self.dest_id);
                assert_eq!(hdr.from, self.sender_id);
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

    // sending side
    fn got_ack(&mut self, ack: Packet) -> Result<Vec<Packet>> {
        match ack {
            Packet::Ack{hdr, cumulative_acked_seq} => {
                assert_eq!(hdr.id, self.flow_id);
                assert_eq!(hdr.from, self.dest_id);
                assert_eq!(hdr.to, self.sender_id);

                // 2 cases
                // in order ACK, all well
                // out of order ACK, must go back N
                if cumulative_acked_seq > self.cumulative_acked {
                    self.cong_control.on_packet(self.cumulative_acked - cumulative_acked_seq, 0 /* rtt, Nanos */);
                    self.cumulative_acked = cumulative_acked_seq;
                    self.maybe_send_more()
                } else {
                    self.cong_control.reduction(ReductionType::Drop);
                    let cumulative_acked = self.cumulative_acked;
                    self.go_back_n(cumulative_acked)
                }
            }
            Packet::Nack{hdr, nacked_seq} => {
                assert_eq!(hdr.id, self.flow_id);
                assert_eq!(hdr.from, self.dest_id);
                assert_eq!(hdr.to, self.sender_id);
                self.cong_control.reduction(ReductionType::Drop);
                self.go_back_n(nacked_seq)
            }
            Packet::Data{..} | Packet::Pause | Packet::Resume => unreachable!(),
        }
    }

    fn maybe_send_more(&mut self) -> Result<Vec<Packet>> {
        let cwnd = self.cong_control.cwnd();
        let mut pkts = vec![];
        loop {
            if self.next_to_send < self.cumulative_acked + cwnd {
                if self.next_to_send + self.max_packet_length < self.length_bytes {
                    // send a full size packet and continue
                    let pkt = Packet::Data{
                        hdr: PacketHeader{
                            id: self.flow_id,
                            from: self.sender_id,
                            to: self.dest_id,
                        },
                        seq: self.next_to_send,
                        length: self.max_packet_length,
                    };

                    self.next_to_send += self.max_packet_length;
                    pkts.push(pkt);
                } else {
                    let pkt = Packet::Data{
                        hdr: PacketHeader{
                            id: self.flow_id,
                            from: self.sender_id,
                            to: self.dest_id,
                        },
                        seq: self.next_to_send,
                        length: self.length_bytes - self.next_to_send,
                    };

                    self.next_to_send += self.length_bytes - self.next_to_send;
                    pkts.push(pkt);
                    break;
                }
            } else {
                break
            }
        }

        Ok(pkts)
    }

    fn go_back_n(&mut self, go_back_to: u32) -> Result<Vec<Packet>> {
        self.next_to_send = go_back_to;
        self.maybe_send_more()
    }
}
