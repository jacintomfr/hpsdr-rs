//! TX relay diagnostics: why does the TX relay click on and off while TUNE / MOX is held?
//!
//! While `/tmp/hpsdr_diag.enable` exists (the same switch as the other diagnostics) every change of the transmit state is logged as a `tx:` line in
//! `/tmp/hpsdr_perf.log`, with a millisecond timestamp, and while transmitting a status line is written every 200 ms. With the file absent nothing is
//! stored and the only cost is one atomic load per call.
//!
//! What is logged:
//! * `tx: mox ON/OFF by <file>:<line>` -- every `RadioSession::set_mox` that changes the state, with the caller's source position (UI button, TUNE
//!   logic, TX inhibit, SWR protection, RTTY / SSTV auto-drop ...). A change that is *not* preceded by such a call came from a path that writes the
//!   flag directly (CAT, rigctl, TCI, the host CW keyer) and is shown as `mox ON/OFF by <direct write: CAT / rigctl / TCI / CW keyer / MIDI?>`.
//! * `tx: state ...` -- on any change of: the app's MOX, the PTT bit the radio echoes back, TUNE, the TX inhibit input, the TX FIFO under / overrun
//!   flags, the ADC overload flags, the TX drive setting (`pw=`, which SWR protection lowers to 10 W) -- one line with everything at that moment.
//! * `tx: tick ...` -- every 200 ms while MOX is on: the same values plus forward / reverse power, PA temperature and current (HL2).
//!
//! Reading it: if `mox OFF by` names a source line, that code released the PTT. If the app's `mox=1` stays steady and the `radio_ptt` echo or the
//! `fifo_under` flag flips, the radio itself dropped TX (a TX FIFO underrun, its own protection, or an external PTT / inhibit input). If `pw=` falls
//! from your setting to 10, SWR protection cut the drive.

use crate::radio::RadioSession;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DIAG_FLAG: &str = "/tmp/hpsdr_diag.enable";
const LOG_FILE: &str = "/tmp/hpsdr_perf.log";

static T0: OnceLock<Instant> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static CHECKED_MS: AtomicU64 = AtomicU64::new(u64::MAX);
/// The caller of the last `set_mox` that changed the state, and when.
static LAST_SET: Mutex<Option<(Instant, String)>> = Mutex::new(None);

fn now_ms() -> u64 {
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// True while the diagnostics switch file exists (re-checked at most twice a second).
pub fn enabled() -> bool {
    let now = now_ms();
    let last = CHECKED_MS.load(Ordering::Relaxed);
    if last == u64::MAX || now.saturating_sub(last) >= 500 {
        CHECKED_MS.store(now, Ordering::Relaxed);
        ENABLED.store(std::path::Path::new(DIAG_FLAG).exists(), Ordering::Relaxed);
    }
    ENABLED.load(Ordering::Relaxed)
}

pub fn log(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(f, "tx: t={}ms {line}", now_ms());
    }
}

/// `RadioSession::set_mox` changed the state from `prev` to `on`, called from `loc`.
pub fn note_set_mox(prev: bool, on: bool, loc: &std::panic::Location<'_>) {
    if prev == on || !enabled() {
        return;
    }
    let by = format!("{}:{}", loc.file().rsplit(['/', '\\']).next().unwrap_or(loc.file()), loc.line());
    log(&format!("mox {} by {by}", if on { "ON" } else { "OFF" }));
    *LAST_SET.lock().unwrap() = Some((Instant::now(), by));
}

#[derive(Clone, Copy, PartialEq)]
struct Flags {
    mox: bool,
    radio_ptt: bool,
    tune: bool,
    inhibit: bool,
    fifo_under: bool,
    fifo_over: bool,
    adc0: bool,
    adc1: bool,
    pw: u32,
}

static LAST_FLAGS: Mutex<Option<Flags>> = Mutex::new(None);
static LAST_TICK: Mutex<Option<Instant>> = Mutex::new(None);

