//! Byte assembler over guest memory. Tracks an origin (the current
//! section's base address) and a cursor (next emit position).
//!
//! `db/dw/dd/dq` emit at the cursor and advance — for dense layouts
//! (cmdline, MP table). `b/w/d/q` poke at an offset from the origin
//! without moving the cursor — for sparse layouts (boot_params, page
//! tables) and back-patches. `here()` returns the cursor's absolute
//! position; snapshot it like a NASM label to bound a checksum range
//! or compute a back-patch offset.
//!
//! All writes panic on bounds violation. The panic message identifies
//! the bug: `layout::*` constants and `Vm::validate`'s memory floor
//! together determine that every emit lands inside guest memory. A
//! panic here means one of those invariants is wrong.

use std::ops::Range;

use crate::lifecycle::memory::Memory;

pub(crate) struct Asm<'m> {
    mem: &'m mut Memory,
    org: u64,
    here: u64,
}

impl<'m> Asm<'m> {
    pub fn new(mem: &'m mut Memory) -> Self {
        Self { mem, org: 0, here: 0 }
    }

    /// Begin a new section at `addr`. Sets origin (for offset pokes) and
    /// resets the cursor (for sequential emits). Like NASM's `.org`.
    pub fn org(&mut self, addr: u64) {
        self.org = addr;
        self.here = addr;
    }

    /// Absolute cursor position. Snapshot it like a NASM label.
    pub fn here(&self) -> u64 {
        self.here
    }

    // ── emit at cursor, advance ────────────────────── NASM-style db/dw/dd/dq
    pub fn db(&mut self, v: u8)         { self.emit(&[v]); }
    pub fn dw(&mut self, v: u16)        { self.emit(&v.to_le_bytes()); }
    pub fn dd(&mut self, v: u32)        { self.emit(&v.to_le_bytes()); }
    pub fn dq(&mut self, v: u64)        { self.emit(&v.to_le_bytes()); }
    pub fn raw(&mut self, bytes: &[u8]) { self.emit(bytes); }

    // ── poke at offset from origin, cursor unchanged ─────────── our addition
    pub fn b(&mut self, off: u64, v: u8)  { self.poke(self.org + off, &[v]); }
    pub fn w(&mut self, off: u64, v: u16) { self.poke(self.org + off, &v.to_le_bytes()); }
    pub fn d(&mut self, off: u64, v: u32) { self.poke(self.org + off, &v.to_le_bytes()); }
    pub fn q(&mut self, off: u64, v: u64) { self.poke(self.org + off, &v.to_le_bytes()); }

    /// Sum bytes over the absolute range, return the byte that makes them
    /// sum to 0 mod 256. Used for MP-table and ACPI-style 8-bit checksums.
    pub fn cksum(&self, range: Range<u64>) -> u8 {
        let mut s: u32 = 0;
        let mut buf = [0u8; 64];
        let (mut off, end) = (range.start, range.end);
        while off < end {
            let n = (end - off).min(64) as usize;
            self.mem.read(off, &mut buf[..n]).expect("asm read out of bounds — fix layout::*");
            s += buf[..n].iter().map(|&b| b as u32).sum::<u32>();
            off += n as u64;
        }
        (256u32.wrapping_sub(s) & 0xff) as u8
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.poke(self.here, bytes);
        self.here += bytes.len() as u64;
    }

