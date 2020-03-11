#![no_std]

#[cfg(test)]
#[macro_use]
extern crate std;

use log::{debug, warn, error};

#[macro_use]
pub mod macros;

pub mod crc;
pub mod driver;
pub mod packet;
pub mod rawpacket;
pub mod traits;

#[cfg(test)]
mod testutils;

use crc::CrcAccum;
use packet::{FrameType, PacketParser, PacketType, PacketTypeResult, SeqSyn, SEQ_MASK};
use traits::{PacketBuffer, WritePacket};

const SEQ_INIT: u8 = 0;

#[derive(PartialEq)]
enum ConnectState {
    Disconnected,
    SentSyn0,
    SentSyn1,
    Connected,
}

#[derive(Debug, PartialEq)]
pub enum ParseResult {
    UserPacket,
    AbortedPacket,
    PacketTooSmall,
    CrcError(CrcAccum),
    MoreDataNeeded,
}

pub struct Transmitter {
    connect_state: ConnectState,
    rx_seq: u8,
    tx_seq: u8,
}

struct Receiver {
    parser: PacketParser,
}

impl Receiver {
    fn new() -> Self {
        Self {
            parser: PacketParser::new(),
        }
    }

    fn reset(&mut self) {
        self.parser.reset();
    }
}

impl Transmitter {
    fn new() -> Self {
        Self {
            connect_state: ConnectState::Disconnected,
            rx_seq: SEQ_INIT,
            tx_seq: SEQ_INIT,
        }
    }

    fn reset(&mut self) {
        self.connect_state = ConnectState::Disconnected;
        self.rx_seq = SEQ_INIT;
        self.tx_seq = SEQ_INIT;
        self.clear_history();
    }

    fn next_frame_seq(&self, seq: u8) -> u8 {
        return (seq + 1) & SEQ_MASK;
    }

    pub fn handle_packet(
        &mut self,
        packet_type: PacketType,
        writer: &mut dyn WritePacket,
    ) -> ParseResult {
        debug!("Received {:?}", packet_type);
        match packet_type {
            PacketType::USR { seq } => {
                return self.handle_frame_usr_rtx(FrameType::USR, seq, writer);
            }
            PacketType::RTX { seq } => {
                return self.handle_frame_usr_rtx(FrameType::RTX, seq, writer);
            }
            PacketType::NAK { seq } => {
                self.handle_frame_nak(seq, writer);
            }
            PacketType::Syn0 => {
                self.handle_frame_syn0(writer);
            }
            PacketType::Syn1 => {
                self.handle_frame_syn1(writer);
            }
            PacketType::Syn2 => {
                self.handle_frame_syn2(writer);
            }
            PacketType::Disconnect => {
                self.handle_frame_disconnect();
            }
        }
        ParseResult::MoreDataNeeded
    }

    fn handle_frame_usr_rtx(
        &mut self,
        frame_type: FrameType,
        seq: u8,
        writer: &mut dyn WritePacket,
    ) -> ParseResult {
        match self.connect_state {
            ConnectState::Disconnected => {
                self.transmit_dis(writer);
            }
            ConnectState::SentSyn0 => {
                self.transmit_syn0(writer);
            }
            ConnectState::SentSyn1 => {
                self.transmit_syn1(writer);
            }
            ConnectState::Connected => {
                if seq != self.rx_seq {
                    if frame_type == FrameType::USR {
                        warn!("Out of order frame received - sending NAK");
                        self.transmit_nak(self.rx_seq, writer);
                    } else {
                        warn!("Out of order retransmitted frame frame received - ignoring");
                    }
                } else {
                    // Good user frame received and accepted. Deliver it.
                    self.rx_seq = self.next_frame_seq(self.rx_seq);
                    return ParseResult::UserPacket;
                }
            }
        }
        ParseResult::MoreDataNeeded
    }

    fn handle_frame_nak(&mut self, _seq: u8, _writer: &mut dyn WritePacket) {
        //TODO
    }

