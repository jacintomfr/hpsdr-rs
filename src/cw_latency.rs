//! CW latency diagnostics: timestamps taken at the key points of the CW paths, logged as `cw:` lines in /tmp/hpsdr_perf.log while
//! /tmp/hpsdr_diag.enable exists (the same switch as the other diagnostics). With the file absent nothing is stored and the only cost is one
//! relaxed atomic load at each call site.
//!
//! What is measured (all with the process's monotonic clock):
//! * `ptt->mox`      radio PTT/key edge seen in a packet -> the UI's break-in mirror has called `set_mox` (rise and fall)
//! * `key->sidetone` paddle/PTT edge seen -> first PC-sidetone sample queued, then `queued->audible` (estimate with the output's own
//!                   reported buffer latency)
//! * `midi->ui`      MIDI event in the driver callback -> handled in the UI frame
//! * `send->chunk`   a CW text send (button, MIDI macro, CAT KY) -> first IQ chunk produced by the TX thread
//! * `send->wire`    the same send -> first IQ handed to the radio (P1: popped for the packet; P2: first real DUC packet sent)
//! * `end->mox off`  last CW element generated -> `set_mox(false)`
//!
//! Not measurable here: key -> RF (HL2 gateware, its TX-latency FIFO). That needs an external receiver.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

const DIAG_FLAG: &str = "/tmp/hpsdr_diag.enable";
const LOG_FILE: &str = "/tmp/hpsdr_perf.log";
/// An edge older than this is treated as unrelated to the event being measured.
const STALE_NS: u64 = 1_000_000_000;

static T0: OnceLock<Instant> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static ENABLED_CHECKED_NS: AtomicU64 = AtomicU64::new(0);

static PTT_EDGE_NS: AtomicU64 = AtomicU64::new(0);
static PTT_EDGE_ON: AtomicBool = AtomicBool::new(false);
static PTT_EDGE_PENDING: AtomicBool = AtomicBool::new(false);
static PADDLE_EDGE_NS: AtomicU64 = AtomicU64::new(0);
static SIDETONE_QUEUED_NS: AtomicU64 = AtomicU64::new(0);
static SIDETONE_PENDING: AtomicBool = AtomicBool::new(false);
static MIDI_NS: AtomicU64 = AtomicU64::new(0);
static SEND_NS: AtomicU64 = AtomicU64::new(0);
static CHUNK_PENDING: AtomicBool = AtomicBool::new(false);
static WIRE_PENDING: AtomicBool = AtomicBool::new(false);
static END_NS: AtomicU64 = AtomicU64::new(0);

fn now_ns() -> u64 {
    T0.get_or_init(Instant::now).elapsed().as_nanos() as u64 + 1
}

/// Whether the diagnostics switch file exists (re-checked at most once a second).
pub fn enabled() -> bool {
    let now = now_ns();
    let last = ENABLED_CHECKED_NS.load(Ordering::Relaxed);
    if last == 0 || now.saturating_sub(last) > 1_000_000_000 {
        ENABLED_CHECKED_NS.store(now, Ordering::Relaxed);
        ENABLED.store(std::path::Path::new(DIAG_FLAG).exists(), Ordering::Relaxed);
    }
    ENABLED.load(Ordering::Relaxed)
}

