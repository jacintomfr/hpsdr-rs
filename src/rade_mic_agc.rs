/*
    Mic-side leveler + compressor/limiter for RADE V1 TX, modeled on
    FreeDV's own two-stage mic conditioning -- AgcStep (a loudness-riding
    leveler) followed by a soft-knee compressor/limiter -- from
    tmiw/freedv-backend (https://github.com/tmiw/freedv-backend,
    BSD-2-Clause, Mooneer Salem), the shared backend the reference FreeDV
    GUI (drowe67/freedv-gui) now pulls this logic from. Combined here under
    the hpsdr-rs project, whose GPL-2.0-or-later license permits combining
    BSD-2-Clause code freely (no relicensing obligation either way).

    Deliberately NOT a literal port -- two real differences from upstream,
    both because the exact reference implementation is either unavailable
    here or overkill for what this buys:

    1. Leveler loudness measurement: the reference measures true EBU R128
       (K-weighted) short-term loudness via libebur128. This uses a plain
       RMS level in dBFS instead -- no K-weighting filter. The GAIN-RIDING
       logic around that measurement (target level, +12/-20 dB limits,
       0.5s attack / 6s release time constants, -33 dBFS silence gate that
       freezes the gain rather than chasing noise) is the same as
       AgcStep.cpp's real shipped constants, not the older "1 dB/s, ±12dB
       symmetric" description this was originally asked for -- confirmed
       by reading that file directly rather than going from memory/docs.
    2. Compressor/limiter: the reference calls WebRTC's legacy fixed-point
       analog-AGC module purely as a limiter (compressionGaindB=0,
       limiterEnable=1), whose internal soft-knee gain-table generator is
       real but not worth porting verbatim (fixed-point, its own
       fast/slow leaky-integrator envelope pair, and its 3:1 ratio /
       ~250-frame decay are closer to hyperparameters of that specific
       implementation than something meaningful to reproduce bit-exact).
       This reimplements the INTENT instead: a floating-point soft-knee
       compressor at that same real 3:1 ratio blended into a limiter at
       that same real -1 dBFS ceiling, with an ordinary attack/release
       envelope follower.

    Applied to raw microphone audio ONLY (RadeHandle::fill_tx's own
    `mic` parameter, before RadeWorker::push_mic ever sees it) -- never to
    RADE's already-modulated tone output. That distinction is the whole
    reason this exists as separate controls from the ordinary WDSP TX
    Leveler/Compressor/CFC (Settings -> TX): those operate on whatever is
    in the TX audio chain at their own point in TxProcessor::process, which
    for an armed RADE over is the modulated modem waveform, not speech --
    running a voice-tuned dynamics processor on that (a real report: the
    carrier "oscillated" and broke RX at the far end) is a different bug
    class entirely from what this file is for.
*/

// ── Leveler ──────────────────────────────────────────────────────────────

const AGC_TARGET_DBFS: f32 = -23.0;
const AGC_MAX_GAIN_DB: f32 = 12.0;
const AGC_MIN_GAIN_DB: f32 = -20.0;
const AGC_ATTACK_SEC: f32 = 0.5;
const AGC_RELEASE_SEC: f32 = 6.0;
/// Below this measured level the gain is frozen rather than chased --
/// AgcStep's own silence gate, so a gap between words (or the mic
/// picking up nothing at all) doesn't get "corrected" by riding the gain
/// up toward the noise floor.
const AGC_SILENCE_DBFS: f32 = -33.0;

/// Loudness-riding leveler -- see this file's own module doc comment.
/// One instance per RADE TX chain; call [`RadeLeveler::process`] once per
/// audio block while armed, [`RadeLeveler::reset`] on disarm (matches
/// AgcStep's own `reset()`, called on a mode change -- an over is exactly
/// that boundary here).
pub struct RadeLeveler {
    current_gain_db: f32,
}

impl RadeLeveler {
    pub fn new() -> Self {
        RadeLeveler { current_gain_db: 0.0 }
    }

    pub fn reset(&mut self) {
        self.current_gain_db = 0.0;
    }

