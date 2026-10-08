//! Host-side CW keying driven by MIDI (a MIDI CW interface: paddles, a straight key, or an external keyer with its own PTT).
//!
//! Ported from piHPSDR (actions.c `CW_LEFT`/`CW_RIGHT`/`CW_KEYER_KEYDOWN`/`CW_KEYER_PTT`, iambic.c, transmitter.c's CW event ring and
//! `next_cw_sidetone_sample`) with deskHPSDR's `CW_STRAIGHT_KEY` added. Hierarchy, as in the references:
//!
//! 1. A MIDI key event is handled on the MIDI driver's own thread (`midi_event`), never through the UI frame.
//! 2. Paddles (`CwLeft`/`CwRight`) wake the iambic keyer thread (`keyer_thread`, a 1 ms state machine that raises MOX for break-in, waits for
//!    the TX thread, and drops MOX after the hang time). They only work with "CW handled in Radio" OFF (piHPSDR: the keyer thread only
//!    runs with `cw_keyer_internal == 0`).
//! 3. Every key-down/up becomes an event `(state, wait)` in a ring: `wait` is the time, in 1/48000 s, since the previous event. The TX
//!    thread (`HostKeyGen`) plays the ring sample by sample, so the key timing is reproduced exactly, delayed by a constant.
//! 4. `CwKeyerKeydown` is a hard key up/down WITHOUT break-in, for an external keyer that has its own PTT (`CwKeyerPtt`); it works with
//!    "CW handled in Radio" ON too, while `CwKeyerPtt` is held (piHPSDR's `MIDI_cw_is_active`).
//! 5. The local (PC) sidetone is keyed directly from the same key state (piHPSDR's delay-free `gpio_set_cw` path), not from the TX chunks.
//! 6. With the host keyer in charge, `host_mode()` is true and main.rs disarms the radio's own internal keyer (CW enable bit off).
//!
//! Not ported: "Keys reversed" and "Enforce letter spacing" (this program has no such settings), CAT CW abort on key hit, the radio's
//! own paddle contacts as a host keyer source.

use crate::midi::{MidiAction, MidiBinding, MidiEventKind, RawMidiEvent};
use crate::radio::{CwKeyerAtomics, CW_KEYER_MODE_IAMBIC_A, CW_KEYER_MODE_STRAIGHT};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Events older than this are dropped when TX is idle (a stale backlog must never replay in a later transmission).
const STALE_EVENTS: Duration = Duration::from_millis(500);
/// A pause longer than this (samples at 48 kHz, 0.5 s) restarts the event timing: the next key-down plays without delay.
const LONG_PAUSE_SAMPLES: i64 = 24_000;
/// Key-down for longer than this (samples at 48 kHz, 20 s) is released by the generator (hardware protection, as piHPSDR).
const MAX_KEYDOWN_SAMPLES: u64 = 960_000;
const SAMPLE_NS: f64 = 1e9 / 48_000.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CwAct {
    Left,
    Right,
    StraightKey,
    KeyerKeydown,
    KeyerPtt,
}

#[derive(Clone, PartialEq, Eq)]
struct CwBinding {
    event: MidiEventKind,
    channel: Option<u8>,
    number: u8,
    act: CwAct,
}

struct Attached {
    mox: Arc<AtomicBool>,
    keyer: Arc<CwKeyerAtomics>,
    /// The radio's own PTT readback (hpsdr_ptt): keeps MOX up after `CwKeyerPtt` is released while the radio itself holds PTT.
    radio_ptt: Arc<AtomicBool>,
}

struct Host {
    /// "CW handled in Radio" (piHPSDR `cw_keyer_internal`). Default on.
    internal: AtomicBool,
    /// "CW Break-In" (piHPSDR `cw_breakin`). Default on.
    breakin: AtomicBool,
    /// Set every UI frame: CW mode selected, not Tune/Two-Tone, TX frequency allowed.
    armed: AtomicBool,
    /// piHPSDR `MIDI_cw_is_active`: set while an external keyer's PTT is held.
    midi_active: AtomicBool,
    /// The key state the local sidetone follows.
    local_key: AtomicBool,
    /// The TX thread is running the host CW generator.
    tx_ready: AtomicBool,
    kcwl: AtomicBool,
    kcwr: AtomicBool,
    dot_memory: AtomicBool,
    dash_memory: AtomicBool,
    external_straight: AtomicBool,
    ring: Mutex<VecDeque<(bool, u32)>>,
    last_enqueue: Mutex<Option<Instant>>,
    /// Time of the previous `CwKeyerKeydown` event (their `wait` is measured from it).
    last_keydown_event: Mutex<Option<Instant>>,
    attached: Mutex<Option<Attached>>,
    keyer_wake: (Mutex<u32>, Condvar),
    tx_wake: (Mutex<bool>, Condvar),
    bindings: Mutex<Vec<CwBinding>>,
}

