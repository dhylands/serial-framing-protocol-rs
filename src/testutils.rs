use core::cmp::{max, min};
use log::{error, info, warn};
use pretty_hex::*;
use simple_logger;
use std::sync::Once;
use std::vec::Vec;

use super::crc::Crc;
use super::rawpacket::{RawPacketParser, RawParseResult};
use super::traits::{PacketBuffer, PacketQueue, PacketWriter, Storage};

static INIT: Once = Once::new();

pub fn setup_log() {
    INIT.call_once(|| {
        simple_logger::init().unwrap();
    });
}

const PACKET_SIZE: usize = 256;

pub struct TestPacketBuffer {
    len: usize,
    buf: [u8; PACKET_SIZE],
}

impl Default for TestPacketBuffer {
    fn default() -> Self {
        TestPacketBuffer {
            len: 0,
            buf: [0; PACKET_SIZE],
        }
    }
}

impl TestPacketBuffer {
    pub fn new() -> Self {
        Default::default()
    }
}

impl PacketBuffer for TestPacketBuffer {
    fn capacity(&self) -> usize {
        PACKET_SIZE
    }

    fn len(&self) -> usize {
        self.len
    }

    fn set_len(&mut self, len: usize) {
        self.len = min(len, PACKET_SIZE);
    }

    fn data(&self) -> &[u8] {
        &self.buf[0..self.len]
    }

    fn data_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..]
    }
}

impl PacketWriter for TestPacketBuffer {
    fn start_write(&mut self) {
        //info!("start_write");
        self.reset();
    }

    fn write_byte(&mut self, byte: u8) {
        //info!("write_byte 0x{:02x} self.len = {}", byte, self.len());
        self.append(byte).unwrap();
    }

    fn end_write(&mut self) {
        //info!("end_write");
    }
}

const QUEUE_SIZE: usize = 8;
pub struct TestPacketQueue {
    len: usize,
    idx: usize,
    packet: [TestPacketBuffer; QUEUE_SIZE],
}

impl PacketQueue for TestPacketQueue {
    /// Returns the maximum number of packets which can be stored.
    fn capacity(&self) -> usize {
        QUEUE_SIZE
    }

    /// Returns the number of packets currently in the queue.
    fn len(&self) -> usize {
        self.len
    }

    /// Sets the number of packets currently in the queue.
    fn set_len(&mut self, len: usize) {
        self.len = min(len, QUEUE_SIZE);
        self.idx = if self.len > 0 {
            min(self.idx, self.len - 1)
        } else {
            0
        }
    }

    /// Returns the index of the most recently added packet to the queue.
    fn idx(&self) -> usize {
        self.idx
    }

    /// Sets the index of the nmost recently added packet to the queue.
    fn set_idx(&mut self, idx: usize) {
        self.idx = min(idx, QUEUE_SIZE - 1);
        self.len = max(self.len, self.idx + 1);
    }

    /// Returns the i'th packet from the queue.
    fn packet(&mut self, idx: usize) -> Option<&mut dyn PacketBuffer> {
        if idx < self.len {
            Some(&mut self.packet[idx])
        } else {
            None
        }
    }
}

impl TestPacketQueue {
    fn new() -> Self {
        TestPacketQueue {
            len: 0,
            idx: 0,
            packet: Default::default(),
        }
    }
}

// Parse a bunch of bytes and return the first return code that isn't
// MoreDataNeeded. This means that this function will parse at most one
// error or packet from the input stream, which is fine for testing.
pub fn parse_bytes(
    parser: &mut RawPacketParser,
    bytes: &[u8],
    rx_packet: &mut dyn PacketBuffer,
) -> RawParseResult {
    for byte in bytes {
        let parse_result = parser.parse_byte(*byte, rx_packet);
        match parse_result {
            RawParseResult::RawPacketReceived(header) => {
                info!(
                    "Header = {:02x} data = {:?}",
                    header,
                    rx_packet.data().hex_dump()
                );
                return RawParseResult::RawPacketReceived(header);
            }

            RawParseResult::MoreDataNeeded => {
                continue;
            }

            RawParseResult::CrcError(rcvd_crc) => {
                let mut crc = Crc::new();
                crc.accum(parser.header());
                let expected_crc = !crc.accum_bytes(rx_packet.data());
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

pub fn parse_bytes_as_packet(
    parser: &mut RawPacketParser,
    bytes: &[u8],
    rx_packet: &mut dyn PacketBuffer,
) -> Vec<u8> {
    let parse_result = parse_bytes(parser, bytes, rx_packet);
    match parse_result {
        RawParseResult::RawPacketReceived(header) => {
            let mut vec = Vec::new();
            vec.push(header);
            vec.extend_from_slice(rx_packet.data());
            return vec;
        }
        _ => {
            error!("{:?}", parse_result);
            return Vec::new();
        }
    }
}

pub struct TestStorage {
    rx_buf: TestPacketBuffer,
    tx_buf: TestPacketBuffer,
    tx_queue: TestPacketQueue,
}
impl Storage for TestStorage {
    /// Returns a reference to Rx PacketBuffer
    fn rx_buf(&mut self) -> &mut dyn PacketBuffer {
        &mut self.rx_buf
    }

    /// Returns a reference to the PacketWriter
    fn tx_writer(&mut self) -> &mut dyn PacketWriter {
        &mut self.tx_buf
    }

    /// Returns a reference to the PacketQueue
    fn tx_queue(&mut self) -> &mut dyn PacketQueue {
        &mut self.tx_queue
    }
}

// A few methods to help out with testing.
impl TestStorage {
    pub fn new() -> Self {
        TestStorage {
            rx_buf: TestPacketBuffer::new(),
            tx_buf: TestPacketBuffer::new(),
            tx_queue: TestPacketQueue::new(),
        }
    }

    pub fn rx_data(&self) -> &[u8] {
        self.rx_buf.data()
    }

    pub fn tx_data(&self) -> &[u8] {
        self.tx_buf.data()
    }

    pub fn tx_vec(&self) -> Vec<u8> {
        self.tx_data().to_vec()
    }
}
