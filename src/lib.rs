#![no_std]

#[cfg(test)]
#[macro_use]
extern crate std;

use core::mem::size_of;
use log::{debug, warn};

#[macro_use]
pub mod macros;

pub mod crc;
pub mod driver;
pub mod rawpacket;

use crc::{Crc, CrcAccum};

enum InternalPacket<'a> {
    USR { seq: u8, data: &'a [u8] },
    RTX { seq: u8, data: &'a [u8] },
    NAK { seq: u8 },
    Syn0,
    Syn1,
    Syn2,
    Disconnect,
}

//use packet::Packet;
/*
const SOF: u8 = 0x7e;   // Start of Frame
const ESC: u8 = 0x7d;
const ESC_FLIP: u8 = 1 << 5;

const SEQ_INIT: u8 = 0;
const SEQ_MASK: u8 = 0x3f;        // Lower 6 bits
const FRAME_TYPE_MASK: u8 = 0xc0; // Upper 2 bits

#[derive(PartialEq)]
enum EscapeState {
  Normal,
  Escaping,
}

#[derive(PartialEq)]
enum FrameState {
  New,
  Receiving,
}

#[derive(PartialEq)]
enum ConnectState {
  Disconnected,
  SentSyn0,
  SentSyn1,
  Connected,
}

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

pub struct Transmitter {
  seq:  u8,
  crc:  Crc,
}

struct Receiver<Driver: driver::Driver> {
  seq:  u8,
  crc:  Crc,
  escape_state: EscapeState,
  frame_state: FrameState,
  header: u8,
  packet: Packet<Driver>,
}

impl<Driver: driver::Driver> Receiver<Driver> {
  fn new() -> Receiver<Driver> {
    Receiver {
      seq: SEQ_INIT,
      crc: Crc::new(),
      escape_state: EscapeState::Normal,
      frame_state: FrameState::New,
      header: 0,
      packet: Packet::new(),
    }
  }

  fn get_frame_type(&self) -> FrameType {
    if let Some(frame_type) =  FrameType::from_u8(self.header & FRAME_TYPE_MASK) {
      frame_type
    } else {
      // We can't actually get this case.
      FrameType::NAK
    }
  }

  fn get_frame_seq(&self) -> u8 {
    return self.header & SEQ_MASK;
  }

  fn reset(&mut self) {
    self.crc.reset();
    self.escape_state = EscapeState::Normal;
    self.frame_state = FrameState::New;
    self.packet.reset();
  }
}

impl Transmitter {
  fn new() -> Self {
    Transmitter {
      seq: SEQ_INIT,
      crc: Crc::new(),
    }
  }
}

pub struct Context<Driver: driver::Driver> {
  connect_state: ConnectState,
  tx:   Transmitter,
  rx:   Receiver<Driver>,
  driver: Driver,
}

impl <Driver: driver::Driver> Context<Driver> {
  fn next_frame_seq(&self, seq: u8) -> u8 {
    return (seq + 1) & SEQ_MASK;
  }
}

pub enum DeliverResult<'a, Driver: driver::Driver> {
  PacketReceived(&'a Packet<Driver>),
  MoreDataNeeded,
  CrcError,
  PacketTooSmall,
}

impl<Driver: driver::Driver> Context<Driver> {
  pub fn new(driver: Driver) -> Self {
    Context {
      connect_state: ConnectState::Disconnected,
      tx: Transmitter::new(),
      rx: Receiver::new(),
      driver,
    }
  }

  pub fn deliver_byte(&mut self, byte: u8) -> DeliverResult<Driver> {
    if byte == SOF {
      if self.rx.frame_state == FrameState::Receiving {
        // We've got a frame
        return self.handle_frame();
      }
      self.rx.reset();
    } else if byte == ESC {
      self.rx.escape_state = EscapeState::Escaping;
    } else {
      let mut byte = byte;
      if self.rx.escape_state == EscapeState::Escaping {
        byte = byte ^ ESC_FLIP;
        self.rx.escape_state = EscapeState::Normal;
      }
      self.rx.crc.accum(byte);
      if self.rx.frame_state == FrameState::New {
        self.rx.header = byte;
        self.rx.frame_state = FrameState::Receiving;
      } else {
        if !self.rx.packet.append(byte).is_ok() {
          // The payload was too big for the packet. This means that the SOF
          // was corrupted or a bad stream or something. We just reset the
          // receiver.
          self.rx.reset();
        }
      }
    }
    DeliverResult::MoreDataNeeded
  }

  pub fn handle_frame(&mut self) -> DeliverResult<Driver> {
    if self.rx.packet.len() < size_of::<CrcAccum>() {
      warn!("Short frame received - sending NAK");
      self.transmit_nak(self.rx.seq);
      return DeliverResult::PacketTooSmall;
    }

    let crc = self.rx.packet.remove_crc();
    if crc != crc::CRC_GOOD {
      warn!("CRC mismatch - sending NAK");
      self.transmit_nak(self.rx.seq);
      return DeliverResult::CrcError;
    }
    let frame_type = self.rx.get_frame_type();
    match frame_type {
      FrameType::USR | FrameType::RTX => {
        return self.handle_frame_usr();
      },
      FrameType::NAK => {
        self.handle_frame_nak();
      },
      FrameType::SYN => {
        self.handle_frame_syn();
      },
    }
    DeliverResult::MoreDataNeeded
  }

  fn handle_frame_usr(&mut self) -> DeliverResult<Driver> {
    let frame_type = self.rx.get_frame_type();
    let seq = self.rx.get_frame_seq();
    debug!("Received {:?} frame, Seq: {}", frame_type, seq);

    match self.connect_state {
      ConnectState::Disconnected => {
        self.transmit_dis();
      },
      ConnectState::SentSyn0 => {
        self.transmit_syn0();
      },
      ConnectState::SentSyn1 => {
        self.transmit_syn1();
      },
      ConnectState::Connected => {
        if seq != self.rx.seq {
          if frame_type == FrameType::USR {
            warn!("Out of order frame received - sending NAK");
            self.transmit_nak(self.rx.get_frame_seq());
          } else {
            warn!("Out of order retransmitted frame frame received - ignoring");
          }
        } else {
          // Good user frame received and accepted. Deliver it.
          self.rx.seq = self.next_frame_seq(self.rx.seq);
          return DeliverResult::PacketReceived(&self.rx.packet);
        }
      },
    }
    DeliverResult::MoreDataNeeded
  }

  fn handle_frame_nak(&mut self) {
    //TODO

  }

  fn handle_frame_syn(&mut self) {
    let seq = self.rx.get_frame_seq();

    match SeqSyn::from_u8(seq) {
      Some(SeqSyn::SYN0) => self.handle_frame_syn0(),
      Some(SeqSyn::SYN1) => self.handle_frame_syn1(),
      Some(SeqSyn::SYN2) => self.handle_frame_syn2(),
      Some(SeqSyn::DIS) => self.handle_frame_dis(),
      None => {
        warn!("SYN with unknown seq: {}", seq);
      }
    }
  }

  fn handle_frame_syn0(&mut self) {
    self.rx.seq = SEQ_INIT;
    self.tx.seq = SEQ_INIT;
    self.clear_history();
    self.connect_state = ConnectState::SentSyn1;
    self.transmit_syn1();
  }

  fn handle_frame_syn1(&mut self) {
    if self.connect_state == ConnectState::Disconnected {
      self.transmit_dis();
      return;
    }
    self.connect_state = ConnectState::Connected;
    debug!("Connected (after SYN1)");
    self.transmit_syn2();
    if self.tx.seq != SEQ_INIT {
      self.transmit_history_from_seq(SEQ_INIT);
    }
  }

  fn handle_frame_syn2(&mut self) {
    if self.connect_state == ConnectState::Disconnected {
      self.transmit_dis();
      return;
    }
    if self.connect_state == ConnectState::SentSyn0 {
      self.transmit_syn0();
      return;
    }
    self.connect_state = ConnectState::Connected;
    debug!("Connected (after SYN2)");
    self.transmit_syn2();
    if self.tx.seq != SEQ_INIT {
      self.transmit_history_from_seq(SEQ_INIT);
    }
  }

  fn handle_frame_dis(&mut self) {
    self.connect_state = ConnectState::Disconnected;
  }

  fn clear_history(&mut self) {
    // TODO
  }

  fn transmit_history_from_seq(&mut self, seq: u8) {
    // TODO
  }

  fn transmit_nak(&mut self, seq: u8) {
    self.transmit_control_packet(FrameType::NAK, seq);
  }

  fn transmit_dis(&mut self) {
    self.transmit_control_packet(FrameType::SYN, SeqSyn::DIS as u8);
  }

  fn transmit_syn0(&mut self) {
    self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN0 as u8);
  }

  fn transmit_syn1(&mut self) {
    self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN1 as u8);
  }

  fn transmit_syn2(&mut self) {
    self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN2 as u8);
  }

  fn transmit_control_packet(&mut self, frame_type: FrameType, seq: u8) {
    let header = (frame_type as u8) | (seq & SEQ_MASK);
    self.tx.crc.reset();

    self.driver.start_write();
    self.driver.write_byte(SOF);
    self.write_escaped(header);
    self.write_crc();
    self.driver.write_byte(SOF);
    self.driver.end_write();
  }

  fn write_crc(&mut self) {
    // Write the CRC out LSB first
    let crc_lsb = self.tx.crc.lsb();
    let crc_msb = self.tx.crc.msb();
    self.write_escaped(crc_lsb);
    self.write_escaped(crc_msb);
  }

  fn write_escaped(&mut self, byte: u8) {
    self.tx.crc.accum(byte);
    if byte == ESC || byte == SOF {
      self.driver.write_byte(ESC);
      self.driver.write_byte(ESC ^ ESC_FLIP);
    } else {
      self.driver.write_byte(byte);
    }
  }
}
*/
