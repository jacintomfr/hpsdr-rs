//! Builds and runs `src/rtty.rs` (and its unit tests: Baudot table and
//! TX->RX round-trips) as a standalone module.
//!
//! hpsdr-rs is a binary-only crate, so integration tests can't reach its
//! modules through a library path; include the file directly instead. This
//! keeps the modem testable before it is declared in main.rs.

#[allow(dead_code)]
#[path = "../src/rtty.rs"]
mod rtty;
