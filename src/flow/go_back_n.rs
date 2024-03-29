use slog;

use ::{Nanos, Result};
use ::congcontrol::{CongAlg, ReductionType};
use super::{Flow, FlowInfo, FlowSide};
use ::packet::{Packet, PacketHeader};

pub fn new<CC: CongAlg>(fi: FlowInfo) -> (Box<GoBackNSender<CC>>, Box<GoBackNReceiver>) {
    (
        Box::new(GoBackNSender {
            flow_info: fi,
            start_time: None,
            completion_time: None,
            next_to_send: 0,
            cumulative_acked: 0,
            retx_timeout: 0,
            cong_control: CC::new(),
        }),
        Box::new(GoBackNReceiver {
            flow_info: fi,
            cumulative_received: 0,
            start_time: None,
            completion_time: None,
            nack_inflight: false,
        }),
    )
}

#[derive(Clone, Debug)]
pub struct GoBackNSender<CC: CongAlg> {
    flow_info: FlowInfo,

    start_time: Option<Nanos>,
    completion_time: Option<Nanos>,
    next_to_send: u32,
    cumulative_acked: u32,
    retx_timeout: Nanos,
    cong_control: CC,
}

#[derive(Clone, Debug)]
pub struct GoBackNReceiver {
    flow_info: FlowInfo,
    cumulative_received: u32,
    start_time: Option<Nanos>,
    completion_time: Option<Nanos>,
    nack_inflight: bool,
}

impl<CC: CongAlg> Flow for GoBackNSender<CC> {
    fn flow_info(&self) -> FlowInfo { self.flow_info }
    fn side(&self) -> FlowSide { FlowSide::Sender }

    fn completion_time(&self) -> Option<Nanos> {
        self.completion_time
    }

    fn receive(&mut self, time: Nanos, pkt: Packet, logger: Option<&slog::Logger>) -> Result<(Vec<Packet>, bool)> {
        match pkt {
            Packet::Data{..} => unreachable!(),
            Packet::Ack{..} | Packet::Nack{..} => {
                self.retx_timeout = time;
                self.got_ack(pkt, time, logger)
            }
            _ => unreachable!(),
        }
    }
    
    fn exec(&mut self, time: Nanos, _logger: Option<&slog::Logger>) -> Result<(Vec<Packet>, bool)> {
        if let None = self.start_time {
            self.start_time = Some(time);
        }

        if self.completion_time.is_some() {
            Ok((vec![], false))
        } else if !self.check_timeout(time) {
            self.maybe_send_more().map(|v| (v, false))
        } else {
            let cum_ack = self.cumulative_acked;
            self.retx_timeout = time;
            self.go_back_n(cum_ack).map(|v| (v, true))
        }
    }
}

impl<CC: CongAlg> GoBackNSender<CC> {
    // sending side
    fn got_ack(&mut self, ack: Packet, time: Nanos, logger: Option<&slog::Logger>) -> Result<(Vec<Packet>, bool)> {
        match ack {
            Packet::Ack{hdr, cumulative_acked_seq} => {
                assert_eq!(hdr.flow, self.flow_info.flow_id);
                assert_eq!(hdr.from, self.flow_info.dest_id);
                assert_eq!(hdr.to, self.flow_info.sender_id);

                // 2 cases
                // in order ACK, all well
                // out of order ACK, must go back N
                if cumulative_acked_seq > self.cumulative_acked {
                    self.cong_control.on_packet(cumulative_acked_seq - self.cumulative_acked, 0 /* rtt, Nanos */);
                    self.cumulative_acked = cumulative_acked_seq;
                    if self.cumulative_acked == self.flow_info.length_bytes {
                        self.completion_time = Some(time - self.start_time.unwrap());
                        if let Some(log) = logger {
                            info!(log, "flow completed";
                                "flow" => self.flow_info.flow_id,
                                "node" => self.flow_info.sender_id,
                                "side" => ?self.side(),
                                "completion_time" => self.completion_time.unwrap(),
                                "start_time" => self.start_time.unwrap(),
                                "end_time" => time,
                            );
                        }

                        Ok((vec![], false))
                    } else {
                        self.maybe_send_more().map(|v| (v, false))
                    }
                } else {
                    // old ACK, ignore
                    Ok((vec![], false))
                }
            }
            Packet::Nack{hdr, nacked_seq} => {
                assert_eq!(hdr.flow, self.flow_info.flow_id);
                assert_eq!(hdr.from, self.flow_info.dest_id);
                assert_eq!(hdr.to, self.flow_info.sender_id);
                self.cong_control.reduction(ReductionType::Drop);
                self.go_back_n(nacked_seq).map(|v| (v, true))
            }
            _ => unreachable!(),
        }
    }