    fn poke(&mut self, addr: u64, bytes: &[u8]) {
        self.mem.write(addr, bytes).expect("asm write out of bounds — fix layout::*");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Memory { Memory::new(0x10_0000).expect("mem") }

    #[test]
    fn emit_writes_le_at_cursor_and_advances() {
        let mut m = mem();
        let mut a = Asm::new(&mut m);
        a.org(0x1000);
        a.db(0x12);
        a.dw(0xABCD);
        a.dd(0xDEADBEEF);
        a.dq(0x0123_4567_89AB_CDEF);
        assert_eq!(a.here(), 0x1000 + 1 + 2 + 4 + 8);
        let mut buf = [0u8; 15];
        m.read(0x1000, &mut buf).unwrap();
        assert_eq!(&buf, &[
            0x12,
            0xCD, 0xAB,
            0xEF, 0xBE, 0xAD, 0xDE,
            0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01,
        ]);
    }

    #[test]
    fn poke_writes_at_origin_offset_without_advancing() {
        let mut m = mem();
        let mut a = Asm::new(&mut m);
        a.org(0x2000);
        let start = a.here();
        a.b(0x100, 0xAA);
        a.w(0x200, 0xBBCC);
        a.d(0x300, 0xDDEE_FF00);
        a.q(0x400, 0x1111_2222_3333_4444);
        assert_eq!(a.here(), start, "poke does not move cursor");
        let mut b = [0u8; 1]; m.read(0x2100, &mut b).unwrap(); assert_eq!(b[0], 0xAA);
        let mut w = [0u8; 2]; m.read(0x2200, &mut w).unwrap(); assert_eq!(u16::from_le_bytes(w), 0xBBCC);
        let mut d = [0u8; 4]; m.read(0x2300, &mut d).unwrap(); assert_eq!(u32::from_le_bytes(d), 0xDDEE_FF00);
        let mut q = [0u8; 8]; m.read(0x2400, &mut q).unwrap(); assert_eq!(u64::from_le_bytes(q), 0x1111_2222_3333_4444);
    }

    #[test]
    fn org_resets_cursor_to_new_section() {
        let mut m = mem();
        let mut a = Asm::new(&mut m);
        a.org(0x3000); a.db(0); a.db(0); a.db(0);
        assert_eq!(a.here(), 0x3003);
        a.org(0x4000);
        assert_eq!(a.here(), 0x4000);
    }

    #[test]
    fn cksum_makes_range_sum_to_zero_mod_256() {
        let mut m = mem();
        let mut a = Asm::new(&mut m);
        a.org(0x5000);
        a.raw(&[0x10, 0x20, 0x30, 0x40, 0x50]); // sums to 0xF0
        let c = a.cksum(0x5000..0x5005);
        assert_eq!(c, 0x10);
        a.db(c);
        let total: u32 = (0x5000..0x5006).map(|addr| {
            let mut b = [0u8; 1]; m.read(addr, &mut b).unwrap(); b[0] as u32
        }).sum();
        assert_eq!(total % 256, 0);
    }

    #[test]
    fn cksum_chunked_range_matches_full_sum() {
        // exercises the 64-byte chunk loop
        let mut m = mem();
        let mut a = Asm::new(&mut m);
        a.org(0x6000);
        for i in 0..200u32 { a.db((i & 0xff) as u8); }
        let c = a.cksum(0x6000..0x6000 + 200);
        let expected: u32 = (0..200u32).map(|i| i & 0xff).sum();
        assert_eq!(c, ((256u32.wrapping_sub(expected)) & 0xff) as u8);
    }

    // ── property tests ─────────────────────────────────────────────────
    //
    // Asm's contract: any sequence of `org`/emit/poke that stays within
    // guest memory bounds completes without panic, and `cksum` of a
    // written range always returns the byte that makes the range sum to
    // 0 mod 256.

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 200, .. ProptestConfig::default() })]

        /// Cursor advances by exactly the number of bytes emitted.
        #[test]
        fn cursor_advances_by_emit_width(
            ops in prop::collection::vec(0u8..4, 1..40),  // 0=db, 1=dw, 2=dd, 3=dq
        ) {
            let mut m = mem();
            let mut a = Asm::new(&mut m);
            a.org(0x1000);
            let mut expected = 0x1000u64;
            for op in ops {
                match op {
                    0 => { a.db(0xAA); expected += 1; }
                    1 => { a.dw(0xBBCC); expected += 2; }
                    2 => { a.dd(0xDD); expected += 4; }
                    _ => { a.dq(0xEE); expected += 8; }
                }
                prop_assert_eq!(a.here(), expected);
            }
        }

        /// Poke at offset doesn't move the cursor.
        #[test]
        fn poke_never_moves_cursor(
            pokes in prop::collection::vec((0u64..0x100, any::<u8>()), 1..20),
        ) {
            let mut m = mem();
            let mut a = Asm::new(&mut m);
            a.org(0x2000);
            let cursor = a.here();
            for (off, v) in pokes {
                a.b(off, v);
                prop_assert_eq!(a.here(), cursor);
            }
        }

        /// Checksum of a range always makes that range plus the checksum
        /// byte sum to 0 mod 256, for any byte pattern.
        #[test]
        fn cksum_balances_any_byte_pattern(
            bytes in prop::collection::vec(any::<u8>(), 1..512),
        ) {
            let mut m = mem();
            let mut a = Asm::new(&mut m);
            a.org(0x3000);
            a.raw(&bytes);
            let c = a.cksum(0x3000..0x3000 + bytes.len() as u64);
            let sum_with_check: u32 = bytes.iter().map(|&b| b as u32).sum::<u32>() + c as u32;
            prop_assert_eq!(sum_with_check % 256, 0);
        }

        /// org resets cursor to the given address; subsequent here()
        /// returns exactly that address.
        #[test]
        fn org_resets_cursor_to_arg(addr in 0u64..0x80_000) {
            let mut m = mem();
            let mut a = Asm::new(&mut m);
            a.org(addr);
            prop_assert_eq!(a.here(), addr);
        }
    }
}
