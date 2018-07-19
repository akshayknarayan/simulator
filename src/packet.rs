#[derive(Clone, Copy, Debug)]
pub struct PacketHeader {
    pub id: u32, // flow_id
    pub from: u32,
    pub to: u32,
}

impl PacketHeader{
    pub fn get_size_bytes(&self) -> u32 {
        40
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Packet {
    Data{hdr: PacketHeader, seq: u32, length: u32},
    Ack{hdr: PacketHeader, cumulative_acked_seq: u32},
    Nack{hdr: PacketHeader, nacked_seq: u32},
    Pause(u32),
    Resume(u32),
}

impl Packet {
    pub fn get_size_bytes(&self) -> u32 {
        match self {
            Packet::Pause(_) | Packet::Resume(_) => 9, // https://github.com/bobzhuyb/ns3-rdma/blob/master/src/point-to-point/model/pause-header.cc#L96
            Packet::Nack{hdr, ..} | Packet::Ack{hdr, ..} => hdr.get_size_bytes(),
            Packet::Data{hdr, length, ..} => {
                length + hdr.get_size_bytes()
            }
        }
    }
}