fn log(what: &str, ns: u64) {
    // One write per line: several threads log, and separate writes would interleave.
    let line = format!("cw: [{:9.3}] {what} {:.1} ms
", now_ns() as f64 / 1e9, ns as f64 / 1e6);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn since(mark: &AtomicU64) -> Option<u64> {
    let m = mark.load(Ordering::Relaxed);
    if m == 0 {
        return None;
    }
    let d = now_ns().saturating_sub(m);
    (d < STALE_NS).then_some(d)
}

/// Radio thread: the radio's PTT/keyed bit changed.
pub fn ptt_edge(on: bool) {
    if !enabled() {
        return;
    }
    PTT_EDGE_NS.store(now_ns(), Ordering::Relaxed);
    PTT_EDGE_ON.store(on, Ordering::Relaxed);
    PTT_EDGE_PENDING.store(true, Ordering::Relaxed);
}

/// Radio thread: the paddle contact bits changed (`contacts` != 0 means at least one paddle closed).
pub fn paddle_edge(contacts: u8) {
    if contacts != 0 && enabled() {
        PADDLE_EDGE_NS.store(now_ns(), Ordering::Relaxed);
    }
}

/// UI frame: the break-in mirror just called `set_mox(on)`.
pub fn mirror_set_mox(on: bool) {
    if PTT_EDGE_PENDING.load(Ordering::Relaxed) && PTT_EDGE_ON.load(Ordering::Relaxed) == on {
        PTT_EDGE_PENDING.store(false, Ordering::Relaxed);
        if let Some(d) = since(&PTT_EDGE_NS) {
            log(if on { "ptt->mox (rise)" } else { "ptt->mox (fall)" }, d);
        }
    }
}

/// Sidetone thread: the first sample of a new element was just queued.
pub fn sidetone_queued() {
    if !enabled() {
        return;
    }
    let ptt = since(&PTT_EDGE_NS);
    let paddle = since(&PADDLE_EDGE_NS);
    match (ptt, paddle) {
        (None, None) => {}
        (a, b) => log("key->sidetone queued", a.unwrap_or(u64::MAX).min(b.unwrap_or(u64::MAX))),
    }
    SIDETONE_QUEUED_NS.store(now_ns(), Ordering::Relaxed);
    SIDETONE_PENDING.store(true, Ordering::Relaxed);
}

/// Sidetone output callback: a non-silent sample is being delivered; `dev_buffer_ns` is the output's own reported playback delay and
/// `frames_in` how many frames into this callback's buffer the sample sits.
pub fn sidetone_audible(dev_buffer_ns: u64, frames_in: usize) {
    if SIDETONE_PENDING.swap(false, Ordering::Relaxed) {
        let queued = SIDETONE_QUEUED_NS.load(Ordering::Relaxed);
        let est = now_ns().saturating_sub(queued) + dev_buffer_ns + (frames_in as u64) * 1_000_000_000 / 48_000;
        log(&format!("sidetone queued->audible (device buffer {:.1} ms)", dev_buffer_ns as f64 / 1e6), est);
    }
}

/// MIDI driver callback thread.
pub fn midi_event() {
    if enabled() {
        MIDI_NS.store(now_ns(), Ordering::Relaxed);
    }
}

/// UI frame: queued MIDI events are being handled.
pub fn midi_handled() {
    let m = MIDI_NS.swap(0, Ordering::Relaxed);
    if m != 0 {
        let d = now_ns().saturating_sub(m);
        if d < STALE_NS {
            log("midi->ui", d);
        }
    }
}

/// A CW text send was just started (button, MIDI macro, CAT KY, rigctl).
pub fn send_started() {
    if !enabled() {
        return;
    }
    if let Some(d) = since(&MIDI_NS) {
        log("midi->send", d);
    }
    SEND_NS.store(now_ns(), Ordering::Relaxed);
    CHUNK_PENDING.store(true, Ordering::Relaxed);
    WIRE_PENDING.store(true, Ordering::Relaxed);
}

/// TX thread: the first CW IQ chunk has been produced.
pub fn chunk_pushed() {
    if CHUNK_PENDING.swap(false, Ordering::Relaxed) {
        if let Some(d) = since(&SEND_NS) {
            log("send->first chunk", d);
        }
    }
}

/// Radio sender: IQ for the radio is going out now (`what` names the protocol path).
pub fn iq_on_wire(what: &str) {
    if WIRE_PENDING.swap(false, Ordering::Relaxed) {
        if let Some(d) = since(&SEND_NS) {
            log(&format!("send->wire ({what})"), d);
        }
    }
}

/// Cheap pre-check for hot paths.
#[inline]
pub fn wire_pending() -> bool {
    WIRE_PENDING.load(Ordering::Relaxed)
}

/// TX thread: the generator has produced its last element (`busy` just went false).
pub fn text_ended() {
    if enabled() {
        END_NS.store(now_ns(), Ordering::Relaxed);
    }
}

/// UI frame: `set_mox(false)` after a text send finished.
pub fn text_mox_off() {
    let m = END_NS.swap(0, Ordering::Relaxed);
    if m != 0 {
        log("last element->mox off", now_ns().saturating_sub(m));
    }
}

/// Cheap pre-check for the audio callback.
#[inline]
pub fn sidetone_pending() -> bool {
    SIDETONE_PENDING.load(Ordering::Relaxed)
}

/// MIDI thread: a CW key event from the driver callback reached the host keyer (the time since the driver callback).
pub fn midi_to_keyer() {
    if let Some(d) = since(&MIDI_NS) {
        if enabled() {
            log("midi->keyer", d);
        }
    }
}