fn snapshot(s: &RadioSession) -> Flags {
    Flags {
        mox: s.mox.load(Ordering::Relaxed),
        radio_ptt: s.cw_ptt_active.load(Ordering::Relaxed),
        tune: s.tune_active.load(Ordering::Relaxed),
        inhibit: s.hardware_tx_inhibit.load(Ordering::Relaxed),
        fifo_under: s.tx_fifo_underrun.load(Ordering::Relaxed),
        fifo_over: s.tx_fifo_overrun.load(Ordering::Relaxed),
        adc0: s.adc0_overload.load(Ordering::Relaxed),
        adc1: s.adc1_overload.load(Ordering::Relaxed),
        pw: s.tx_power_watts.load(Ordering::Relaxed),
    }
}

fn describe(s: &RadioSession, f: &Flags) -> String {
    format!(
        "mox={} radio_ptt={} tune={} inhibit={} fifo_under={} fifo_over={} adc0_ov={} adc1_ov={} pw={}W freq={} fwd_raw={} rev_raw={} pa_temp_raw={} pa_cur_raw={} oc_tx={:#04x}",
        f.mox as u8,
        f.radio_ptt as u8,
        f.tune as u8,
        f.inhibit as u8,
        f.fifo_under as u8,
        f.fifo_over as u8,
        f.adc0 as u8,
        f.adc1 as u8,
        f.pw,
        s.tx_frequency_hz.load(Ordering::Relaxed),
        s.tx_forward_power.load(Ordering::Relaxed),
        s.tx_reverse_power.load(Ordering::Relaxed),
        s.hl2_pa_temp_raw.load(Ordering::Relaxed),
        s.hl2_pa_current_raw.load(Ordering::Relaxed),
        s.oc_tx.load(Ordering::Relaxed),
    )
}

/// Call once per UI frame: logs the changes of the transmit state and, while transmitting, a status line every 200 ms.
pub fn tick(s: &RadioSession) {
    if !enabled() {
        return;
    }
    let f = snapshot(s);
    let mut last = LAST_FLAGS.lock().unwrap();
    let prev = *last;
    *last = Some(f);
    if let Some(p) = prev {
        if p != f {
            let mut what = Vec::new();
            if p.mox != f.mox {
                // A change that no set_mox call announced in the last 300 ms was a direct write (CAT, rigctl, TCI, the host CW keyer).
                let announced = LAST_SET.lock().unwrap().as_ref().is_some_and(|(t, _)| t.elapsed() < Duration::from_millis(300));
                what.push(if announced {
                    format!("mox {}", if f.mox { "ON" } else { "OFF" })
                } else {
                    format!("mox {} by <direct write: CAT / rigctl / TCI / CW keyer>", if f.mox { "ON" } else { "OFF" })
                });
            }
            if p.radio_ptt != f.radio_ptt {
                what.push(format!("radio PTT echo {}", if f.radio_ptt { "ON" } else { "OFF" }));
            }
            if p.tune != f.tune {
                what.push(format!("tune {}", if f.tune { "ON" } else { "OFF" }));
            }
            if p.inhibit != f.inhibit {
                what.push(format!("TX inhibit {}", if f.inhibit { "ASSERTED" } else { "released" }));
            }
            if p.fifo_under != f.fifo_under {
                what.push(format!("TX FIFO underrun {}", if f.fifo_under { "SET" } else { "cleared" }));
            }
            if p.fifo_over != f.fifo_over {
                what.push(format!("TX FIFO overrun {}", if f.fifo_over { "SET" } else { "cleared" }));
            }
            if p.adc0 != f.adc0 || p.adc1 != f.adc1 {
                what.push("ADC overload changed".to_string());
            }
            if p.pw != f.pw {
                what.push(format!("TX power setting {}W -> {}W", p.pw, f.pw));
            }
            log(&format!("state [{}] | {}", what.join(", "), describe(s, &f)));
        }
    }
    if f.mox || f.radio_ptt {
        let mut lt = LAST_TICK.lock().unwrap();
        if lt.map_or(true, |t| t.elapsed() >= Duration::from_millis(200)) {
            *lt = Some(Instant::now());
            log(&format!("tick | {}", describe(s, &f)));
        }
    }
}