    /// Ride `block`'s gain toward AGC_TARGET_DBFS in place, at `sample_rate`.
    pub fn process(&mut self, block: &mut [f32], sample_rate: f64) {
        if block.is_empty() || sample_rate <= 0.0 {
            return;
        }
        let mean_sq = block.iter().map(|&s| s * s).sum::<f32>() / block.len() as f32;
        let rms = mean_sq.sqrt().max(1e-9);
        let level_dbfs = 20.0 * rms.log10();

        if level_dbfs > AGC_SILENCE_DBFS {
            let target_gain_db =
                (AGC_TARGET_DBFS - level_dbfs).clamp(AGC_MIN_GAIN_DB, AGC_MAX_GAIN_DB);
            // Matches AgcStep.cpp exactly: attack (the faster 0.5s
            // constant) is used when the target is BELOW the current gain
            // -- i.e. the input just got louder and the gain needs to come
            // down quickly to protect against it; release (the slower 6s
            // constant) is used recovering back up after a quiet spell.
            let interval =
                if target_gain_db < self.current_gain_db { AGC_ATTACK_SEC } else { AGC_RELEASE_SEC };
            let step = (target_gain_db - self.current_gain_db) / interval
                * (block.len() as f32 / sample_rate as f32);
            self.current_gain_db = (self.current_gain_db + step).clamp(AGC_MIN_GAIN_DB, AGC_MAX_GAIN_DB);
        }
        // Below the silence gate: current_gain_db is left exactly where it
        // was, same "freeze" semantics as the reference.

        let scale = 10f32.powf(self.current_gain_db / 20.0);
        for s in block.iter_mut() {
            *s = (*s * scale).clamp(-1.0, 1.0);
        }
    }
}

impl Default for RadeLeveler {
    fn default() -> Self {
        Self::new()
    }
}

// ── Compressor/limiter ──────────────────────────────────────────────────

/// Where soft-knee compression starts biting.
const COMP_THRESHOLD_DBFS: f32 = -12.0;
/// The reference's real ratio (WebRTC AGC's `kCompRatio`), not the 2:1
/// this was originally described as.
const COMP_RATIO: f32 = 3.0;
/// Width of the soft transition into full-ratio compression, centred on
/// COMP_THRESHOLD_DBFS.
const COMP_KNEE_DB: f32 = 6.0;
/// Hard ceiling -- the reference's own `LIMITER_LEVEL_DB` (-1 dBFS), not
/// the analytical 0 dBFS a plain limiter would otherwise imply.
const COMP_CEILING_DBFS: f32 = -1.0;
const COMP_ATTACK_MS: f32 = 5.0;
/// The reference's own decay figure ("kAvgDecayTime = 250").
const COMP_RELEASE_MS: f32 = 250.0;

/// Soft-knee compressor blended into a hard limiter at the ceiling -- see
/// this file's own module doc comment for how this differs from the
/// reference it is modeled on. One instance per RADE TX chain; call
/// [`RadeCompressor::process`] once per audio block while armed (AFTER
/// [`RadeLeveler::process`], same order as the reference's own pipeline:
/// level to a consistent target first, then guard the peaks).
pub struct RadeCompressor {
    /// Smoothed input level, dBFS.
    envelope_db: f32,
}

impl RadeCompressor {
    pub fn new() -> Self {
        RadeCompressor { envelope_db: -90.0 }
    }

    pub fn reset(&mut self) {
        self.envelope_db = -90.0;
    }

