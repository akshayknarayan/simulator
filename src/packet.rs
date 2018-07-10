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
    Pause,
    Resume,
}

impl Packet {
    pub fn get_size_bytes(&self) -> u32 {
        match self {
            Packet::Pause | Packet::Resume => unimplemented!(),
            Packet::Nack{..} => unimplemented!(),
            Packet::Ack{hdr, ..} => hdr.get_size_bytes(),
            Packet::Data{hdr, length, ..} => {
                length + hdr.get_size_bytes()
            }
        }
    }
}