    fn handle_frame_syn0(&mut self, writer: &mut dyn WritePacket) {
        self.rx_seq = SEQ_INIT;
        self.tx_seq = SEQ_INIT;
        self.clear_history();
        self.connect_state = ConnectState::SentSyn1;
        self.transmit_syn1(writer);
    }

    fn handle_frame_syn1(&mut self, writer: &mut dyn WritePacket) {
        if self.connect_state == ConnectState::Disconnected {
            self.transmit_dis(writer);
            return;
        }
        self.connect_state = ConnectState::Connected;
        debug!("Connected (after SYN1)");
        self.transmit_syn2(writer);
        if self.tx_seq != SEQ_INIT {
            self.transmit_history_from_seq(SEQ_INIT, writer);
        }
    }

    fn handle_frame_syn2(&mut self, writer: &mut dyn WritePacket) {
        if self.connect_state == ConnectState::Disconnected {
            self.transmit_dis(writer);
            return;
        }
        if self.connect_state == ConnectState::SentSyn0 {
            self.transmit_syn0(writer);
            return;
        }
        self.connect_state = ConnectState::Connected;
        debug!("Connected (after SYN2)");
        if self.tx_seq != SEQ_INIT {
            self.transmit_history_from_seq(SEQ_INIT, writer);
        }
    }

    fn handle_frame_disconnect(&mut self) {
        self.connect_state = ConnectState::Disconnected;
    }

    fn clear_history(&mut self) {
        // TODO
    }

    fn transmit_history_from_seq(&mut self, _seq: u8, _writer: &mut dyn WritePacket) {
        // TODO
    }

    fn transmit_nak(&mut self, seq: u8, writer: &mut dyn WritePacket) {
        self.transmit_control_packet(FrameType::NAK, seq, writer);
    }

    fn transmit_dis(&mut self, writer: &mut dyn WritePacket) {
        self.transmit_control_packet(FrameType::SYN, SeqSyn::DIS as u8, writer);
    }

    fn transmit_syn0(&mut self, writer: &mut dyn WritePacket) {
        self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN0 as u8, writer);
    }

    fn transmit_syn1(&mut self, writer: &mut dyn WritePacket) {
        self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN1 as u8, writer);
    }

    fn transmit_syn2(&mut self, writer: &mut dyn WritePacket) {
        self.transmit_control_packet(FrameType::SYN, SeqSyn::SYN2 as u8, writer);
    }

    fn transmit_control_packet(
        &mut self,
        frame_type: FrameType,
        seq: u8,
        writer: &mut dyn WritePacket,
    ) {
        let header = (frame_type as u8) | (seq & SEQ_MASK);
        let data: &[u8] = &[];

        writer.write_packet_data(header, data);
    }
}

pub struct Context {
    tx: Transmitter,
    rx: Receiver,
}

impl Context {
    pub fn new() -> Self {
        Self {
            tx: Transmitter::new(),
            rx: Receiver::new(),
        }
    }

    pub fn connect(&mut self, writer: &mut dyn WritePacket) {
        self.tx.reset();
        self.rx.reset();
        self.tx.transmit_syn0(writer);
        self.tx.connect_state = ConnectState::SentSyn0;
    }

    pub fn is_connected(&self) -> bool {
        return self.tx.connect_state == ConnectState::Connected;
    }

    pub fn parse_byte(
        &mut self,
        byte: u8,
        rx_data: &mut dyn PacketBuffer,
        writer: &mut dyn WritePacket,
    ) -> ParseResult {
        let parse_result = self.rx.parser.parse_byte(byte, rx_data);
        match parse_result {
            PacketTypeResult::PacketReceived(packet_type) => {
                self.tx.handle_packet(packet_type, writer)
            }
            PacketTypeResult::AbortedPacket => ParseResult::AbortedPacket,
            PacketTypeResult::PacketTooSmall => ParseResult::PacketTooSmall,
            PacketTypeResult::CrcError(rcvd_crc) => ParseResult::CrcError(rcvd_crc),
            PacketTypeResult::MoreDataNeeded => ParseResult::MoreDataNeeded,
        }
    }