    pub fn process(&mut self, block: &mut [f32], sample_rate: f64) {
        if block.is_empty() || sample_rate <= 0.0 {
            return;
        }
        let sr = sample_rate as f32;
        // Per-sample one-pole envelope follower -- ordinary attack/release
        // time constants (not the reference's own fast/slow leaky-
        // integrator pair, which is specific to WebRTC's fixed-point
        // implementation and not meaningfully more correct for this).
        let attack_coef = (-1.0f32 / (COMP_ATTACK_MS / 1000.0 * sr)).exp();
        let release_coef = (-1.0f32 / (COMP_RELEASE_MS / 1000.0 * sr)).exp();

        for s in block.iter_mut() {
            let in_db = 20.0 * s.abs().max(1e-9).log10();
            let coef = if in_db > self.envelope_db { attack_coef } else { release_coef };
            self.envelope_db = coef * self.envelope_db + (1.0 - coef) * in_db;

            // Soft-knee compression: a quadratic blend from 0 dB reduction
            // (a knee-width below threshold) to the full ratio (a knee-
            // width above it), matching the standard soft-knee formula.
            let over = self.envelope_db - COMP_THRESHOLD_DBFS;
            let half_knee = COMP_KNEE_DB / 2.0;
            let comp_reduction_db = if over <= -half_knee {
                0.0
            } else if over >= half_knee {
                over * (1.0 - 1.0 / COMP_RATIO)
            } else {
                let x = over + half_knee;
                (x * x) / (2.0 * COMP_KNEE_DB) * (1.0 - 1.0 / COMP_RATIO)
            };

            // Limiter: whatever the compressor left above the ceiling gets
            // clamped there outright, same "soft ratio, hard ceiling"
            // shape as the reference.
            let post_comp_db = self.envelope_db - comp_reduction_db;
            let over_ceiling_db = (post_comp_db - COMP_CEILING_DBFS).max(0.0);

            let total_reduction_db = comp_reduction_db + over_ceiling_db;
            let gain = 10f32.powf(-total_reduction_db / 20.0);
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

impl Default for RadeCompressor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Equalizer ────────────────────────────────────────────────────────────

/// Clamp limits matching freedv-gui's own `dlg_filter.cpp` sliders
/// (`FilterDlg::ExchangeData`/`OnMicInDefault`) exactly, so a value typed
/// here behaves the same as the familiar FreeDV GUI control.
pub const EQ_BASS_FREQ_MAX_HZ: f32 = 600.0;
pub const EQ_TREBLE_FREQ_MAX_HZ: f32 = 3900.0;
pub const EQ_MID_FREQ_MAX_HZ: f32 = 3900.0;
pub const EQ_GAIN_MIN_DB: f32 = -20.0;
pub const EQ_GAIN_MAX_DB: f32 = 20.0;
pub const EQ_Q_MIN: f32 = 0.1;
pub const EQ_Q_MAX: f32 = 10.0;

/// freedv-gui's own "Default" button values (`FilterDlg::OnMicInDefault`):
/// bass shelf at 100Hz, mid peak at 1500Hz/Q=1, treble shelf at 3000Hz, all
/// at 0dB (i.e. the EQ starts as a no-op until the user shapes it).
pub const EQ_DEFAULT_BASS_FREQ_HZ: f32 = 100.0;
pub const EQ_DEFAULT_MID_FREQ_HZ: f32 = 1500.0;
pub const EQ_DEFAULT_MID_Q: f32 = 1.0;
pub const EQ_DEFAULT_TREBLE_FREQ_HZ: f32 = 3000.0;

/// A single RBJ Audio-EQ-Cookbook biquad (Direct Form I), used for the
/// bass/treble shelves and the mid peaking band -- the same three filter
/// shapes freedv-gui builds via `sox_biquad_create("bass"/"treble"/
/// "equalizer", ...)` in `eq.cpp`'s `designAnEQFilter`, reimplemented
/// directly since hpsdr-rs has no SoX dependency. Coefficients come from
/// the standard RBJ cookbook (https://www.w3.org/-- widely published,
/// also what SoX's own shelving/peaking filters are derived from), not a
/// byte-for-byte port of SoX's internals.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn identity() -> Self {
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn low_shelf(freq_hz: f32, gain_db: f32, sample_rate: f64) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate as f32;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / 2.0 * 2f32.sqrt();
        let sqrt_a = a.sqrt();
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
        Biquad {
            b0: a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha) / a0,
            b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0) / a0,
            b2: a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha) / a0,
            a1: -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0) / a0,
            a2: ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn high_shelf(freq_hz: f32, gain_db: f32, sample_rate: f64) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate as f32;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / 2.0 * 2f32.sqrt();
        let sqrt_a = a.sqrt();
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
        Biquad {
            b0: a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha) / a0,
            b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0) / a0,
            b2: a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha) / a0,
            a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0) / a0,
            a2: ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn peaking(freq_hz: f32, gain_db: f32, q: f32, sample_rate: f64) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate as f32;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q.max(0.01));
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos_w0 / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: (1.0 - alpha / a) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Replace this biquad's coefficients in place, keeping its x1/x2/y1/y2
    /// history -- so a parameter redesign between blocks doesn't click
    /// (which a fresh `Biquad::low_shelf(...)`/etc. with zeroed history
    /// would, since the filter's memory of the signal so far would be
    /// discarded on every single block).
    fn set_coeffs(&mut self, src: Biquad) {
        self.b0 = src.b0;
        self.b1 = src.b1;
        self.b2 = src.b2;
        self.a1 = src.a1;
        self.a2 = src.a2;
    }

    #[inline]
    fn process_sample(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Plain-data snapshot of an eq's band settings -- what gets passed
/// across the lock boundary (`TxHandle::rade_eq`/`set_rade_eq`) and
/// persisted to `Config`, since [`RadeEqualizer`] itself also carries
/// live filter state that has no business being serialized or copied.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct RadeEqParams {
    pub bass_freq_hz: f32,
    pub bass_gain_db: f32,
    pub mid_freq_hz: f32,
    pub mid_gain_db: f32,
    pub mid_q: f32,
    pub treble_freq_hz: f32,
    pub treble_gain_db: f32,
    pub vol_gain_db: f32,
}

