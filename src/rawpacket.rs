use crate::crc::{Crc, CrcAccum, CRC_GOOD};

use core::fmt;
use core::mem::size_of;
use core::result::Result;
use log::info;
use pretty_hex::*;

const SOF: u8 = 0x7e; // Start of Frame
const ESC: u8 = 0x7d;
const ESC_FLIP: u8 = 0x20;

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

pub trait PacketBuffer {
    /// Returns the capacity of the packet buffer.
    fn capacity(&self) -> usize;

    /// Resets the packet buffer to start collecting a new set of bytes.
    fn reset(&mut self);

    /// Appends a byte to the end of the packet buffer. This function will
    /// return an error result if the packet buffer is full.
    fn append(&mut self, byte: u8) -> Result<(), ()>;

    /// Removes the CRC from the packet buffer. The CRC is assumed to be a
    /// 16-bit CRC stored with LSB first (i.e. LSB is at a lower memory
    /// location that the MSB)
    fn remove_crc(&mut self) -> CrcAccum;

    /// Returns the number of bytes which have currently been accumulated in
    /// the packet buffer.
    fn len(&self) -> usize;

    /// Determines if the current buffer is currently empty or not.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a slice containing valid packet data.
    fn data(&self) -> &[u8];
}

impl fmt::Debug for dyn PacketBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.data().hex_dump())
    }
}

pub trait WritePacket {
    /// Called at the beginning of writing a packet. Allows the driver implementation to implement
    /// buffering.
    fn start_write(&mut self) {}

    /// Called to write some data (not necessarily a complete packet) to the hardware.
    fn write_byte(&mut self, byte: u8);

    /// Called at the end of the writing a packet. Allows the driver to flush a
    /// buffer if a buffered implementation is used.
    fn end_write(&mut self) {}

    /// Called to write an entire packet
    fn write_packet_data(&mut self, header: u8, bytes: &[u8]) {
        info!(
            "write_packet_data header: 0x{:02x} len: {}",
            header,
            bytes.len()
        );
        let mut crc = Crc::new();
        self.start_write();
        self.write_byte(SOF);
        self.write_escaped_byte(&mut crc, header);
        self.write_escaped_bytes(&mut crc, bytes);
        self.write_crc(&mut crc);
        self.write_byte(SOF);
        self.end_write();
    }

    fn write_crc(&mut self, crc: &mut Crc) {
        // Write the CRC out LSB first
        let crc_lsb = crc.lsb();
        let crc_msb = crc.msb();
        self.write_escaped_byte(crc, crc_lsb);
        self.write_escaped_byte(crc, crc_msb);
    }

    fn write_escaped_bytes(&mut self, crc: &mut Crc, bytes: &[u8]) {
        for byte in bytes {
            self.write_escaped_byte(crc, *byte);
        }
    }

    fn write_escaped_byte(&mut self, crc: &mut Crc, byte: u8) {
        crc.accum(byte);
        if byte == ESC || byte == SOF {
            self.write_byte(ESC);
            self.write_byte(byte ^ ESC_FLIP);
        } else {
            self.write_byte(byte);
        }
    }
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
pub struct RawPacketParser<'a> {
    header: u8,
    crc: Crc,
    escape_state: EscapeState,
    frame_state: FrameState,
    packet: &'a mut (dyn PacketBuffer + 'a),
}

