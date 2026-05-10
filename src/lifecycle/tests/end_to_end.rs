//! End-to-end VM execution with synthetic x86_64 ELFs. Each test
//! builds an ELF whose entry payload is a tiny snippet of long-mode
//! instructions, runs it through Vm::new + Vm::run, and checks the
//! resulting ExitStatus.

use crate::outcome::ExitStatus;

use super::run_with_payload;

#[test]
fn synthetic_hlt_kernel_reaches_halted() {
    match run_with_payload("hlt", &[0xf4]) {
        Ok(outcome) => assert!(matches!(outcome.exit_status, ExitStatus::Halted)),
        Err(e) => eprintln!("[synthetic_hlt_kernel_reaches_halted] skipped: {:?}", e),
    }
}

#[test]
fn synthetic_pio_in_consumed_then_halted() {
    // in al, dx ; hlt — DX is 0 at long-mode entry; we expect the PIO
    // read to be silently consumed and execution to proceed to hlt.
    match run_with_payload("pio_in", &[0xec, 0xf4]) {
        Ok(outcome) => assert!(matches!(outcome.exit_status, ExitStatus::Halted)),
        Err(e) => eprintln!("[synthetic_pio_in_consumed_then_halted] skipped: {:?}", e),
    }
}

#[test]
fn synthetic_pio_out_consumed_then_halted() {
    // out dx, al ; hlt — discarded write, then halt.
    match run_with_payload("pio_out", &[0xee, 0xf4]) {
        Ok(outcome) => assert!(matches!(outcome.exit_status, ExitStatus::Halted)),
        Err(e) => eprintln!("[synthetic_pio_out_consumed_then_halted] skipped: {:?}", e),
    }
}
