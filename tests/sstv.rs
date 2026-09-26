//! Builds and runs `src/sstv.rs` (mode table, FSK ID, TX->RX round-trips for
//! every mode) as a standalone module.
//!
//! hpsdr-rs is a binary-only crate, so integration tests can't reach its
//! modules through a library path; include the file directly instead. This
//! keeps the modem testable before it is declared in main.rs.

#[allow(dead_code)]
#[path = "../src/sstv.rs"]
mod sstv;
