/*
    Glue between the RTTY modem (rtty.rs -- the DSP itself, consumed here
    as a library and not modified) and the rest of the app: one shared,
    cheap-to-Clone handle, same shape as report_recorder.rs's
    ReportRecorder.

    RX: spectrum.rs's run() hands feed_rx() the same pre-Audio-Gain mono
    demod downmix the CW decoder reads (48kHz), only while rx_enabled
    (i.e. while main.rs's RTTY window is open -- decoding stops rather
    than piling up text in the background, same reasoning as the CW
    decoder's own enable toggle).

    TX: tx.rs's run() calls fill_tx() in place of the mic/TCI source
    selection while tx_armed, exactly where REC/PLAY's playback is
    injected -- so the AFSK tones go through the normal mic gain/TXA
    chain like any other audio source. PTT/MOX itself is still the
    operator's (this never keys the radio on its own); while armed and
    keyed with nothing queued, the modem sends its idle mark tone.

    Owned by ConnectedState rather than by SpectrumHandle: the main
    SpectrumHandle is rebuilt on a sample-rate change while TxHandle is
    not, so a handle living inside SpectrumHandle would leave TX holding
    a stale copy.
*/

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::rtty::{RttyRx, RttyTx};

/// Both the RX demod output (spectrum.rs's OUTPUT_RATE) and the TX mic
/// chunk (tx.rs's mic_rate, always MicInput's 48kHz) run at this rate.
pub const SAMPLE_RATE_HZ: f64 = 48_000.0;

/// Received-text scrollback cap -- older text is trimmed from the front.
const MAX_RX_TEXT_CHARS: usize = 20_000;