impl Default for RadeEqParams {
    fn default() -> Self {
        RadeEqParams {
            bass_freq_hz: EQ_DEFAULT_BASS_FREQ_HZ,
            bass_gain_db: 0.0,
            mid_freq_hz: EQ_DEFAULT_MID_FREQ_HZ,
            mid_gain_db: 0.0,
            mid_q: EQ_DEFAULT_MID_Q,
            treble_freq_hz: EQ_DEFAULT_TREBLE_FREQ_HZ,
            treble_gain_db: 0.0,
            vol_gain_db: 0.0,
        }
    }
}

/// Three-band mic EQ (bass shelf + mid peak + treble shelf) plus an
/// overall Vol trim, matching freedv-gui's own mic-in equalizer band for
/// band (`eq.cpp`/`dlg_filter.cpp`) -- same filter shapes, same default
/// center frequencies, applied at the same point in the TX chain (raw
/// mic audio, generically ahead of the codec/modem, not RADE-specific --
/// see this file's own module doc comment). One instance per RADE TX
/// chain; call [`RadeEqualizer::process`] once per audio block while
/// armed, [`RadeEqualizer::reset`] on disarm.
pub struct RadeEqualizer {
    pub bass_freq_hz: f32,
    pub bass_gain_db: f32,
    pub mid_freq_hz: f32,
    pub mid_gain_db: f32,
    pub mid_q: f32,
    pub treble_freq_hz: f32,
    pub treble_gain_db: f32,
    pub vol_gain_db: f32,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
}

impl RadeEqualizer {
    pub fn new() -> Self {
        RadeEqualizer {
            bass_freq_hz: EQ_DEFAULT_BASS_FREQ_HZ,
            bass_gain_db: 0.0,
            mid_freq_hz: EQ_DEFAULT_MID_FREQ_HZ,
            mid_gain_db: 0.0,
            mid_q: EQ_DEFAULT_MID_Q,
            treble_freq_hz: EQ_DEFAULT_TREBLE_FREQ_HZ,
            treble_gain_db: 0.0,
            vol_gain_db: 0.0,
            bass: Biquad::identity(),
            mid: Biquad::identity(),
            treble: Biquad::identity(),
        }
    }

    /// Clears filter history (not the band settings) -- call on disarm so
    /// a new over doesn't carry over stale filter state across a gap.
    pub fn reset(&mut self) {
        self.bass.reset();
        self.mid.reset();
        self.treble.reset();
    }

    pub fn params(&self) -> RadeEqParams {
        RadeEqParams {
            bass_freq_hz: self.bass_freq_hz,
            bass_gain_db: self.bass_gain_db,
            mid_freq_hz: self.mid_freq_hz,
            mid_gain_db: self.mid_gain_db,
            mid_q: self.mid_q,
            treble_freq_hz: self.treble_freq_hz,
            treble_gain_db: self.treble_gain_db,
            vol_gain_db: self.vol_gain_db,
        }
    }

    pub fn set_params(&mut self, p: RadeEqParams) {
        self.bass_freq_hz = p.bass_freq_hz.clamp(1.0, EQ_BASS_FREQ_MAX_HZ);
        self.bass_gain_db = p.bass_gain_db.clamp(EQ_GAIN_MIN_DB, EQ_GAIN_MAX_DB);
        self.mid_freq_hz = p.mid_freq_hz.clamp(1.0, EQ_MID_FREQ_MAX_HZ);
        self.mid_gain_db = p.mid_gain_db.clamp(EQ_GAIN_MIN_DB, EQ_GAIN_MAX_DB);
        self.mid_q = p.mid_q.clamp(EQ_Q_MIN, EQ_Q_MAX);
        self.treble_freq_hz = p.treble_freq_hz.clamp(1.0, EQ_TREBLE_FREQ_MAX_HZ);
        self.treble_gain_db = p.treble_gain_db.clamp(EQ_GAIN_MIN_DB, EQ_GAIN_MAX_DB);
        self.vol_gain_db = p.vol_gain_db.clamp(EQ_GAIN_MIN_DB, EQ_GAIN_MAX_DB);
    }

