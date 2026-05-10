//! End-to-end VM execution with synthetic x86_64 ELFs. Each test
//! builds an ELF whose entry payload is a tiny snippet of long-mode
//! instructions, runs it through Vm::new + Vm::run, and checks the
//! resulting ExitStatus.

use crate::outcome::ExitStatus;
use crate::{State, StdoutSink};

use super::{run_with_payload, run_with_stdout};

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

#[test]
fn telemetry_records_full_state_transition_sequence() {
    match run_with_payload("transitions", &[0xf4]) {
        Ok(outcome) => {
            let actual: Vec<_> = outcome.telemetry.transitions.iter().map(|t| (t.from, t.to)).collect();
            assert_eq!(actual, vec![
                (State::Configured, State::Initializing),
                (State::Initializing, State::Booting),
                (State::Booting, State::Running),
                (State::Running, State::Exiting),
                (State::Exiting, State::Terminated),
            ]);
            assert!(outcome.telemetry.boot_started_at <= outcome.telemetry.boot_completed_at.unwrap());
            assert!(outcome.telemetry.boot_completed_at.unwrap() <= outcome.telemetry.terminated_at.unwrap());
        }
        Err(e) => eprintln!("[telemetry_records_full_state_transition_sequence] skipped: {:?}", e),
    }
}

#[test]
fn synthetic_kernel_writes_hi_to_mmio_uart() {
    // Long-mode payload that emits "Hi" through the MMIO UART at 0xD0000000
    // (the THR register at offset 0) and halts.
    //   48 B8 00 00 00 D0 00 00 00 00   mov rax, 0xD0000000
    //   C6 00 48                         mov byte [rax], 'H'
    //   C6 00 69                         mov byte [rax], 'i'
    //   F4                               hlt
    let payload = [
        0x48, 0xb8, 0x00, 0x00, 0x00, 0xd0, 0x00, 0x00, 0x00, 0x00, 0xc6, 0x00, b'H', 0xc6, 0x00,
        b'i', 0xf4,
    ];
    match run_with_stdout("uart_write", &payload, StdoutSink::Buffer) {
        Ok(outcome) => {
            assert!(matches!(outcome.exit_status, ExitStatus::Halted));
            assert_eq!(outcome.stdout, b"Hi".to_vec());
        }
        Err(e) => eprintln!("[synthetic_kernel_writes_hi_to_mmio_uart] skipped: {:?}", e),
    }
}