impl<'a> RawPacketParser<'a> {
    pub fn new(packet: &'a mut (dyn PacketBuffer + 'a)) -> Self {
        RawPacketParser {
            header: 0,
            crc: Crc::new(),
            escape_state: EscapeState::Normal,
            frame_state: FrameState::New,
            packet,
        }
    }

    pub fn dump(&self) {
        info!("header: {:02x}", self.header);
        info!("  escape_state: {:?}", self.escape_state);
        info!("  frame_state: {:?}", self.frame_state);
        info!("  packet: {:?}", self.packet.data().hex_dump());
    }

    pub fn data(&self) -> &[u8] {
        self.packet.data()
    }

    pub fn header(&self) -> u8 {
        self.header
    }

    /// Feeds a single byte into the raw packet parser. Once a complete packet
    /// has been parsed, a RawPacketReceived variant will be returned. The
    /// packet data will be stored in the PacketBuffer object that was passed
    /// to RawPacketParser::new() and will remain valid until the next time
    /// that parse_u8 is called.
    pub fn parse_u8(&mut self, byte: u8) -> RawParseResult {
        //info!("parse_u8 0x{:02x}", byte);
        let mut byte = byte;
        if self.escape_state == EscapeState::Escaping {
            self.escape_state = EscapeState::Normal;
            if byte == SOF {
                // ESC SOF is treated as an abort sequence
                self.frame_state = FrameState::New;
                self.reset();
                return RawParseResult::AbortedPacket;
            }
            byte ^= ESC_FLIP;
        } else if byte == SOF {
            if self.frame_state == FrameState::Collecting {
                // We've got a raw frame.
                self.frame_state = FrameState::New;

                if self.packet.len() < size_of::<CrcAccum>() {
                    return RawParseResult::PacketTooSmall;
                }

                let crc = self.packet.remove_crc();
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
            self.header = byte;
            self.frame_state = FrameState::Collecting;
        } else if self.packet.append(byte).is_err() {
            // The payload was too big for the packet. This means that the SOF
            // was corrupted or a bad stream or something. We just reset the
            // receiver. Things will get resynchronized on the next valid frame.
            self.reset();
        }
        self.crc.accum(byte);
        RawParseResult::MoreDataNeeded
    }

    pub fn reset(&mut self) {
        self.crc.reset();
        self.escape_state = EscapeState::Normal;
        self.packet.reset();
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
    use log::{error, info, warn};
    use simple_logger;
    use std::sync::Once;
    use std::vec::Vec;

    static INIT: Once = Once::new();

    fn setup() {
        INIT.call_once(|| {
            simple_logger::init().unwrap();
        });
    }
    struct TestPacketBuffer {
        len: usize,
        buf: [u8; 256],
    }

    impl fmt::Debug for TestPacketBuffer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{:?}", (&self.buf[0..self.len]).hex_dump())
        }
    }

    impl TestPacketBuffer {
        fn new() -> Self {
            Self {
                len: 0,
                buf: [0; 256],
            }
        }
    }

    impl PacketBuffer for TestPacketBuffer {
        fn capacity(&self) -> usize {
            self.buf.len()
        }

        fn reset(&mut self) {
            self.len = 0;
        }

        fn append(&mut self, byte: u8) -> Result<(), ()> {
            if self.len < self.capacity() {
                self.buf[self.len] = byte;
                self.len += 1;
                Ok(())
            } else {
                Err(())
            }
        }

        fn remove_crc(&mut self) -> CrcAccum {
            if self.len < size_of::<CrcAccum>() {
                return 0;
            }

            // LSB is transmitted first
            self.len -= 2;
            ((self.buf[self.len + 1] as CrcAccum) << 8) | (self.buf[self.len] as CrcAccum)
        }

        fn len(&self) -> usize {
            self.len
        }

        fn data(&self) -> &[u8] {
            &self.buf[0..self.len]
        }
    }

    impl WritePacket for TestPacketBuffer {
        fn start_write(&mut self) {
            //info!("start_write");
            self.reset();
        }

        fn write_byte(&mut self, byte: u8) {
            //info!("write_byte 0x{:02x}", byte);
            self.append(byte).unwrap();
        }
    }

    // Parse a bunch of bytes and return the first return code that isn't
    // MoreDataNeeded. This means that this function will parse at most one
    // error or packet from the input stream, which is fine for testing.
    fn parse_bytes<'a>(parser: &mut RawPacketParser, bytes: &[u8]) -> RawParseResult {
        for byte in bytes {
            let parse_result = parser.parse_u8(*byte);
            match parse_result {
                RawParseResult::RawPacketReceived(header) => {
                    info!(
                        "Header = {:02x} data = {:?}",
                        header,
                        parser.data().hex_dump()
                    );
                    return RawParseResult::RawPacketReceived(header);
                }

                RawParseResult::MoreDataNeeded => {
                    continue;
                }

                RawParseResult::CrcError(rcvd_crc) => {
                    let mut crc = Crc::new();
                    crc.accum(parser.header());
                    let expected_crc = !crc.accum_bytes(parser.data());
                    warn!(
                        "CRC Error: Rcvd {:04x} Expected {:04x}",
                        rcvd_crc, expected_crc
                    );
                    return RawParseResult::CrcError(rcvd_crc);
                }

                _ => {
                    info!("{:?}", parse_result);
                    return parse_result;
                }
            }
        }
        info!("MoreDataNeeded");
        RawParseResult::MoreDataNeeded
    }

    fn parse_bytes_as_packet(parser: &mut RawPacketParser, bytes: &[u8]) -> Vec<u8> {
        let parse_result = parse_bytes(parser, bytes);
        match parse_result {
            RawParseResult::RawPacketReceived(header) => {
                let mut vec = Vec::new();
                vec.push(header);
                vec.extend_from_slice(parser.data());
                return vec;
            }
            _ => {
                error!("{:?}", parse_result);
                return Vec::new();
            }
        }
    }

    fn encode_decode_packet(parser: &mut RawPacketParser, header: u8, data: &[u8]) -> Vec<u8> {
        // Write the packet out and collect it.
        info!("=== encode_decode_packet ===");
        info!("=== Input ===");
        info!("Header: 0x{:02x} Data: {:?}", header, data.hex_dump());
        let mut writer = TestPacketBuffer::new();
        writer.write_packet_data(header, data);

        info!("=== Output ===");
        info!("{:?}", (&writer.buf[0..writer.len]).hex_dump());

        // Then run the generated packet through the packet parser
        let ret = parse_bytes_as_packet(parser, &writer.buf[0..writer.len]);
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
        info!("{:?}", (&writer.buf[0..writer.len]).hex_dump());

        (&writer.buf[0..writer.len]).to_vec()
    }

    #[test]
    fn test_raw_parser() {
        setup();
        let mut packet = TestPacketBuffer::new();
        let mut parser = RawPacketParser::new(&mut packet);

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
            assert_eq!(parse_bytes(&mut parser, &test.0), test.1);
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
            assert_eq!(parse_bytes_as_packet(&mut parser, &test.0), test.1);
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
        setup();
        let mut packet = TestPacketBuffer::new();
        let mut parser = RawPacketParser::new(&mut packet);

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