pub const BAUD_CHOICES: [f64; 4] = [45.45, 50.0, 75.0, 100.0];
pub const SHIFT_CHOICES: [f64; 4] = [170.0, 425.0, 450.0, 850.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RttySettings {
    pub center_hz: f64,
    pub baud: f64,
    pub shift_hz: f64,
    pub reverse: bool,
    pub afc: bool,
}

impl Default for RttySettings {
    fn default() -> Self {
        Self { center_hz: 1500.0, baud: 45.45, shift_hz: 170.0, reverse: false, afc: true }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RttyRxStatus {
    pub confidence: f32,
    pub lock: f32,
    pub afc_offset_hz: f32,
}

struct Inner {
    settings: Mutex<RttySettings>,
    rx: Mutex<RttyRx>,
    tx: Mutex<RttyTx>,
    rx_text: Mutex<String>,
    rx_enabled: AtomicBool,
    tx_armed: AtomicBool,
}

#[derive(Clone)]
pub struct RttyHandle {
    inner: Arc<Inner>,
}

impl RttyHandle {
    pub fn new() -> Self {
        let s = RttySettings::default();
        let mut rx = RttyRx::new(SAMPLE_RATE_HZ, s.center_hz, s.baud, s.shift_hz);
        rx.set_reverse(s.reverse);
        rx.set_afc(s.afc);
        let tx = RttyTx::new(SAMPLE_RATE_HZ, s.center_hz, s.baud, s.shift_hz);
        Self {
            inner: Arc::new(Inner {
                settings: Mutex::new(s),
                rx: Mutex::new(rx),
                tx: Mutex::new(tx),
                rx_text: Mutex::new(String::new()),
                rx_enabled: AtomicBool::new(false),
                tx_armed: AtomicBool::new(false),
            }),
        }
    }

    pub fn same_as(&self, other: &RttyHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn settings(&self) -> RttySettings {
        *self.inner.settings.lock().unwrap()
    }

    /// Applies only what actually changed. A baud change rebuilds the
    /// transmitter (RttyTx has no runtime baud setter), dropping any
    /// text still queued for it.
    pub fn set_settings(&self, new: RttySettings) {
        let mut cur = self.inner.settings.lock().unwrap();
        if *cur == new {
            return;
        }
        let old = *cur;
        *cur = new;
        drop(cur);
        {
            let mut rx = self.inner.rx.lock().unwrap();
            if old.center_hz != new.center_hz || old.baud != new.baud || old.shift_hz != new.shift_hz {
                rx.set_tuning(new.center_hz, new.baud, new.shift_hz);
            }
            if old.reverse != new.reverse {
                rx.set_reverse(new.reverse);
            }
            if old.afc != new.afc {
                rx.set_afc(new.afc);
            }
        }
        let mut tx = self.inner.tx.lock().unwrap();
        if old.baud != new.baud {
            *tx = RttyTx::new(SAMPLE_RATE_HZ, new.center_hz, new.baud, new.shift_hz);
        } else if old.center_hz != new.center_hz || old.shift_hz != new.shift_hz {
            tx.set_tuning(new.center_hz, new.shift_hz);
        }
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

    /// RX audio tap -- see spectrum.rs's run(). Mono, 48kHz.
    pub fn feed_rx(&self, audio: &[f32]) {
        if audio.is_empty() {
            return;
        }
        let text = self.inner.rx.lock().unwrap().process(audio);
        if text.is_empty() {
            return;
        }
        let mut buf = self.inner.rx_text.lock().unwrap();
        buf.push_str(&text);
        if buf.len() > MAX_RX_TEXT_CHARS {
            let mut cut = buf.len() - MAX_RX_TEXT_CHARS;
            while !buf.is_char_boundary(cut) {
                cut += 1;
            }
            buf.drain(..cut);
        }
    }

    pub fn rx_text(&self) -> String {
        self.inner.rx_text.lock().unwrap().clone()
    }

    pub fn clear_rx_text(&self) {
        self.inner.rx_text.lock().unwrap().clear();
    }

    pub fn rx_status(&self) -> RttyRxStatus {
        let rx = self.inner.rx.lock().unwrap();
        RttyRxStatus {
            confidence: rx.confidence(),
            lock: rx.lock(),
            afc_offset_hz: rx.afc_offset_hz(),
        }
    }

    pub fn send_text(&self, text: &str) {
        self.inner.tx.lock().unwrap().push_text(text);
    }

    pub fn clear_tx(&self) {
        self.inner.tx.lock().unwrap().clear();
    }

    /// (sent, total) characters of the TX queue since the last clear.
    pub fn tx_progress(&self) -> (usize, usize) {
        let tx = self.inner.tx.lock().unwrap();
        (tx.sent_chars(), tx.total_chars())
    }

    /// TX mic-input tap -- see tx.rs's run(). Fills `out` with AFSK
    /// audio (idle mark when nothing is queued).
    pub fn fill_tx(&self, out: &mut [f32]) {
        self.inner.tx.lock().unwrap().next_block(out);
    }
}

impl Default for RttyHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// fill_tx -> feed_rx through the handle, in the same 512-sample
    /// chunks tx.rs/spectrum.rs use, for every baud choice.
    #[test]
    fn handle_loopback() {
        for baud in BAUD_CHOICES {
            let h = RttyHandle::new();
            h.set_settings(RttySettings { baud, ..RttySettings::default() });
            h.set_rx_enabled(true);
            let mut chunk = [0.0f32; 512];
            // ~0.5s of idle mark first, as when MOX is keyed before Send.
            for _ in 0..47 {
                h.fill_tx(&mut chunk);
                h.feed_rx(&chunk);
            }
            h.send_text("RYRYRY CQ CQ DE CU2ED K ");
            for _ in 0..((48_000.0 * 12.0 / 512.0) as usize) {
                h.fill_tx(&mut chunk);
                h.feed_rx(&chunk);
            }
            let text = h.rx_text();
            assert!(text.contains("CQ DE CU2ED"), "baud {baud}: {text:?}");
            let (sent, total) = h.tx_progress();
            assert_eq!(sent, total);
        }
    }
}