static HOST: OnceLock<Host> = OnceLock::new();

fn host() -> &'static Host {
    HOST.get_or_init(|| {
        let h = Host {
            internal: AtomicBool::new(true),
            breakin: AtomicBool::new(true),
            armed: AtomicBool::new(false),
            midi_active: AtomicBool::new(false),
            local_key: AtomicBool::new(false),
            tx_ready: AtomicBool::new(false),
            kcwl: AtomicBool::new(false),
            kcwr: AtomicBool::new(false),
            dot_memory: AtomicBool::new(false),
            dash_memory: AtomicBool::new(false),
            external_straight: AtomicBool::new(false),
            ring: Mutex::new(VecDeque::new()),
            last_enqueue: Mutex::new(None),
            last_keydown_event: Mutex::new(None),
            attached: Mutex::new(None),
            keyer_wake: (Mutex::new(0), Condvar::new()),
            tx_wake: (Mutex::new(false), Condvar::new()),
            bindings: Mutex::new(Vec::new()),
        };
        std::thread::Builder::new().name("cw-keyer".into()).spawn(keyer_thread).ok();
        h
    })
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Settings / state shared with the UI
// ---------------------------------------------------------------------------------------------------------------------------------

/// Connects the keyer to the current session (MOX flag, keyer settings, the radio's own PTT readback).
pub fn attach(mox: Arc<AtomicBool>, keyer: Arc<CwKeyerAtomics>, radio_ptt: Arc<AtomicBool>) {
    *host().attached.lock().unwrap() = Some(Attached { mox, keyer, radio_ptt });
}

pub fn set_internal(on: bool) {
    host().internal.store(on, Ordering::Relaxed);
}

pub fn internal() -> bool {
    host().internal.load(Ordering::Relaxed)
}

pub fn set_breakin(on: bool) {
    host().breakin.store(on, Ordering::Relaxed);
}

pub fn breakin() -> bool {
    host().breakin.load(Ordering::Relaxed)
}

/// UI frame: whether host CW keying is currently allowed at all (CW mode, not Tune/Two-Tone, TX frequency allowed).
pub fn set_armed(on: bool) {
    let h = host();
    if h.armed.swap(on, Ordering::Relaxed) && !on {
        h.local_key.store(false, Ordering::Relaxed);
        h.ring.lock().unwrap().clear();
    }
}

/// The host (not the radio's own keyer) is in charge of CW keying: main.rs disarms the radio's internal keyer while this is true, and the
/// TX thread runs the host CW generator.
pub fn host_mode() -> bool {
    let h = host();
    h.armed.load(Ordering::Relaxed) && (!h.internal.load(Ordering::Relaxed) || h.midi_active.load(Ordering::Relaxed))
}

/// Local sidetone key state.
pub fn local_key() -> bool {
    host().local_key.load(Ordering::Relaxed)
}

fn set_local_key(on: bool) {
    host().local_key.store(on, Ordering::Relaxed);
}