    pub fn write_packet(&mut self, data: &[u8], writer: &mut dyn WritePacket) {
        if !self.is_connected() {
            error!("Not connected");
            return;
        }
        let header: u8 = FrameType::USR as u8 | self.tx.tx_seq;

        // TODO Add the packet to the transmit history
        writer.write_packet_data(header, data);
        self.tx.tx_seq = self.tx.next_frame_seq(self.tx.tx_seq);
    }
}

// ===========================================================================
//
// Tests
//
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutils::{setup_log, TestPacketBuffer};
    use crate::traits::{PacketBuffer, SOF};
    use log::info;

    impl Context {
        // Parse a bunch of bytes and return the first return code that isn't
        // MoreDataNeeded. This means that this function will parse at most one
        // error or packet from the input stream, which is fine for testing.
        pub fn parse_bytes(
            &mut self,
            bytes: &[u8],
            rx_packet: &mut dyn PacketBuffer,
            writer: &mut dyn WritePacket,
        ) -> ParseResult {
            writer.start_write();   // Clears the outout buffer.
            for byte in bytes.iter() {
                let parse_result = self.parse_byte(*byte, rx_packet, writer);
                match parse_result {
                    ParseResult::UserPacket => {
                        return ParseResult::UserPacket;
                    }

                    ParseResult::MoreDataNeeded => {
                        continue;
                    }

                    ParseResult::AbortedPacket => {
                        return ParseResult::AbortedPacket;
                    }

                    ParseResult::PacketTooSmall => {
                        return ParseResult::PacketTooSmall;
                    }

                    ParseResult::CrcError(rcvd_crc) => {
                        return ParseResult::CrcError(rcvd_crc);
                    }
                }
            }
            ParseResult::MoreDataNeeded
        }
    }

    #[test]
    fn test_connect() {
        setup_log();

        info!("Running test_connect");

        let mut packet1to2 = TestPacketBuffer::new();
        let mut packet2to1 = TestPacketBuffer::new();
        let mut packet1 = TestPacketBuffer::new();
        let mut packet2 = TestPacketBuffer::new();
        let mut ctx1 = Context::new();
        let mut ctx2 = Context::new();

        ctx1.connect(&mut packet1to2);

        // This should put a SYN0 packet into packet2
        assert_eq!(packet1to2.data().to_vec(), vec![SOF, 0xc0, 0x74, 0x36, SOF]);

        // Sending the SYN0 to the other side, should generate a SYN1 in response
        assert_eq!(
            ctx2.parse_bytes(packet1to2.data(), &mut packet2, &mut packet2to1),
            ParseResult::MoreDataNeeded
        );
        assert_eq!(packet2to1.data().to_vec(), vec![SOF, 0xc1, 0xfd, 0x27, SOF]);

        // Sending SYN1 to initial side should generate a SYN2 in response Side 1 should be connected
        assert_eq!(
            ctx1.parse_bytes(packet2to1.data(), &mut packet1, &mut packet1to2),
            ParseResult::MoreDataNeeded
        );
        assert!(ctx1.is_connected());
        assert_eq!(packet1to2.data().to_vec(), vec![SOF, 0xc2, 0x66, 0x15, SOF]);

        // Sending the SYN2 to Side 2 should then put it into a connected state
        assert_eq!(
            ctx2.parse_bytes(packet1to2.data(), &mut packet2, &mut packet2to1),
            ParseResult::MoreDataNeeded
        );
        assert!(ctx2.is_connected());
        assert_eq!(packet2to1.data().to_vec(), vec![]);

        // Send a User packet from Side 1 to Side 2

        ctx1.write_packet("Testing".as_bytes(), &mut packet1to2);
        assert_eq!(packet1to2.data().to_vec(), vec![SOF, 0x00, 0x54, 0x65, 0x73, 0x74, 0x69, 0x6e, 0x67, 0xc5, 0x5c, SOF]);
        assert_eq!(
            ctx2.parse_bytes(packet1to2.data(), &mut packet2, &mut packet2to1),
            ParseResult::UserPacket
        );
        assert_eq!(packet2.data(), "Testing".as_bytes());
        assert_eq!(packet2to1.data().to_vec(), vec![]);

        //info!("packet1to2: {:?}", packet1to2.dump());
    }
}
