use generic_array::GenericArray;

use crate::crc::{Crc, CrcAccum};
use crate::driver;

use core::result::Result;

const SOF: u8 = 0x7e;   // Start of Frame
const ESC: u8 = 0x7d;
const ESC_FLIP: u8 = 1 << 5;

#[derive(PartialEq, Clone)]
enum EscapeState {
  Normal,
  Escaping,
}

#[derive(PartialEq, Clone)]
enum FrameState {
  New,
  Collecting,
}

pub struct Packet<Driver: driver::Driver>
{
  len: usize,
  data: GenericArray<u8, Driver::PACKET_SIZE>,
}

impl<Driver: driver::Driver> Packet<Driver> {
  pub fn new() -> Packet<Driver> {
    Packet {
      len: 0,
      data: Default::default(),
    }
  }

  pub fn data(&self) -> &[u8] {
    &self.data[0..self.len]
  }

  pub fn append(&mut self, byte: u8) -> Result<(), ()> {
    if self.len < self.data.len() {
      self.data[self.len] = byte;
      self.len += 1;
      Ok(())
    } else {
      Err(())
    }
  }

  pub fn len(&self) -> usize {
    return self.len;
  }

  pub fn reset(&mut self) {
    self.len = 0;
  }

  pub fn remove_crc(&mut self) -> CrcAccum {
    if self.len < 2 {
      return 0;
    }

    // LSB is transmitted first
    self.len -= 2;
    return ((self.data[self.len + 1] as CrcAccum) << 8) | (self.data[self.len] as CrcAccum);
  }
}

// Packet types:
// USR seq data
// RTX seq data
// NAK seq
// SYN SYN0
// SYN SYN1
// SYN SYN2
// SYN DIS

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

#[derive(Clone)]
// The UserPacket is the packet of data that the user of this library will see.
pub struct UserPacket<Driver: driver::Driver>
{
  len: usize,
  data: GenericArray<u8, Driver::PACKET_SIZE>,
}

impl<Driver: driver::Driver> UserPacket<Driver> {
  pub fn new() -> UserPacket<Driver> {
    UserPacket {
      len: 0,
      data: Default::default(),
    }
  }

  pub fn data(&self) -> &[u8] {
    &self.data[0..self.len]
  }

  pub fn append(&mut self, byte: u8) -> Result<(), ()> {
    if self.len < self.data.len() {
      self.data[self.len] = byte;
      self.len += 1;
      Ok(())
    } else {
      Err(())
    }
  }

  pub fn len(&self) -> usize {
    return self.len;
  }

  pub fn reset(&mut self) {
    self.len = 0;
  }

  pub fn remove_crc(&mut self) -> CrcAccum {
    if self.len < 2 {
      return 0;
    }

    // LSB is transmitted first
    self.len -= 2;
    return ((self.data[self.len] as CrcAccum) << 8) | (self.data[self.len + 1] as CrcAccum);
  }
}

enum InternalPacket<'a> {
  USR { seq: u8, data: &'a [u8] },
  RTX { seq: u8, data: &'a [u8] },
  NAK { seq: u8 },
  Syn0,
  Syn1,
  Syn2,
  Disconnect,
}
pub enum RawParseResult<'a> {
  RawPacketReceived {header: u8, data: &'a [u8] },
  AbortedPacket,
  MoreDataNeeded,
}

// A raw packet consists of a framing byte (SOF) followed by a one byte header,
// a variable amount of data, 2 CRC bytes another framing byte.
//
// The driver determines the maximum packet size.
//
// SOF HEADER ...data... CRC-LSB CRC-MSB SOF
#[derive(Clone)]
pub struct RawPacketParser<Driver: driver::Driver> {
  header: u8,
  crc: Crc,
  escape_state: EscapeState,
  frame_state: FrameState,
  user_packet: UserPacket<Driver>,
}

impl <Driver: driver::Driver> RawPacketParser<Driver> {
  
  pub fn new() -> RawPacketParser<Driver> {
    RawPacketParser {
      header: 0,
      crc: Crc::new(),
      escape_state: EscapeState::Normal,
      frame_state: FrameState::New,
      user_packet: UserPacket::new(),
    }    
  }

  /// Feeds a single byte into the raw packet parser. Once a complete packet
  /// has been parsed, a RawPacketReceived variant will be returned. The
  /// packet data will be valid until the next time that parse_u8 is called.
  pub fn parse_u8(&mut self, byte: u8) -> RawParseResult {
    let mut byte = byte;
    if self.escape_state == EscapeState::Escaping {
      self.escape_state = EscapeState::Normal;
      if byte == SOF {
        // ESC SOF is treated as an abort sequence
        self.frame_state = FrameState::New;
        return RawParseResult::AbortedPacket;
      }
      byte = byte ^ ESC_FLIP;
    } else if byte == SOF {
      if self.frame_state == FrameState::Collecting {
        // We've got a raw frame.
        self.frame_state = FrameState::New;
        return RawParseResult::RawPacketReceived {header: self.header, data: self.user_packet.data()};
      }
      // Receving a SOF while in the New state is considered a no-op
      return RawParseResult::MoreDataNeeded;
    }

    if self.frame_state == FrameState::New {
      // We're just starting a new frame. The first byte will be the header
      // and everything after that will be user bytes.
      self.reset();
      self.header = byte;
      self.frame_state = FrameState::Collecting;
    } else {
      if !self.user_packet.append(byte).is_ok() {
        // The payload was too big for the packet. This means that the SOF
        // was corrupted or a bad stream or something. We just reset the
        // receiver. Things will get resynchronized on the next valid frame.
        self.reset();
      }
    }
    self.crc.accum(byte);
    RawParseResult::MoreDataNeeded
  }

  pub fn reset(&mut self) {
    self.crc.reset();
    self.escape_state = EscapeState::Normal;
    self.user_packet.reset();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use typenum::U256;

  #[derive(Clone)]
  pub struct TestDriver {
  }
  impl driver::Driver for TestDriver {
    type PACKET_SIZE = U256;
    fn write_byte(&mut self, byte: u8) {}
  }

  fn parse_bytes<'a>(parser: &'a mut RawPacketParser<TestDriver>, bytes: &[u8]) -> RawParseResult<'a> {
    for byte in bytes {
      match parser.clone().parse_u8(*byte) {
        RawParseResult::RawPacketReceived {header, data} => {
          println!("Header = {} data = {:?}", header, data);
          return RawParseResult::RawPacketReceived {header, data};
        }

        RawParseResult::AbortedPacket => {
          return RawParseResult::AbortedPacket;
        }
        RawParseResult::MoreDataNeeded => {
          continue;
        }
      }
    }
    RawParseResult::MoreDataNeeded
  }
  #[test]
  fn test_raw_parser() {
    let mut parser: RawPacketParser<TestDriver> = RawPacketParser::new();
    let bytes = &vec![SOF, SOF];

    parse_bytes(&mut parser, bytes);
  }
}

