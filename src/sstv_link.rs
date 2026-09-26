/*
    Glue between the SSTV modem (sstv.rs -- the DSP itself, consumed here as
    a library and not modified) and the rest of the app: one shared,
    cheap-to-Clone handle, same shape as rtty_link.rs's RttyHandle.

    RX only for now -- see the "Digital" window's own mode switch in main.rs
    (rtty_link.rs and this module are siblings there, selected one at a
    time). sstv.rs's SstvTx is already ported and unit-tested, ready for a
    future transmit feature (image picker, crop, FSK-ID-on-send), but
    nothing here calls it yet.

    RX: spectrum.rs's run() hands feed_rx() the same pre-Audio-Gain mono
    demod downmix the RTTY/CW decoders read (48kHz), only while rx_enabled
    (i.e. while main.rs's Digital window is open AND SSTV is the selected
    mode -- decoding stops rather than running two DSPs' worth of FIR
    filtering in the background for no UI anyone is looking at).
*/

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::sstv::{SstvEvent, SstvMode, SstvRx};

/// The RX demod output (spectrum.rs's OUTPUT_RATE), same as rtty_link's
/// SAMPLE_RATE_HZ.
pub const SAMPLE_RATE_HZ: f64 = 48_000.0;

/// A snapshot of what the panel needs to draw: the image so far (or the
/// last completed one), plus the receiver's current state.
#[derive(Clone)]
pub struct SstvSnapshot {
    /// Monotonically increasing across pictures -- bumped on ModeDetected
    /// and on restart(), so the UI knows to rebuild its texture instead of
    /// re-uploading an unchanged one every frame.
    pub image_id: u32,
    pub w: u16,
    pub h: u16,
    /// `3 * w * h` bytes, interleaved RGB, row-major. Empty when no picture
    /// has started yet.
    pub rgb: Vec<u8>,
    /// True while a picture is actively being decoded (VIS locked).
    pub receiving: bool,
    /// Fraction of the current image decoded, 0.0..=1.0 -- 0 when not
    /// receiving (matches SstvRx::progress()).
    pub progress: f32,
    /// The mode last detected (stays set after ImageComplete, until the
    /// next VIS header or a restart()).
    pub detected: Option<SstvMode>,
    /// Smoothed in-band signal level, ~0..1, for an activity meter.
    pub level: f32,
    /// How clean the most recently decoded line's sync pulse was, 0.0..=1.0
    /// -- the same tuning aid QSSTV's own "Sync" meter is: a strong,
    /// consistently full sync pulse every line means the operator is
    /// tuned correctly; a weak or intermittent one means to nudge the VFO
    /// a few Hz either way and watch which direction it improves. See
    /// sstv.rs's SstvRx::sync_quality doc comment.
    pub sync_quality: f32,
    /// The last header seen for a mode this build cannot draw -- see
    /// sstv.rs's SstvEvent::UnsupportedMode. Cleared when a picture this
    /// decoder *can* draw starts.
    pub unsupported: Option<String>,
    /// The callsign the last station sent in its FSK ID, if any -- arrives
    /// after the picture, so it is held here rather than travelling with
    /// one.
    pub rx_id: Option<String>,
}

impl Default for SstvSnapshot {
    fn default() -> Self {
        SstvSnapshot {
            image_id: 0,
            w: 0,
            h: 0,
            rgb: Vec::new(),
            receiving: false,
            progress: 0.0,
            detected: None,
            level: 0.0,
            sync_quality: 0.0,
            unsupported: None,
            rx_id: None,
        }
    }
}

struct Inner {
    rx: Mutex<SstvRx>,
    rx_enabled: AtomicBool,
    /// Mirrors what was last passed to SstvRx::set_expected -- SstvRx has
    /// no getter of its own, and the mode picker needs to show the current
    /// selection.
    expected: Mutex<Option<SstvMode>>,
    snapshot: Mutex<SstvSnapshot>,
    /// Smoothed level as bits, updated every feed_rx call -- cheap to read
    /// from the UI thread without taking the (higher-contention) `rx` lock.
    level_bits: AtomicU32,
    /// SstvRx::sync_quality() as bits, same reasoning as level_bits.
    sync_quality_bits: AtomicU32,
}

#[derive(Clone)]
pub struct SstvHandle {
    inner: Arc<Inner>,
}

