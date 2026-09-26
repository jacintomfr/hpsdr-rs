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
}
