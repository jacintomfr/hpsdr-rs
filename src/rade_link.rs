/*
    Glue between the RADE V1 modem (src/rade/ -- the DSP/FFI itself, consumed
    here as a library and not modified) and the rest of the app: one shared,
    cheap-to-Clone handle, same shape as rtty_link.rs's RttyHandle and
    sstv_link.rs's SstvHandle.

    Unlike RTTY/SSTV, RADE drives its own dedicated thread (rade::RadeWorker
    -- neural inference is small per call but not free, and must not land on
    spectrum.rs's demod loop or tx.rs's mic-pacing loop) with lock-free rings
    at the edges, so this handle is mostly a thin, mutex-guarded pass-through
    to it rather than owning any DSP state itself.

    RX: spectrum.rs's run() hands feed_rx() the same pre-Audio-Gain mono
    demod downmix the RTTY/CW/SSTV decoders read (48kHz), only while
    rx_enabled -- but unlike those, which only accumulate text/an image,
    feed_rx() also RETURNS decoded speech: RADE's received audio is not
    intelligible SSB, it is modem tones that only make sense demodulated, so
    while it is the active mode the decoded speech REPLACES the normal
    demodulated audio in the speaker/TCI output (see spectrum.rs's own
    rade_rx-gated block) rather than riding alongside it.

    TX: tx.rs's run() calls fill_tx() in place of the mic/TCI source
    selection while tx_armed -- real mic audio goes in, RADE-modulated tone
    audio comes out, which then flows through the normal mic gain/TXA chain
    like any other audio source (same pattern as RTTY's AFSK, except RADE's
    input is the live microphone rather than typed text).

    Owned by ConnectedState rather than by SpectrumHandle, same reasoning as
    RttyHandle/SstvHandle: the main SpectrumHandle is rebuilt on a
    sample-rate change while TxHandle is not, so a handle living inside
    SpectrumHandle would leave TX holding a stale copy.
*/

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::rade::{RadeStats, RadeTextRx, RadeWorker};

/// Both the RX demod output (spectrum.rs's OUTPUT_RATE) and the TX mic
/// chunk (tx.rs's mic_rate, always MicInput's 48kHz) run at this rate --
/// same constant as rtty_link.rs/sstv_link.rs's own SAMPLE_RATE_HZ.
pub const SAMPLE_RATE_HZ: f64 = 48_000.0;

/// Received-callsign scrollback cap, same reasoning as rtty_link.rs's
/// MAX_RX_TEXT_CHARS -- older entries are trimmed from the front.
const MAX_RX_LOG: usize = 50;

struct Inner {
    /// `None` when RadeWorker::new() failed (e.g. a stale RADE context from
    /// an unclean previous session still reports AlreadyOpen) -- see
    /// RadeHandle::available's own doc comment. Behind one Mutex rather
    /// than split per-field: unlike RttyRx/SstvRx (synchronous DSP called
    /// directly on the caller's thread), RadeWorker's own methods are
    /// already just enqueue/dequeue on lock-free rings into ITS thread, so
    /// the lock here is held only as long as one push/pop call, never for
    /// the DSP work itself.
    worker: Mutex<Option<RadeWorker>>,
    rx_enabled: AtomicBool,
    tx_armed: AtomicBool,
    rx_log: Mutex<Vec<RadeTextRx>>,
}

#[derive(Clone)]
pub struct RadeHandle {
    inner: Arc<Inner>,
}