    fn check_timeout(&mut self, now: Nanos) -> bool {
        self.retx_timeout > 0 
            && self.completion_time.is_none() 
            && (now - self.retx_timeout) > 1_000_000_000 // 1s // TODO configurable
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
                            flow: self.flow_info.flow_id,
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
                            flow: self.flow_info.flow_id,
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

        Ok(pkts)
    }

    fn go_back_n(&mut self, go_back_to: u32) -> Result<Vec<Packet>> {
        self.next_to_send = go_back_to;
        self.maybe_send_more()
    }
}

impl Flow for GoBackNReceiver {
    fn flow_info(&self) -> FlowInfo { self.flow_info }
    fn side(&self) -> FlowSide { FlowSide::Receiver }

    fn completion_time(&self) -> Option<Nanos> {
        self.completion_time
    }

    fn receive(&mut self, time: Nanos, pkt: Packet, logger: Option<&slog::Logger>) -> Result<(Vec<Packet>, bool)> {
        match pkt {
            Packet::Data{..} => self.got_data(pkt, time, logger).map(|v| (v, false)),
            _ => unreachable!(),
        }
    }
    
    fn exec(&mut self, _time: Nanos, _logger: Option<&slog::Logger>) -> Result<(Vec<Packet>, bool)> {
        Ok((vec![], false))
    }
}

impl GoBackNReceiver {
    // ack-ing side
    fn got_data(&mut self, data: Packet, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Packet>> {
        if let None = self.start_time {
            self.start_time = Some(time);
        }

        match data {
            Packet::Data{hdr, seq, length} => {
                assert_eq!(hdr.flow, self.flow_info.flow_id);
                assert_eq!(hdr.to, self.flow_info.dest_id);
                assert_eq!(hdr.from, self.flow_info.sender_id);
                if seq == self.cumulative_received {
                    self.cumulative_received += length;
                    self.nack_inflight = false;
                    if self.cumulative_received == self.flow_info.length_bytes {
                        self.completion_time = Some(time - self.start_time.unwrap());
                        if let Some(log) = logger {
                            info!(log, "flow completed";
                                "flow" => self.flow_info.flow_id,
                                "node" => self.flow_info.dest_id,
                                "side" => ?self.side(),
                                "completion_time" => self.completion_time.unwrap(),
                                "start_time" => self.start_time.unwrap(),
                                "end_time" => time,
                            );
                        }
                    }

                    // send ACK
                    Ok(vec![Packet::Ack{
                        hdr: PacketHeader{
                            flow: hdr.flow,
                            from: hdr.to,
                            to: hdr.from,
                        },
                        cumulative_acked_seq: self.cumulative_received,
                    }])
                } else {
                    // out of order packet
                    // send NACK
                    if !self.nack_inflight {
                        self.nack_inflight = true;
                        Ok(vec![Packet::Nack{
                            hdr: PacketHeader{
                                flow: hdr.flow,
                                from: hdr.to,
                                to: hdr.from,
                            },
                            nacked_seq: self.cumulative_received,
                        }])
                    } else {
                        Ok(vec![])
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}