    /// Apply bass/mid/treble/vol to `block` in place, at `sample_rate`.
    /// Redesigns the three biquads from the current band settings every
    /// call -- this runs once per audio block (not per sample), so the
    /// cost is negligible, and it keeps a UI slider edit taking effect on
    /// the very next block with no separate "dirty" bookkeeping.
    pub fn process(&mut self, block: &mut [f32], sample_rate: f64) {
        if block.is_empty() || sample_rate <= 0.0 {
            return;
        }
        self.bass.set_coeffs(Biquad::low_shelf(self.bass_freq_hz, self.bass_gain_db, sample_rate));
        self.mid.set_coeffs(Biquad::peaking(self.mid_freq_hz, self.mid_gain_db, self.mid_q, sample_rate));
        self.treble.set_coeffs(Biquad::high_shelf(self.treble_freq_hz, self.treble_gain_db, sample_rate));
        let vol_scale = 10f32.powf(self.vol_gain_db / 20.0);
        for s in block.iter_mut() {
            let y = self.bass.process_sample(*s);
            let y = self.mid.process_sample(y);
            let y = self.treble.process_sample(y);
            *s = (y * vol_scale).clamp(-1.0, 1.0);
        }
    }
}

impl Default for RadeEqualizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A steady tone well above the leveler's target should have its gain
    /// pulled DOWN over time (using the faster attack interval), settling
    /// at the floor since a 0 dBFS tone is far louder than even
    /// AGC_MIN_GAIN_DB can bring down to the -23 dBFS target.
    #[test]
    fn leveler_rides_a_loud_steady_tone_down() {
        let rate = 48_000.0;
        let mut lev = RadeLeveler::new();
        let block_len = (rate * 0.01) as usize; // 10ms blocks
        for i in 0..300 {
            // 0 dBFS sine -- far above the -23 dBFS target.
            let mut block: Vec<f32> = (0..block_len)
                .map(|n| ((i * block_len + n) as f32 * 0.05).sin())
                .collect();
            lev.process(&mut block, rate);
        }
        // 300 * 10ms = 3s, comfortably past the 0.5s attack constant, so
        // the gain should have settled at the floor by now -- a tolerance
        // wide enough for the last block's phase-dependent RMS jitter
        // right at the clamp boundary, not a claim of exact convergence.
        assert!(
            (lev.current_gain_db - AGC_MIN_GAIN_DB).abs() < 0.5,
            "expected the gain to settle at the floor ({AGC_MIN_GAIN_DB}), got {}",
            lev.current_gain_db
        );
    }

    /// Genuine silence must not be chased -- the gain should stay exactly
    /// where it started (the silence gate freezes it).
    #[test]
    fn leveler_freezes_on_silence() {
        let rate = 48_000.0;
        let mut lev = RadeLeveler::new();
        let mut block = vec![0.0f32; 480];
        lev.process(&mut block, rate);
        assert_eq!(lev.current_gain_db, 0.0, "silence must not move the gain from its start");
    }

    /// Output never exceeds full scale regardless of input level.
    #[test]
    fn leveler_output_is_always_in_range() {
        let rate = 48_000.0;
        let mut lev = RadeLeveler::new();
        let mut block: Vec<f32> = (0..480).map(|n| (n as f32 * 0.3).sin() * 5.0).collect();
        lev.process(&mut block, rate);
        assert!(block.iter().all(|&s| (-1.0..=1.0).contains(&s)));
    }

    /// A full-scale tone must come out at or below the compressor's own
    /// ceiling, never above it.
    #[test]
    fn compressor_limits_to_the_ceiling() {
        let rate = 48_000.0;
        let mut comp = RadeCompressor::new();
        let ceiling = 10f32.powf(COMP_CEILING_DBFS / 20.0);
        for i in 0..50 {
            let mut block: Vec<f32> =
                (0..480).map(|n| ((i * 480 + n) as f32 * 0.3).sin()).collect();
            comp.process(&mut block, rate);
            if i > 10 {
                // Give the envelope a moment to settle before asserting.
                assert!(
                    block.iter().all(|&s| s.abs() <= ceiling + 1e-3),
                    "block {i} exceeded the ceiling"
                );
            }
        }
    }

    /// A quiet signal, well under the threshold, should pass through
    /// essentially unchanged (no gain reduction applied).
    #[test]
    fn compressor_leaves_quiet_signal_alone() {
        let rate = 48_000.0;
        let mut comp = RadeCompressor::new();
        let mut block: Vec<f32> = (0..480).map(|n| (n as f32 * 0.3).sin() * 0.05).collect();
        let original = block.clone();
        comp.process(&mut block, rate);
        for (o, p) in original.iter().zip(block.iter()) {
            assert!((o - p).abs() < 0.01, "quiet signal should be left nearly untouched");
        }
    }

    #[test]
    fn reset_returns_to_unity() {
        let rate = 48_000.0;
        let mut lev = RadeLeveler::new();
        let mut block: Vec<f32> = (0..4800).map(|n| (n as f32 * 0.05).sin()).collect();
        lev.process(&mut block, rate);
        assert_ne!(lev.current_gain_db, 0.0);
        lev.reset();
        assert_eq!(lev.current_gain_db, 0.0);
    }

    /// At freedv-gui's own all-0dB defaults the EQ should be very close to
    /// a no-op (the three biquads all reduce to near-identity filters).
    #[test]
    fn eq_at_default_gains_is_near_identity() {
        let rate = 48_000.0;
        let mut eq = RadeEqualizer::new();
        let original: Vec<f32> = (0..480).map(|n| (n as f32 * 0.1).sin() * 0.5).collect();
        let mut block = original.clone();
        // Run a couple of blocks so any filter-startup transient settles.
        eq.process(&mut block.clone(), rate);
        eq.process(&mut block, rate);
        for (o, p) in original.iter().zip(block.iter()) {
            assert!((o - p).abs() < 0.05, "expected near-identity at 0dB gains, got {o} vs {p}");
        }
    }

    /// Boosting the bass band should raise the energy of a low-frequency
    /// tone relative to leaving the EQ flat.
    #[test]
    fn eq_bass_boost_raises_a_low_tone() {
        let rate = 48_000.0;
        let tone: Vec<f32> = (0..4800).map(|n| (2.0 * std::f32::consts::PI * 80.0 * n as f32 / rate as f32).sin()).collect();

        let mut flat = RadeEqualizer::new();
        let mut flat_block = tone.clone();
        flat.process(&mut flat_block, rate);
        let flat_rms = (flat_block.iter().map(|&s| s * s).sum::<f32>() / flat_block.len() as f32).sqrt();

        let mut boosted = RadeEqualizer::new();
        boosted.bass_gain_db = 12.0;
        let mut boosted_block = tone;
        boosted.process(&mut boosted_block, rate);
        let boosted_rms =
            (boosted_block.iter().map(|&s| s * s).sum::<f32>() / boosted_block.len() as f32).sqrt();

        assert!(boosted_rms > flat_rms, "bass boost should raise an 80Hz tone's level");
    }

    /// Vol gain is a plain dB scalar on top of the shaped bands.
    #[test]
    fn eq_vol_gain_scales_output() {
        let rate = 48_000.0;
        let mut eq = RadeEqualizer::new();
        eq.vol_gain_db = -6.0;
        let mut block: Vec<f32> = vec![0.1; 480];
        eq.process(&mut block, rate);
        let expected = 0.1 * 10f32.powf(-6.0 / 20.0);
        assert!((block[400] - expected).abs() < 0.01);
    }

    #[test]
    fn eq_params_round_trip_through_set_params() {
        let mut eq = RadeEqualizer::new();
        let p = RadeEqParams {
            bass_freq_hz: 150.0,
            bass_gain_db: 3.0,
            mid_freq_hz: 1200.0,
            mid_gain_db: -2.0,
            mid_q: 2.0,
            treble_freq_hz: 3500.0,
            treble_gain_db: 4.0,
            vol_gain_db: 1.0,
        };
        eq.set_params(p);
        let got = eq.params();
        assert_eq!(got.bass_freq_hz, 150.0);
        assert_eq!(got.mid_q, 2.0);
        assert_eq!(got.vol_gain_db, 1.0);
    }
}
