//use log::info;

pub type CrcAccum = u16;

const CRC_INIT: CrcAccum = 0xffff;
pub const CRC_GOOD: CrcAccum = 0xf0b8;

#[derive(Debug)]
pub struct Crc {
    val: CrcAccum,
}

impl Default for Crc {
    fn default() -> Self {
        Self { val: CRC_INIT }
    }
}

impl Crc {
    pub fn new() -> Self {
        //info!("CRC new");
        Default::default()
    }

    pub fn accum(&mut self, byte: u8) {
        //info!("CRC accum 0x{:02x}", byte);
        let byte = byte ^ ((self.val & 0xff) as u8);
        let byte = byte ^ (byte << 4);
        let byte16 = byte as u16;
        self.val = ((byte16 << 8) | ((self.val >> 8) & 0x00ff)) ^ (byte16 >> 4) ^ (byte16 << 3);
    }

    pub fn accum_bytes(&mut self, bytes: &[u8]) -> CrcAccum {
        for byte in bytes.iter() {
            self.accum(*byte);
        }
        self.val
    }

    pub fn accum_crc(&mut self) -> CrcAccum {
        let crc = !self.val;
        self.accum((crc & 0xff) as u8);
        self.accum(((crc >> 8) & 0xff) as u8);

        self.val
    }

    pub fn reset(&mut self) {
        //info!("CRC init");
        self.val = CRC_INIT;
    }

    pub fn crc(&self) -> CrcAccum {
        self.val
    }

    pub fn lsb(&self) -> u8 {
        (!self.val & 0x00ff) as u8
    }

    pub fn msb(&self) -> u8 {
        ((!self.val >> 8) & 0x00ff) as u8
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test0() {
        use crate::Crc;
        let mut crc = Crc::new();
        crc.accum(0xc0);
        assert_eq!(!crc.val, 0x3674);
        assert_eq!(crc.accum_crc(), crate::crc::CRC_GOOD);
    }
    #[test]
    fn test1() {
        use crate::Crc;
        let mut crc = Crc::new();
        crc.accum(0xc0);
        crc.accum(0x11);
        crc.accum(0x22);
        crc.accum(0x33);
        assert_eq!(!crc.val, 0x0bd5);
        assert_eq!(crc.accum_crc(), crate::crc::CRC_GOOD);
    }
    #[test]
    fn test2() {
        use crate::Crc;
        let mut crc = Crc::new();
        crc.accum(0x7d);
        assert_eq!(!crc.val, 0x581a);
        assert_eq!(crc.accum_crc(), crate::crc::CRC_GOOD);
    }
}
