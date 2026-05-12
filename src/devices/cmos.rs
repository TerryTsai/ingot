//! Minimal CMOS RTC shim on PIO ports 0x70 (index) / 0x71 (data).
//! Linux's `read_persistent_clock` and TSC calibration both poke these
//! ports during boot. Returning 0xFF on the data port would set Status
//! A's "Update In Progress" bit and hang the kernel; this shim reports
//! "RTC running, not updating, 24h binary mode" and zero for everything
//! else so the kernel's epoch fallback path proceeds.

use super::bus::PioDevice;

const PORT_INDEX: u16 = 0;
const PORT_DATA: u16 = 1;

pub(crate) struct Cmos {
    index: u8,
}

impl Cmos {
    pub fn new() -> Self {
        Cmos { index: 0 }
    }

    pub const BASE: u16 = 0x70;
    pub const SIZE: u16 = 2;

    /// Plausible byte for CMOS register `index`. Time fields read as zero
    /// (kernel's epoch fallback handles this); status registers report
    /// "running, not updating, 24h binary mode."
    fn read_register(index: u8) -> u8 {
        match index {
            0x0A => 0x26,
            0x0B => 0x06,
            0x0D => 0x80,
            _ => 0,
        }
    }
}

impl PioDevice for Cmos {
    fn read(&mut self, port_offset: u16, data: &mut [u8]) {
        let value = match port_offset {
            PORT_DATA => Self::read_register(self.index),
            _ => 0xFF,
        };
        for b in data.iter_mut() { *b = value; }
    }

    fn write(&mut self, port_offset: u16, data: &[u8]) {
        if data.is_empty() { return; }
        if port_offset == PORT_INDEX { self.index = data[0] & 0x7F; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_register_a_reports_not_updating() {
        let mut c = Cmos::new();
        c.write(PORT_INDEX, &[0x0A]);
        let mut buf = [0u8; 1];
        c.read(PORT_DATA, &mut buf);
        assert_eq!(buf[0], 0x26);
    }

    #[test]
    fn status_register_b_reports_24h_binary() {
        let mut c = Cmos::new();
        c.write(PORT_INDEX, &[0x0B]);
        let mut buf = [0u8; 1];
        c.read(PORT_DATA, &mut buf);
        assert_eq!(buf[0], 0x06);
    }

    #[test]
    fn unknown_register_reads_zero() {
        let mut c = Cmos::new();
        c.write(PORT_INDEX, &[0x05]);
        let mut buf = [0u8; 1];
        c.read(PORT_DATA, &mut buf);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn index_high_bit_is_masked() {
        let mut c = Cmos::new();
        c.write(PORT_INDEX, &[0x8A]); // top bit set
        let mut buf = [0u8; 1];
        c.read(PORT_DATA, &mut buf);
        assert_eq!(buf[0], 0x26, "should read register 0x0A");
    }
}
