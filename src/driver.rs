use generic_array::ArrayLength;

pub trait Driver {
    /// Maximum size of a packet sent or received (doesn't include framing or escape bytes).
    type PACKET_SIZE: ArrayLength<u8>;

    /// Called at the beginning of writing a packet. Allows the driver implementation to implement
    /// buffering.
    fn start_write(&mut self) {}

    /// Called to write some data (not necessarily a complete packet) to the hardware.
    fn write_byte(&mut self, byte: u8);

    /// Called at the end of the writing a packet. Allows the driver to flush a
    /// buffer if a buffered implementation is used.
    fn end_write(&mut self) {}
}
