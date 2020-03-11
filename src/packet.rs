use crate::crc::CrcAccum;
use crate::rawpacket::{RawPacketParser, RawParseResult};
use crate::traits::PacketBuffer;

pub const FRAME_TYPE_MASK: u8 = 0xc0;
pub const SEQ_MASK: u8 = 0x3f;

// FrameType makes up the top 2 bits of the 8-it sequence number.
c_like_enum! {
  FrameType {
    USR = 0x00,
    RTX = 0x40,
    NAK = 0x80,
    SYN = 0xc0,
  }
}

// When the FrameType is Syn, then the following enumeration populates
// the sequence number (lower 6 bits).
c_like_enum! {
  SeqSyn {
    SYN0  = 0,
    SYN1  = 1,
    SYN2  = 2,
    DIS   = 3,
  }
}

#[derive(Debug)]
pub enum PacketType {
    USR { seq: u8 },
    RTX { seq: u8 },
    NAK { seq: u8 },
    Syn0,
    Syn1,
    Syn2,
    Disconnect,
}

#[derive(Debug)]
pub enum PacketTypeResult {
    PacketReceived(PacketType),
    AbortedPacket,
    PacketTooSmall,
    CrcError(CrcAccum),
    MoreDataNeeded,
}

pub struct PacketParser {
    raw_parser: RawPacketParser,
}

impl PacketParser {
    pub fn new() -> Self {
        Self {
            raw_parser: RawPacketParser::new(),
        }
    }

    fn get_frame_type(&self, header: u8) -> FrameType {
        if let Some(frame_type) = FrameType::from_u8(header & FRAME_TYPE_MASK) {
            frame_type
        } else {
            // We can't actually get this case since FrameType covers all of the bit patterns that
            // are contained in FRAME_TYPE_MASK.
            FrameType::NAK
        }
    }

    fn get_frame_seq(&self, header: u8) -> u8 {
        return header & SEQ_MASK;
    }

    pub fn parse_byte(&mut self, byte: u8, rx_data: &mut dyn PacketBuffer) -> PacketTypeResult {
        let parse_result = self.raw_parser.parse_byte(byte, rx_data);
        match parse_result {
            RawParseResult::RawPacketReceived(header) => {
                let frame_type = self.get_frame_type(header);
                let seq = self.get_frame_seq(header);
                match frame_type {
                    FrameType::USR => {
                        return PacketTypeResult::PacketReceived(PacketType::USR { seq });
                    }
                    FrameType::RTX => {
                        return PacketTypeResult::PacketReceived(PacketType::RTX { seq });
                    }
                    FrameType::NAK => {
                        return PacketTypeResult::PacketReceived(PacketType::NAK { seq });
                    }
                    FrameType::SYN => {
                        if let Some(seq_syn) = SeqSyn::from_u8(seq) {
                            return match seq_syn {
                                SeqSyn::SYN0 => PacketTypeResult::PacketReceived(PacketType::Syn0),
                                SeqSyn::SYN1 => PacketTypeResult::PacketReceived(PacketType::Syn1),
                                SeqSyn::SYN2 => PacketTypeResult::PacketReceived(PacketType::Syn2),
                                SeqSyn::DIS => {
                                    PacketTypeResult::PacketReceived(PacketType::Disconnect)
                                }
                            };
                        }
                        return PacketTypeResult::MoreDataNeeded;
                    }
                }
            }
            RawParseResult::AbortedPacket => PacketTypeResult::AbortedPacket,
            RawParseResult::PacketTooSmall => PacketTypeResult::PacketTooSmall,
            RawParseResult::CrcError(rcvd_crc) => PacketTypeResult::CrcError(rcvd_crc),
            RawParseResult::MoreDataNeeded => PacketTypeResult::MoreDataNeeded,
        }
    }

    pub fn reset(&mut self) {
        self.raw_parser.reset();
    }
}
