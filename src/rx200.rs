//! Background UDP listener for a DL1BZ-style "RX200" homebrew SWR/power
//! meter -- a separate physical device (not part of this radio, not part
//! of the openHPSDR protocol) that broadcasts its forward/reflected power
//! readings as JSON over plain UDP, same port/payload deskHPSDR's own
//! `rx200_udp_listener` (src/rigctl.c) listens for:
//!
//! ```json
//! {"pwrfwd": "12.3", "pwrref": "0.4", "time": "..."}
//! ```
//!
//! Purely read-only telemetry, entirely independent of this radio's own
//! internal TX forward/reverse power meter (RadioSession::tx_forward_power/
//! tx_reverse_power) -- it's a second, external opinion on SWR, not a
//! replacement, and never feeds into any TX protection logic here.

use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// UDP broadcast port the RX200 device sends on -- fixed by the device's
/// own firmware, matches deskHPSDR's `rx200_port`/`rx200_udp_port`.
const RX200_PORT: u16 = 5573;

/// A reading is shown as "RX200 offline" once it's older than this --
/// same 10s idea as deskHPSDR's own SO_RCVTIMEO, generous enough to
/// survive one dropped broadcast without flickering.
const STALE_AFTER: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct Rx200Reading {
    pub fwd_watts: f64,
    pub ref_watts: f64,
    /// Recomputed locally from fwd/ref, same as deskHPSDR's hardened
    /// listener -- NOT trusted from the device's own "swr" field (an
    /// earlier deskHPSDR version did, and it was the source of a real
    /// "shows incorrect SWR" bug there).
    pub swr: f64,
    pub device_time: String,
}

/// Owns the background listener thread -- started once (see
/// ConnectedState::rx200's doc comment) and stopped cleanly on Drop.
pub struct Rx200Monitor {
    latest: Arc<Mutex<Option<(Rx200Reading, Instant)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Rx200Monitor {
    pub fn start() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_latest = Arc::clone(&latest);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || listener_loop(thread_latest, thread_stop));
        Self { latest, stop, thread: Some(thread) }
    }

    /// Most recent reading, or None if the device hasn't been heard
    /// from (never, or not within STALE_AFTER) -- callers show an
    /// "RX200 offline" placeholder in the None case, same as
    /// deskHPSDR's rx200_valid gate.
    pub fn latest(&self) -> Option<Rx200Reading> {
        let guard = self.latest.lock().unwrap();
        guard.as_ref().and_then(|(reading, at)| {
            if at.elapsed() < STALE_AFTER {
                Some(reading.clone())
            } else {
                None
            }
        })
    }
}

impl Drop for Rx200Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Same "wake the blocking recv" trick as deskHPSDR's hardened
        // listener -- without it, drop would block for up to the
        // socket's own read timeout waiting for the thread to notice
        // the stop flag.
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            let _ = sock.send_to(b"{}", ("127.0.0.1", RX200_PORT));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn listener_loop(latest: Arc<Mutex<Option<(Rx200Reading, Instant)>>>, stop: Arc<AtomicBool>) {
    let socket = match UdpSocket::bind(("0.0.0.0", RX200_PORT)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rx200: couldn't bind UDP port {RX200_PORT}: {e}");
            return;
        }
    };
    // Short timeout so the stop flag gets checked promptly even with
    // no traffic, rather than blocking in recv_from forever.
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_secs(2))) {
        eprintln!("rx200: couldn't set socket read timeout: {e}");
    }
    let mut buf = [0u8; 1024];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((len, _addr)) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(reading) = parse_packet(&buf[..len]) {
                    *latest.lock().unwrap() = Some((reading, Instant::now()));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => continue,
            Err(_) => continue,
        }
    }
}

/// Accepts fwd/ref power as either a JSON number or a JSON string (the
/// device's own firmware isn't ours to pin down, and deskHPSDR's listener
/// coerces via json-c's get_string regardless of the underlying type) --
/// tolerant field access, same spirit as deskHPSDR's hardened parser.
fn field_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn parse_packet(bytes: &[u8]) -> Option<Rx200Reading> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let fwd_watts = field_f64(&value, "pwrfwd")?;
    let ref_watts = field_f64(&value, "pwrref")?;
    if !fwd_watts.is_finite() || !ref_watts.is_finite() || fwd_watts < 0.0 || ref_watts < 0.0 {
        return None;
    }
    let device_time = match value.get("time") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    // Same VSWR-from-power formula as deskHPSDR's hardened rx200 parser
    // (and, separately, this app's own internal TX meter) -- computed
    // here rather than trusted from the device's own "swr" field.
    let swr = if fwd_watts > 0.0 {
        if ref_watts >= fwd_watts {
            99.9
        } else {
            let rho = (ref_watts / fwd_watts).sqrt();
            (1.0 + rho) / (1.0 - rho)
        }
    } else {
        0.0
    };
    Some(Rx200Reading { fwd_watts, ref_watts, swr, device_time })
}
