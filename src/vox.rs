/*!
VOX level detector, modelled on deskHPSDR's vox.c / vox_menu.c.

The TX thread feeds every block of microphone audio (idle or keyed) through `VoxDetector`, which multiplies it by the
mic gain, optionally band-passes it (deskHPSDR's "side channel filter", done with two biquads here instead of WDSP's
DEXP) and publishes the peak. The same thread then runs the VOX decision (threshold, hang time, keying `mox`) in
`decide`, so the UI redraws nothing for VOX; the UI only reads the peak (`take_level`) for the meter of the VOX window.
*/

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Largest block peak since the last `take_level` (f32 bits; positive floats order like their bits).
static VOX_LEVEL: AtomicU32 = AtomicU32::new(0);
static VOX_ENABLED: AtomicBool = AtomicBool::new(false);
static VOX_THRESHOLD: AtomicU32 = AtomicU32::new(0x3a83126f); // 0.001
static VOX_HANG_MS: AtomicU32 = AtomicU32::new(250);
static VOX_FILTER_ON: AtomicBool = AtomicBool::new(false);
static VOX_FILTER_LOW_HZ: AtomicU32 = AtomicU32::new(1000);
static VOX_FILTER_HIGH_HZ: AtomicU32 = AtomicU32::new(2000);

/// Side channel filter settings (deskHPSDR's dexp_filter, _low, _high).
pub fn configure(filter_on: bool, low_hz: u32, high_hz: u32) {
    VOX_FILTER_ON.store(filter_on, Ordering::Relaxed);
    VOX_FILTER_LOW_HZ.store(low_hz, Ordering::Relaxed);
    VOX_FILTER_HIGH_HZ.store(high_hz, Ordering::Relaxed);
}

/// Enable, threshold (0..1) and hang time (ms); read by the TX thread, which keys the radio itself, so the UI does not
/// have to redraw anything for VOX.
pub fn set_params(enabled: bool, threshold: f64, hang_ms: f64) {
    VOX_ENABLED.store(enabled, Ordering::Relaxed);
    VOX_THRESHOLD.store((threshold as f32).to_bits(), Ordering::Relaxed);
    VOX_HANG_MS.store(hang_ms as u32, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    VOX_ENABLED.load(Ordering::Relaxed)
}

/// The largest detector level since the previous call (0.0..~1.0), then restarts from zero.
pub fn take_level() -> f32 {
    f32::from_bits(VOX_LEVEL.swap(0, Ordering::Relaxed))
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// RBJ cookbook, Butterworth Q. `high_pass` false = low-pass.
    fn design(high_pass: bool, freq_hz: f64, rate: f64) -> Self {
        let f = freq_hz.clamp(10.0, rate * 0.45);
        let w = 2.0 * std::f64::consts::PI * f / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high_pass {
            ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0)
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0)
        };
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0, z1: 0.0, z2: 0.0 }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

pub struct VoxDetector {
    high_pass: Biquad,
    low_pass: Biquad,
    designed_for: Option<(u32, u32, u32)>,
    /// True while the transmitter is keyed by VOX; `until` is when the hang (+ PTT delay) runs out.
    active: bool,
    until: Option<Instant>,
}

impl VoxDetector {
    pub fn new() -> Self {
        VoxDetector { high_pass: Biquad::default(), low_pass: Biquad::default(), designed_for: None, active: false, until: None }
    }

    /// Feeds one block of mic audio (before the TX chain) and publishes its peak.
    pub fn feed(&mut self, samples: &[f32], mic_gain: f32, rate_hz: f64) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let filter_on = VOX_FILTER_ON.load(Ordering::Relaxed);
        let mut peak = 0.0f32;
        if filter_on {
            let lo = VOX_FILTER_LOW_HZ.load(Ordering::Relaxed);
            let hi = VOX_FILTER_HIGH_HZ.load(Ordering::Relaxed);
            let key = (lo, hi, rate_hz as u32);
            if self.designed_for != Some(key) {
                self.high_pass = Biquad::design(true, lo as f64, rate_hz);
                self.low_pass = Biquad::design(false, hi as f64, rate_hz);
                self.designed_for = Some(key);
            }
            for &s in samples {
                let y = self.low_pass.run(self.high_pass.run(s as f64 * mic_gain as f64));
                peak = peak.max(y.abs() as f32);
            }
        } else {
            for &s in samples {
                peak = peak.max((s * mic_gain).abs());
            }
        }
        VOX_LEVEL.fetch_max(peak.to_bits(), Ordering::Relaxed);
        peak
    }

    /// deskHPSDR vox.c: above the threshold the radio is keyed (`mox`); it is released `hang` ms (+ 50 ms PTT delay)
    /// after the last block above it. `allowed` is false while something else owns the transmitter (Tune, Two-tone, CW,
    /// digital modes) or VOX is off, which also releases a VOX-keyed radio. MOX pressed or dropped by hand wins.
    pub fn decide(&mut self, peak: f32, mox: &AtomicBool, allowed: bool) {
        const PTT_DELAY: Duration = Duration::from_millis(50);
        let keyed = mox.load(Ordering::Relaxed);
        if self.active && !keyed {
            self.active = false;
            self.until = None;
        }
        if !allowed || !VOX_ENABLED.load(Ordering::Relaxed) {
            if self.active {
                mox.store(false, Ordering::Relaxed);
                self.active = false;
                self.until = None;
            }
            return;
        }
        let threshold = f32::from_bits(VOX_THRESHOLD.load(Ordering::Relaxed));
        let hang = Duration::from_millis(VOX_HANG_MS.load(Ordering::Relaxed) as u64);
        if peak > threshold {
            if !keyed {
                mox.store(true, Ordering::Relaxed);
                self.active = true;
            }
            if self.active {
                self.until = Some(Instant::now() + hang + PTT_DELAY);
            }
        } else if self.active && self.until.is_some_and(|t| Instant::now() >= t) {
            mox.store(false, Ordering::Relaxed);
            self.active = false;
            self.until = None;
        }
    }
}
