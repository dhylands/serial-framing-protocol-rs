use crate::crc::{Crc, CrcAccum, CRC_GOOD};

use core::mem::size_of;
use log::info;

use crate::traits::{PacketBuffer, ESC, ESC_FLIP, SOF};

#[derive(PartialEq, Debug)]
enum EscapeState {
    Normal,
    Escaping,
}

#[derive(PartialEq, Debug)]
enum FrameState {
    New,
    Collecting,
}

#[derive(PartialEq, Debug)]
pub enum RawParseResult {
    RawPacketReceived(u8),
    AbortedPacket,
    PacketTooSmall,
    CrcError(CrcAccum),
    MoreDataNeeded,
}

// A raw packet consists of a framing byte (SOF) followed by a one byte
//  header, a variable amount of data, 2 CRC bytes another framing byte.
//
// The caller determines the maximum packet size by implementing the
// PacketBuffer trait.
//
// So a packet will look like something like the following:
// SOF HEADER ...data... CRC-LSB CRC-MSB SOF
pub struct RawPacketParser {
    header: u8,
    crc: Crc,
    escape_state: EscapeState,
    frame_state: FrameState,
}

impl<'a> RawPacketParser {
    pub fn new() -> Self {
        RawPacketParser {
            header: 0,
            crc: Crc::new(),
            escape_state: EscapeState::Normal,
            frame_state: FrameState::New,
        }
    }

    pub fn dump(&self) {
        info!("header: {:02x}", self.header);
        info!("  escape_state: {:?}", self.escape_state);
        info!("  frame_state: {:?}", self.frame_state);
    }

    pub fn header(&self) -> u8 {
        self.header
    }

    /// Feeds a single byte into the raw packet parser. Once a complete packet
    /// has been parsed, a RawPacketReceived variant will be returned. The
    /// packet data will be stored in the PacketBuffer object that was passed
    /// to RawPacketParser::new() and will remain valid until the next time
    /// that parse_byte is called.
    pub fn parse_byte(&mut self, byte: u8, rx_data: &mut dyn PacketBuffer) -> RawParseResult {
        //info!("parse_byte 0x{:02x}", byte);
        let mut byte = byte;
        if self.escape_state == EscapeState::Escaping {
            self.escape_state = EscapeState::Normal;
            if byte == SOF {
                // ESC SOF is treated as an abort sequence
                self.frame_state = FrameState::New;
                self.reset();
                rx_data.reset();
                return RawParseResult::AbortedPacket;
            }
            byte ^= ESC_FLIP;
        } else if byte == SOF {
            if self.frame_state == FrameState::Collecting {
                // We've got a raw frame.
                self.frame_state = FrameState::New;

                if rx_data.len() < size_of::<CrcAccum>() {
                    return RawParseResult::PacketTooSmall;
                }

                let crc = rx_data.remove_crc();
                if self.crc.crc() != CRC_GOOD {
                    return RawParseResult::CrcError(crc);
                }

                return RawParseResult::RawPacketReceived(self.header);
            }
            // Receving a SOF while in the New state is considered a no-op
            return RawParseResult::MoreDataNeeded;
        } else if byte == ESC {
            self.escape_state = EscapeState::Escaping;
            return RawParseResult::MoreDataNeeded;
        }

        if self.frame_state == FrameState::New {
            // We're just starting a new frame. The first byte will be the header
            // and everything after that will be user bytes.
            self.reset();
            rx_data.reset();
            self.header = byte;
            self.frame_state = FrameState::Collecting;
        } else if rx_data.append(byte).is_err() {
            // The payload was too big for the packet. This means that the SOF
            // was corrupted or a bad stream or something. We just reset the
            // receiver. Things will get resynchronized on the next valid frame.
            self.reset();
            rx_data.reset();
        }
        self.crc.accum(byte);
        RawParseResult::MoreDataNeeded
    }

    pub fn reset(&mut self) {
        self.crc.reset();
        self.escape_state = EscapeState::Normal;
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
    use crate::testutils::{parse_bytes, parse_bytes_as_packet, setup_log, TestPacketBuffer};
    use crate::traits::WritePacket;
    use log::info;
    use pretty_hex::*;
    use std::vec::Vec;

    fn encode_decode_packet(parser: &mut RawPacketParser, header: u8, data: &[u8]) -> Vec<u8> {
        // Write the packet out and collect it.
        info!("=== encode_decode_packet ===");
        info!("=== Input ===");
        info!("Header: 0x{:02x} Data: {:?}", header, data.hex_dump());
        let mut writer = TestPacketBuffer::new();
        writer.write_packet_data(header, data);

        info!("=== Output ===");
        info!("{:?}", (&writer.data()).hex_dump());

        // Then run the generated packet through the packet parser
        let mut rx_data = TestPacketBuffer::new();
        let ret = parse_bytes_as_packet(parser, &writer.data(), &mut rx_data);
        info!("=== Reparsed ===");
        info!("{:?}", &ret.hex_dump());
        ret
    }

    // Uses the WritePacket trait to convert a user packet into a Vec<u8> of
    // the bytes that would be written to a real device.
    fn write_packet(header: u8, data: &[u8]) -> Vec<u8> {
        info!("=== write_packet ===");
        info!("=== Input ===");
        info!("Header: 0x{:02x} Data: {:?}", header, data.hex_dump());
        let mut writer = TestPacketBuffer::new();
        writer.write_packet_data(header, data);

        info!("=== Output ===");
        info!("{:?}", (&writer.data()).hex_dump());

        (&writer.data()).to_vec()
    }

    #[test]
    fn test_raw_parser() {
        setup_log();
        let mut packet = TestPacketBuffer::new();
        let mut parser = RawPacketParser::new();

        // Cover every type of return code from the parser
        let tests = &vec![
            (vec![SOF, SOF], RawParseResult::MoreDataNeeded),
            (vec![SOF, 0x00, SOF], RawParseResult::PacketTooSmall),
            (vec![SOF, 0x00, 0x00, SOF], RawParseResult::PacketTooSmall),
            (
                vec![SOF, 0x00, 0x00, 0x00, SOF],
                RawParseResult::CrcError(0x0000),
            ),
            (
                vec![SOF, 0xc0, 0x74, 0x36, SOF],
                RawParseResult::RawPacketReceived(0xc0),
            ),
            (
                vec![SOF, 0x00, 0x78, 0xf0, SOF],
                RawParseResult::RawPacketReceived(0x00),
            ),
            (
                vec![SOF, 0xc0, 0xee, 0x9d, 0xcb, SOF],
                RawParseResult::RawPacketReceived(0xc0),
            ),
            (
                vec![SOF, 0xc0, 0x11, 0x22, 0x33, 0x44, 0x73, 0x75, SOF],
                RawParseResult::RawPacketReceived(0xc0),
            ),
            (
                vec![SOF, 0xc0, 0x11, ESC, SOF],
                RawParseResult::AbortedPacket,
            ),
        ];

        info!("----- Testing parsing results -----");
        for test in tests.iter() {
            assert_eq!(parse_bytes(&mut parser, &test.0, &mut packet), test.1);
        }

        // Verify that escaping works

        // NOTE: packets with a header of 0xcc won't be run through the writer test since those tests
        //       don't regenerate the input.
        let tests = &vec![
            // Plain unescaped packet
            (
                vec![SOF, 0xc0, 0x11, 0x5e, 0xe4, 0xfb, SOF],
                vec![0xc0, 0x11, 0x5e],
            ),
            // Packet with an escaped SOF
            (
                vec![SOF, 0xc0, 0x11, ESC, 0x5e, 0xe6, 0xda, SOF],
                vec![0xc0, 0x11, SOF],
            ),
            // Packet with an escaped ESC and a second time because the CRC happens to have the ESC character
            (
                vec![SOF, 0xc0, 0x11, ESC, 0x5d, ESC, 0x5d, 0xe8, SOF],
                vec![0xc0, 0x11, ESC],
            ),
            // Packet with an escaped space
            (
                vec![SOF, 0xcc, 0x11, ESC, 0x00, 0xbe, 0xc4, SOF],
                vec![0xcc, 0x11, 0x20],
            ),
            // Make sure double SOF is ignored
            (
                vec![SOF, SOF, 0xcc, 0x11, 0x5e, 0x47, 0x5e, SOF],
                vec![0xcc, 0x11, 0x5e],
            ),
            // Make sure that 2 packets with just a single SOF between then is parsed
            // properly. For this we just leave off the leading SOF since the trailing SOF
            // from the previous packet should be sufficient.
            (
                vec![0xcc, 0x11, 0x5e, 0x47, 0x5e, SOF],
                vec![0xcc, 0x11, 0x5e],
            ),
        ];

        info!("----- Testing parser -----");
        // Test that the packet parser produces the correct results
        for test in tests.iter() {
            assert_eq!(
                parse_bytes_as_packet(&mut parser, &test.0, &mut packet),
                test.1
            );
        }

        info!("----- Testing writer -----");
        // Flip things around and verify that given the raw packet, we get the written stream.
        for test in tests.iter() {
            let header = test.1[0];
            if header != 0xcc {
                let data = &test.1[1..test.1.len()];
                assert_eq!(write_packet(header, data), test.0);
            }
        }
    }

    #[test]
    fn test_packet_encode_decode() {
        setup_log();
        let mut parser = RawPacketParser::new();

        // Take each of the folloing "user packets", write them out, and then
        // reparse to make sure that we get the original packets back.

        let tests = &vec![
            vec![0xc0],
            vec![0xc0, 0x11],
            vec![0xc0, 0x11, 0x22],
            vec![SOF],
            vec![SOF, SOF],
            vec![SOF, SOF, SOF],
            vec![ESC],
            vec![ESC, ESC],
            vec![ESC, ESC, ESC],
        ];

        info!("----- Testing encode/decode -----");
        for test in tests.iter() {
            let header = test[0];
            let data = &test[1..test.len()];
            assert_eq!(&encode_decode_packet(&mut parser, header, data), test);
        }
    }
}