/// UI frame: keeps the MIDI thread's copy of the CW bindings current.
pub fn sync_bindings(bindings: &[MidiBinding]) {
    let wanted: Vec<CwBinding> = bindings
        .iter()
        .filter_map(|b| {
            let act = match b.action {
                MidiAction::CwLeft => CwAct::Left,
                MidiAction::CwRight => CwAct::Right,
                MidiAction::CwStraightKey => CwAct::StraightKey,
                MidiAction::CwKeyerKeydown => CwAct::KeyerKeydown,
                MidiAction::CwKeyerPtt => CwAct::KeyerPtt,
                _ => return None,
            };
            Some(CwBinding { event: b.event, channel: b.channel, number: b.number, act })
        })
        .collect();
    let mut cur = host().bindings.lock().unwrap();
    if *cur != wanted {
        *cur = wanted;
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// MIDI thread entry (piHPSDR actions.c schedule_action, handled immediately)
// ---------------------------------------------------------------------------------------------------------------------------------

/// Called from the MIDI driver's callback thread for every event. Handles the CW actions at once.
pub fn midi_event(ev: &RawMidiEvent) {
    let h = host();
    let act = {
        let b = h.bindings.lock().unwrap();
        b.iter().find(|b| b.event == ev.kind && b.number == ev.number && b.channel.is_none_or(|c| c == ev.channel)).map(|b| b.act)
    };
    let Some(act) = act else { return };
    let pressed = !ev.off;
    crate::cw_latency::midi_to_keyer();
    match act {
        CwAct::Left | CwAct::Right => keyer_event(act == CwAct::Left, pressed),
        CwAct::StraightKey => straight_event(pressed),
        CwAct::KeyerKeydown => keyer_keydown(pressed),
        CwAct::KeyerPtt => keyer_ptt(pressed),
    }
}

fn wake_keyer() {
    let (m, c) = &host().keyer_wake;
    *m.lock().unwrap() += 1;
    c.notify_one();
}

/// piHPSDR keyer_event(): a paddle was hit (`pressed`) or released. `left` = dot paddle (keys not reversed).
fn keyer_event(left: bool, pressed: bool) {
    let h = host();
    // piHPSDR: the keyer thread only exists with "CW handled in Radio" off.
    if h.internal.load(Ordering::Relaxed) || !h.armed.load(Ordering::Relaxed) {
        return;
    }
    if left {
        h.kcwl.store(pressed, Ordering::Relaxed);
        if pressed {
            h.dot_memory.store(true, Ordering::Relaxed);
        }
    } else {
        h.kcwr.store(pressed, Ordering::Relaxed);
        if pressed {
            h.dash_memory.store(true, Ordering::Relaxed);
        }
    }
    if pressed {
        wake_keyer();
    }
}

/// deskHPSDR CW_STRAIGHT_KEY (keyer_straight_event): an external straight key, independent of the paddle mode.
fn straight_event(pressed: bool) {
    let h = host();
    if h.internal.load(Ordering::Relaxed) || !h.armed.load(Ordering::Relaxed) {
        return;
    }
    h.external_straight.store(pressed, Ordering::Relaxed);
    if pressed {
        wake_keyer();
    }
}

/// piHPSDR CW_KEYER_KEYDOWN: hard key down/up without break-in, for an external keyer that handles PTT itself.
fn keyer_keydown(pressed: bool) {
    let h = host();
    if !h.armed.load(Ordering::Relaxed) {
        return;
    }
    let now = Instant::now();
    let wait = {
        let mut last = h.last_keydown_event.lock().unwrap();
        let w = last.map(|t| (48_000.0 * now.duration_since(t).as_secs_f64() + 0.5) as i64).unwrap_or(i64::MAX);
        *last = Some(now);
        w
    };
    if pressed && (!h.internal.load(Ordering::Relaxed) || h.midi_active.load(Ordering::Relaxed)) {
        set_local_key(true);
        crate::cw_latency::paddle_edge(1);
        // After a pause, queue without delay.
        queue_event(true, if wait > LONG_PAUSE_SAMPLES { 0 } else { wait as u32 });
    } else {
        set_local_key(false);
        queue_event(false, wait.clamp(0, u32::MAX as i64 / 2) as u32);
    }
}

/// piHPSDR CW_KEYER_PTT: PTT from an external keyer. Also sets `midi_active`, which temporarily disables "CW handled in Radio".
fn keyer_ptt(pressed: bool) {
    let h = host();
    let att = h.attached.lock().unwrap();
    let Some(att) = att.as_ref() else { return };
    if pressed {
        if !h.armed.load(Ordering::Relaxed) {
            return;
        }
        h.midi_active.store(true, Ordering::Relaxed);
        // Wake the UI so the radio's internal keyer is disarmed (cw_mode_active) right away.
        crate::wake_ui();
        att.mox.store(true, Ordering::Relaxed);
        wake_tx();
    } else {
        h.midi_active.store(false, Ordering::Relaxed);
        crate::wake_ui();
        // The radio itself may be holding PTT (footswitch, or a key on the radio): then it ends the transmission.
        if !att.radio_ptt.load(Ordering::Relaxed) {
            att.mox.store(false, Ordering::Relaxed);
        }
        set_local_key(false);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Event ring (piHPSDR tx_queue_cw_event) and the TX-side generator
// ---------------------------------------------------------------------------------------------------------------------------------

/// piHPSDR tx_queue_cw_event(): `wait` is in 1/48000 s since the previous event.
fn queue_event(down: bool, wait: u32) {
    if down {
        crate::cw_latency::send_started();
    }
    let h = host();
    let mut ring = h.ring.lock().unwrap();
    // If the ring is nearly full, make all events key-up.
    let down = down && ring.len() + 16 <= 1024;
    if ring.len() < 1024 {
        ring.push_back((down, wait));
    }
    *h.last_enqueue.lock().unwrap() = Some(Instant::now());
}

/// TX thread: whether to run the host CW generator this iteration (MOX is on).
pub fn tx_branch_wanted() -> bool {
    host_mode()
}

/// TX thread: marks the host generator as running (the keyer waits for this after raising MOX, deskHPSDR's `cw_not_ready`).
pub fn tx_set_ready(ready: bool) {
    host().tx_ready.store(ready, Ordering::Relaxed);
}

/// TX thread, idle branch (MOX off): forget stale events and wait for the next MOX request or 20 ms. Replaces a plain 20 ms sleep so
/// that raising MOX from the keyer wakes the TX thread at once.
pub fn tx_idle_wait(timeout: Duration) {
    let h = host();
    h.tx_ready.store(false, Ordering::Relaxed);
    let stale = h.last_enqueue.lock().unwrap().is_none_or(|t| t.elapsed() > STALE_EVENTS);
    if stale {
        h.ring.lock().unwrap().clear();
    }
    let (m, c) = &h.tx_wake;
    let mut woke = m.lock().unwrap();
    if !*woke {
        woke = c.wait_timeout(woke, timeout).unwrap().0;
    }
    *woke = false;
}

fn wake_tx() {
    let (m, c) = &host().tx_wake;
    *m.lock().unwrap() = true;
    c.notify_one();
}

/// Plays the event ring: one shaped carrier (I = envelope, Q = 0) plus a sidetone tone for the TX monitor, at the DUC `rate`.
pub struct HostKeyGen {
    pending: VecDeque<(bool, u32)>,
    keydown: bool,
    /// Output samples since the last event was applied.
    delay: u64,
    gain: f32,
    sidetone_phase: f32,
}

impl HostKeyGen {
    pub fn new() -> Self {
        Self { pending: VecDeque::new(), keydown: false, delay: 0, gain: 0.0, sidetone_phase: 0.0 }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Fills `iq` (interleaved I,Q, `2*count` values) and `mono` (`count` sidetone samples).
    pub fn fill(&mut self, count: usize, rate: u32, sidetone_freq_hz: f32, sidetone_volume: f32, iq: &mut Vec<f32>, mono: &mut Vec<f32>) {
        iq.clear();
        mono.clear();
        {
            let mut ring = host().ring.lock().unwrap();
            self.pending.extend(ring.drain(..));
        }
        let ramp_step = 1.0 / (0.005 * rate as f32).max(1.0);
        let tone_step = 2.0 * std::f32::consts::PI * sidetone_freq_hz / rate as f32;
        let per_48k = (rate as u64 / 48_000).max(1);
        let max_down = MAX_KEYDOWN_SAMPLES * per_48k;
        for _ in 0..count {
            self.delay = (self.delay + 1).min(u64::MAX / 2);
            if self.keydown && self.delay > max_down {
                self.keydown = false;
            }
            if let Some(&(state, wait)) = self.pending.front() {
                if self.delay >= wait as u64 * per_48k {
                    self.delay = 0;
                    self.keydown = state;
                    self.pending.pop_front();
                }
            }
            let target = if self.keydown { 1.0 } else { 0.0 };
            if self.gain < target {
                self.gain = (self.gain + ramp_step).min(target);
            } else if self.gain > target {
                self.gain = (self.gain - ramp_step).max(target);
            }
            iq.push(self.gain);
            iq.push(0.0);
            self.sidetone_phase += tone_step;
            if self.sidetone_phase >= 2.0 * std::f32::consts::PI {
                self.sidetone_phase -= 2.0 * std::f32::consts::PI;
            }
            mono.push(sidetone_volume * self.gain * self.sidetone_phase.sin());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Iambic keyer thread (piHPSDR iambic.c)
// ---------------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Check,
    Straight,
    ExternalStraight,
    PreDot,
    AfterDot,
    PreDash,
    AfterDash,
    ExitLoop,
}

fn sleep_until(t: Instant) {
    let now = Instant::now();
    if t > now {
        std::thread::sleep(t - now);
    }
}

fn samples_ns(samples: i64) -> Duration {
    Duration::from_nanos((samples as f64 * SAMPLE_NS) as u64)
}

fn keyer_thread() {
    let h = host();
    loop {
        // Wait for a paddle event (the semaphore).
        {
            let (m, c) = &h.keyer_wake;
            let mut n = m.lock().unwrap();
            while *n == 0 {
                n = c.wait(n).unwrap();
            }
            *n = 0;
        }
        // Swallow events posted during the last hang time.
        if !h.kcwl.load(Ordering::Relaxed) && !h.kcwr.load(Ordering::Relaxed) && !h.external_straight.load(Ordering::Relaxed) {
            continue;
        }
        let (mox, keyer) = {
            let a = h.attached.lock().unwrap();
            match a.as_ref() {
                Some(a) => (Arc::clone(&a.mox), Arc::clone(&a.keyer)),
                None => continue,
            }
        };
        run_session(h, &mox, &keyer);
    }
}

/// One break-in session: from the first key event until the hang time after the last element has run out.
fn run_session(h: &Host, mox: &AtomicBool, keyer: &CwKeyerAtomics) {
    // If MOX is already on when the key is hit (PTT footswitch), the keyer does not switch back to RX afterwards.
    let moxbefore = mox.load(Ordering::Relaxed);
    let mut cwvox: i64 = 0; // without break-in this stays 0
    if h.breakin.load(Ordering::Relaxed) && h.armed.load(Ordering::Relaxed) {
        mox.store(true, Ordering::Relaxed);
        wake_tx();
        // Wait for the TX side to be running, so that the first dot is not swallowed. Give up after 200 ms (out of band: it never comes).
        let mut i = 200;
        while !h.tx_ready.load(Ordering::Relaxed) && i > 0 {
            std::thread::sleep(Duration::from_millis(1));
            i -= 1;
        }
        cwvox = keyer.hang_time_ms.load(Ordering::Relaxed) as i64;
    }
    let mut key_state = KeyState::Check;
    let mut dot_held = false;
    let mut dash_held = false;
    let mut tdown = Instant::now();
    while key_state != KeyState::ExitLoop || cwvox > 0 {
        if !h.armed.load(Ordering::Relaxed) {
            // Left CW mode (or TX no longer allowed): stop at once.
            set_local_key(false);
            if !moxbefore && h.breakin.load(Ordering::Relaxed) {
                mox.store(false, Ordering::Relaxed);
            }
            return;
        }
        let speed = keyer.speed_wpm.load(Ordering::Relaxed).max(1) as i64;
        let weight = keyer.weight.load(Ordering::Relaxed).max(1) as i64;
        let mode = keyer.mode.load(Ordering::Relaxed);
        let dot_samples = 57_600 / speed;
        let dash_samples = (3_456 * weight) / speed;
        let kdot = h.kcwl.load(Ordering::Relaxed);
        let kdash = h.kcwr.load(Ordering::Relaxed);
        let ext = h.external_straight.load(Ordering::Relaxed);
        // Re-trigger the hang timer for all states except the busy-spinning ones.
        if cwvox > 0 && key_state != KeyState::ExitLoop && key_state != KeyState::Check {
            cwvox = keyer.hang_time_ms.load(Ordering::Relaxed) as i64;
        }
        let entry = Instant::now();
        let mut next = entry; // when this iteration's wait ends
        match key_state {
            KeyState::ExitLoop => {
                // cwvox is greater than zero here (otherwise the outer loop would have ended).
                cwvox -= 1;
                if cwvox == 0 {
                    if !moxbefore {
                        mox.store(false, Ordering::Relaxed);
                    }
                } else {
                    key_state = KeyState::Check;
                }
                next = entry + Duration::from_millis(1);
            }
            KeyState::Check => {
                key_state = KeyState::ExitLoop;
                if cwvox > 1 {
                    cwvox -= 1;
                }
                if ext {
                    // deskHPSDR's external straight key: independent of the paddle mode.
                    tdown = entry;
                    set_local_key(true);
                    crate::cw_latency::paddle_edge(1);
                    queue_event(true, 0);
                    key_state = KeyState::ExternalStraight;
                } else if mode == CW_KEYER_MODE_STRAIGHT {
                    // Straight/External key or bug: a dot paddle makes automatic dots, the dash paddle is the key (it wins).
                    if kdot {
                        key_state = KeyState::PreDot;
                    }
                    if kdash {
                        tdown = entry;
                        set_local_key(true);
                        crate::cw_latency::paddle_edge(1);
                        queue_event(true, 0);
                        key_state = KeyState::Straight;
                    }
                } else {
                    // Paddles: a simultaneous squeeze means dot-dash.
                    if kdash {
                        key_state = KeyState::PreDash;
                    }
                    if kdot {
                        key_state = KeyState::PreDot;
                    }
                }
            }
            KeyState::Straight | KeyState::ExternalStraight => {
                let held = if key_state == KeyState::Straight { kdash } else { ext };
                if !held {
                    // An almost immediate key-up, but the key-down lasts as long as it really was down.
                    set_local_key(false);
                    let down_samples = (entry.duration_since(tdown).as_secs_f64() * 48_000.0) as i64;
                    queue_event(false, down_samples.max(0) as u32);
                    key_state = KeyState::Check;
                } else {
                    next = entry + Duration::from_millis(1);
                }
            }
            KeyState::PreDot => {
                // Start sending the dot.
                h.dash_memory.store(false, Ordering::Relaxed);
                dash_held = kdash;
                set_local_key(true);
                crate::cw_latency::paddle_edge(1);
                queue_event(true, 0);
                queue_event(false, dot_samples as u32); // wait the dot length, then key-up
                queue_event(false, dot_samples as u32); // and the inter-element space
                // The local sidetone ends with the dot; the loop waits for the end of the inter-element pause.
                sleep_until(entry + samples_ns(dot_samples));
                set_local_key(false);
                next = entry + samples_ns(2 * dot_samples);
                key_state = KeyState::AfterDot;
            }
            KeyState::AfterDot => {
                if mode == CW_KEYER_MODE_STRAIGHT {
                    // Bug mode: continue sending dots or exit, depending on the dot key.
                    key_state = KeyState::ExitLoop;
                    if kdot {
                        key_state = KeyState::PreDot;
                    }
                } else {
                    // Mode A: if both keys are released at the end of the delay, do not start the next element, unless the dash was hit
                    // DURING the preceding dot.
                    if mode == CW_KEYER_MODE_IAMBIC_A && !kdot && !kdash {
                        dash_held = false;
                    }
                    if h.dash_memory.load(Ordering::Relaxed) || kdash || dash_held {
                        key_state = KeyState::PreDash;
                    } else if kdot {
                        key_state = KeyState::PreDot;
                    } else {
                        key_state = KeyState::ExitLoop;
                    }
                }
            }
            KeyState::PreDash => {
                h.dot_memory.store(false, Ordering::Relaxed);
                dot_held = kdot; // remember whether the dot is still held at the start of the dash
                set_local_key(true);
                crate::cw_latency::paddle_edge(1);
                queue_event(true, 0);
                queue_event(false, dash_samples as u32);
                queue_event(false, dot_samples as u32);
                sleep_until(entry + samples_ns(dash_samples));
                set_local_key(false);
                next = entry + samples_ns(dash_samples + dot_samples);
                key_state = KeyState::AfterDash;
            }
            KeyState::AfterDash => {
                if mode == CW_KEYER_MODE_IAMBIC_A && !kdot && !kdash {
                    dot_held = false;
                }
                if h.dot_memory.load(Ordering::Relaxed) || kdot || dot_held {
                    key_state = KeyState::PreDot;
                } else if kdash {
                    key_state = KeyState::PreDash;
                } else {
                    key_state = KeyState::ExitLoop;
                }
            }
        }
        sleep_until(next);
    }
    // The hang time has run out (break-in) or there was none: MOX was dropped above when break-in is on.
    set_local_key(false);
}
