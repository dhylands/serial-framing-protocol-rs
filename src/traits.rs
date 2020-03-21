use core::fmt;
use core::mem::size_of;
use log::info;
use pretty_hex::*;

use crate::crc::{Crc, CrcAccum};

pub const SOF: u8 = 0x7e; // Start of Frame
pub const ESC: u8 = 0x7d;
pub const ESC_FLIP: u8 = 0x20;

pub trait PacketBuffer {
    /// Returns the number of bytes which have currently been accumulated in
    /// the packet buffer.
    fn len(&self) -> usize;

    /// Sets the length of the data found in the buffer.
    fn set_len(&mut self, len: usize);

    /// Returns a slice containing valid packet data.
    fn data(&self) -> &[u8];

    /// Returns the capacity of the packet buffer.
    fn capacity(&self) -> usize;

    /// Stores a byte into the buffer.
    fn store(&mut self, idx: usize, byte: u8);

    /// Determines if the current buffer is currently empty or not.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resets the packet buffer to start collecting a new set of bytes.
    fn reset(&mut self) {
        self.set_len(0);
    }

    /// Appends a byte to the end of the packet buffer. This function will
    /// return an error result if the packet buffer is full.
    fn append(&mut self, byte: u8) -> Result<(), ()> {
        let len = self.len();
        if len < self.capacity() {
            self.set_len(len + 1);
            self.store(len, byte);
            Ok(())
        } else {
            Err(())
        }
    }

    /// Removes the CRC from the packet buffer. The CRC is assumed to be a
    /// 16-bit CRC stored with LSB first (i.e. LSB is at a lower memory
    /// location that the MSB)
    fn remove_crc(&mut self) -> CrcAccum {
        let mut len = self.len();
        if len < size_of::<CrcAccum>() {
            return 0;
        }

        // LSB is transmitted first
        len -= 2;
        let data = self.data();
        let crc = ((data[len + 1] as CrcAccum) << 8) | (data[len] as CrcAccum);
        self.set_len(len);
        crc
    }

    /// Dumps the contents of a packet buffer in a nice hexadecimal format.
    fn dump(&self) {
        info!("{:?}", self.data().hex_dump());
    }
}

impl fmt::Debug for dyn PacketBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.data().hex_dump())
    }
}

pub trait PacketWriter {
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

/// The PacketQueue is used to store the N most recent packets which have
/// been sent.
pub trait PacketQueue {
    /// Returns the maximum number of packets which can be stored.
    fn capacity(&self) -> usize;

    /// Returns the number of packets currently in the queue.
    fn len(&self) -> usize;

    /// Removes all packets from the queue.
    fn clear(&mut self);

    /// Returns a reference to the next packet to send.
    fn next(&mut self) -> &mut dyn PacketBuffer;

    /// Returns the offset'th most recent packet. Passing in 0 returns the most
    /// recent, passing in 1 returns the packet before that, etc.
    fn get(&self, offset: usize) -> Option<&dyn PacketBuffer>;
}

pub trait Storage {
    /// Returns a reference to Rx PacketBuffer
    fn rx_buf(&mut self) -> &mut dyn PacketBuffer;

    /// Returns a reference to the PacketWriter
    fn tx_writer(&mut self) -> &mut dyn PacketWriter;

    /// Returns a reference to the PacketQueue
    fn tx_queue(&mut self) -> &mut dyn PacketQueue;
}