impl RadeHandle {
    pub fn new() -> Self {
        let worker = match RadeWorker::new(SAMPLE_RATE_HZ, SAMPLE_RATE_HZ) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("[rade] failed to start: {e:?} -- RADE mode will be unavailable this session");
                None
            }
        };
        Self {
            inner: Arc::new(Inner {
                worker: Mutex::new(worker),
                rx_enabled: AtomicBool::new(false),
                tx_armed: AtomicBool::new(false),
                rx_log: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn same_as(&self, other: &RadeHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// False when the RADE C context failed to open this session (see
    /// `new`'s own doc comment) -- the UI shows this rather than silently
    /// doing nothing.
    pub fn available(&self) -> bool {
        self.inner.worker.lock().unwrap().is_some()
    }

    pub fn rx_enabled(&self) -> bool {
        self.inner.rx_enabled.load(Ordering::Relaxed)
    }

    pub fn set_rx_enabled(&self, on: bool) {
        self.inner.rx_enabled.store(on, Ordering::Relaxed);
    }

    pub fn tx_armed(&self) -> bool {
        self.inner.tx_armed.load(Ordering::Relaxed)
    }

    pub fn set_tx_armed(&self, on: bool) {
        self.inner.tx_armed.store(on, Ordering::Relaxed);
    }

    /// Set the callsign transmitted in the End-of-Over frame -- see
    /// rade::RadeWorker::set_callsign's own doc comment.
    pub fn set_callsign(&self, call: &str) {
        if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
            w.set_callsign(call);
        }
    }

    /// RX audio tap -- see spectrum.rs's run(). Mono, 48kHz. Decoded speech
    /// (mono, also 48kHz -- see rade::MODEM_RATE/SPEECH_RATE's own doc
    /// comments for the resampling that happens inside the worker) is
    /// appended to `speech_out`; empty is normal (RADE frames are 120ms,
    /// so most calls produce nothing yet).
    pub fn feed_rx(&self, audio: &[f32], speech_out: &mut Vec<f32>) {
        if audio.is_empty() {
            return;
        }
        let mut w = self.inner.worker.lock().unwrap();
        if let Some(w) = w.as_mut() {
            w.push_rx(audio);
            w.pop_rx(speech_out);
            for RadeTextRx { call, snr_db } in w.poll_text() {
                let mut log = self.inner.rx_log.lock().unwrap();
                log.push(RadeTextRx { call, snr_db });
                if log.len() > MAX_RX_LOG {
                    log.remove(0);
                }
            }
        }
    }

    /// TX mic-input tap -- see tx.rs's run(). `mox_on` reflects the radio's
    /// real key state every call (RadeWorker::set_tx no-ops when it hasn't
    /// changed, so calling this every chunk while merely armed -- not yet
    /// keyed -- is fine): keying starts an over (the modem needs to know,
    /// to time its End-of-Over frame), unkeying flushes it. `mic` is real
    /// microphone audio; `out` is filled with the RADE-modulated tone
    /// audio to inject into the TX chain in its place -- silence while no
    /// worker is available (see `available`).
    pub fn fill_tx(&self, mox_on: bool, mic: &[f32], out: &mut [f32]) {
        let mut w = self.inner.worker.lock().unwrap();
        match w.as_mut() {
            Some(w) => {
                w.set_tx(mox_on);
                w.push_mic(mic);
                w.pop_tx(out);
            }
            None => out.fill(0.0),
        }
    }

    /// Current receive/transmit state, for the UI. Default (all-zero/false)
    /// when no worker is available.
    pub fn stats(&self) -> RadeStats {
        self.inner.worker.lock().unwrap().as_ref().map(|w| w.stats()).unwrap_or_default()
    }

    /// See rade::RadeWorker::tx_drained's own doc comment. `true` (nothing
    /// left to drain) when no worker is available.
    pub fn tx_drained(&self) -> bool {
        self.inner.worker.lock().unwrap().as_ref().map(|w| w.tx_drained()).unwrap_or(true)
    }

    /// Callsigns recovered from remote End-of-Over frames this session,
    /// oldest first, capped at [`MAX_RX_LOG`].
    pub fn rx_log(&self) -> Vec<RadeTextRx> {
        self.inner.rx_log.lock().unwrap().clone()
    }

    pub fn clear_rx_log(&self) {
        self.inner.rx_log.lock().unwrap().clear();
    }

    /// Drop receive state (sync, vocoder warm-up, buffered audio) -- see
    /// rade::RadeWorker::reset's own doc comment.
    pub fn reset_rx(&self) {
        if let Some(w) = self.inner.worker.lock().unwrap().as_ref() {
            w.reset();
        }
    }
}

impl Default for RadeHandle {
    fn default() -> Self {
        Self::new()
    }
}