impl SstvHandle {
    pub fn new() -> Self {
        let mut rx = SstvRx::new(SAMPLE_RATE_HZ);
        rx.set_expected(None); // Auto by default
        Self {
            inner: Arc::new(Inner {
                rx: Mutex::new(rx),
                rx_enabled: AtomicBool::new(false),
                expected: Mutex::new(None),
                snapshot: Mutex::new(SstvSnapshot::default()),
                level_bits: AtomicU32::new(0),
                sync_quality_bits: AtomicU32::new(0),
            }),
        }
    }

    pub fn same_as(&self, other: &SstvHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn rx_enabled(&self) -> bool {
        self.inner.rx_enabled.load(Ordering::Relaxed)
    }

    pub fn set_rx_enabled(&self, on: bool) {
        self.inner.rx_enabled.store(on, Ordering::Relaxed);
    }

    /// The mode free-run decoding is pinned to, or `None` for Auto
    /// (identify from the sync cadence).
    pub fn expected(&self) -> Option<SstvMode> {
        *self.inner.expected.lock().unwrap()
    }

    pub fn set_expected(&self, mode: Option<SstvMode>) {
        *self.inner.expected.lock().unwrap() = mode;
        self.inner.rx.lock().unwrap().set_expected(mode);
    }

    /// Abandon whatever picture is in progress and go back to hunting for a
    /// header -- see SstvRx::restart's own doc comment for why this exists
    /// (a wrong or stale VIS lock otherwise commits the receiver for as
    /// long as the mode it locked onto runs, up to several minutes).
    pub fn restart(&self) {
        self.inner.rx.lock().unwrap().restart();
        let mut snap = self.inner.snapshot.lock().unwrap();
        snap.image_id = snap.image_id.wrapping_add(1);
        snap.w = 0;
        snap.h = 0;
        snap.rgb.clear();
        snap.receiving = false;
        snap.progress = 0.0;
        snap.detected = None;
    }

    /// RX audio tap -- see spectrum.rs's run(). Mono, 48kHz.
    pub fn feed_rx(&self, audio: &[f32]) {
        if audio.is_empty() {
            return;
        }
        let mut rx = self.inner.rx.lock().unwrap();
        let mut events = Vec::new();
        rx.process(audio, &mut events);
        self.inner.level_bits.store(rx.level().to_bits(), Ordering::Relaxed);
        self.inner.sync_quality_bits.store(rx.sync_quality().to_bits(), Ordering::Relaxed);
        let receiving = rx.receiving();
        let progress = rx.progress();
        drop(rx);
        if events.is_empty() && !receiving {
            // Still keep progress/receiving fresh even with no events this
            // block (the common case while hunting).
            let mut snap = self.inner.snapshot.lock().unwrap();
            snap.receiving = receiving;
            snap.progress = progress;
            return;
        }
        let mut snap = self.inner.snapshot.lock().unwrap();
        snap.receiving = receiving;
        snap.progress = progress;
        for e in events {
            match e {
                SstvEvent::ModeDetected(mode) => {
                    snap.image_id = snap.image_id.wrapping_add(1);
                    let (w, h) = mode.dimensions();
                    snap.w = w;
                    snap.h = h;
                    snap.rgb = vec![0u8; w as usize * h as usize * 3];
                    snap.detected = Some(mode);
                    snap.unsupported = None;
                }
                SstvEvent::Line { y, rgb } => {
                    let w = snap.w as usize;
                    let row = y as usize * w * 3;
                    if row + rgb.len() <= snap.rgb.len() {
                        snap.rgb[row..row + rgb.len()].copy_from_slice(&rgb);
                    }
                }
                SstvEvent::ImageComplete => {}
                SstvEvent::FskId(id) => {
                    snap.rx_id = Some(id);
                }
                SstvEvent::UnsupportedMode { code, name } => {
                    snap.unsupported =
                        Some(name.map(|n| n.to_string()).unwrap_or_else(|| format!("VIS ${code:02X}")));
                }
            }
        }
    }

    /// A snapshot of the current image + receiver state, for the UI to
    /// draw. Cheap to call at UI frame rate: one lock, one clone of the
    /// current RGB buffer (at most 800x616x3 = ~1.5MB for the largest mode,
    /// PD290).
    pub fn snapshot(&self) -> SstvSnapshot {
        let mut snap = self.inner.snapshot.lock().unwrap().clone();
        snap.level = f32::from_bits(self.inner.level_bits.load(Ordering::Relaxed));
        snap.sync_quality = f32::from_bits(self.inner.sync_quality_bits.load(Ordering::Relaxed));
        snap
    }
}

impl Default for SstvHandle {
    fn default() -> Self {
        Self::new()
    }
}
