// Bindings to and a safe wrapper over RNNoise (vendor/rnnoise, a git
// submodule -- https://github.com/xiph/rnnoise, BSD-3-Clause), built by
// build_rnnoise.rs. Combined here under the hpsdr-rs project, whose own
// GPL-2.0-or-later license permits combination with BSD-3-Clause code
// freely (no relicensing obligation either way).
//
// This is the noise-reduction stage FreeDV's own mic pipeline applies
// before the leveler/compressor (tmiw/freedv-backend's RNNoiseStep, same
// library, same convention -- confirmed by reading that source directly).
// Wired into hpsdr-rs's own RADE mic chain (tx.rs, alongside
// rade_mic_agc.rs's Leveler/Compressor) as a separate, RADE-only control,
// same reasoning as those: it belongs on the raw microphone, before the
// vocoder ever sees it, never on the modulated RF.

use std::os::raw::c_float;

// Hand-written rather than bindgen-generated: RNNoise's public API
// (rnnoise.h) is five functions and two opaque types, not worth a whole
// build-time codegen pass (and the extra libclang dependency that comes
// with it -- see build_rade.rs's own doc comment for what that already
// costs) for something this small.
#[repr(C)]
struct DenoiseState {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn rnnoise_get_frame_size() -> i32;
    fn rnnoise_create(model: *mut std::os::raw::c_void) -> *mut DenoiseState;
    fn rnnoise_destroy(st: *mut DenoiseState);
    fn rnnoise_process_frame(st: *mut DenoiseState, out: *mut c_float, input: *const c_float) -> c_float;
}

/// RNNoise's fixed frame size (480 samples = 10ms at its native 48kHz).
pub fn frame_size() -> usize {
    // Safety: no arguments, no state -- a pure query.
    unsafe { rnnoise_get_frame_size() as usize }
}

/// Streaming wrapper: feed arbitrary-sized blocks via [`RadeDenoiser::process`],
/// which buffers internally to RNNoise's own fixed frame size. `Send`, not
/// `Sync` -- one instance belongs to one thread (hpsdr-rs's TX mic-pacing
/// thread, alongside the Leveler/Compressor it runs next to).
pub struct RadeDenoiser {
    st: *mut DenoiseState,
    frame: usize,
    /// Samples awaiting a full RNNoise frame, in RNNoise's own int16-range
    /// float convention (see `process`'s own doc comment).
    pending: Vec<f32>,
    /// Denoised samples produced but not yet returned to the caller --
    /// RNNoise only ever hands back whole frames, which rarely line up
    /// with the caller's own block size.
    out_buf: Vec<f32>,
}

// Safety: `DenoiseState` owns only its own heap state (weights + filter
// history) and has no thread affinity.
unsafe impl Send for RadeDenoiser {}

impl RadeDenoiser {
    /// Uses the default (built-in) trained model -- `rnnoise_create(NULL)`.
    pub fn new() -> Self {
        // Safety: NULL selects the built-in model, matching rnnoise.h's own
        // documented convention; the returned pointer is owned and freed in
        // Drop.
        let st = unsafe { rnnoise_create(std::ptr::null_mut()) };
        assert!(!st.is_null(), "rnnoise_create returned NULL");
        RadeDenoiser { st, frame: frame_size(), pending: Vec::new(), out_buf: Vec::new() }
    }

    /// Denoise `block` in place (full-scale-normalised audio, `+/-1.0`).
    ///
    /// RNNoise itself works in int16-range floats (`+/-32768`, matching its
    /// own reference examples -- a direct `short` -> `float` cast, no
    /// division), so samples are scaled up before processing and back down
    /// after. Buffers internally to RNNoise's fixed 480-sample frame size,
    /// so a block shorter than one frame may see no output yet (fine for a
    /// continuous streaming caller -- see rade_mic_agc.rs's own callers,
    /// which run at a small, steady block size every TX chunk) and any
    /// remainder above a whole number of frames is carried to the next
    /// call rather than dropped or padded.
    pub fn process(&mut self, block: &mut [f32]) {
        if block.is_empty() {
            return;
        }
        self.pending.extend(block.iter().map(|&s| s * 32_768.0));

        let mut frame_buf = vec![0.0f32; self.frame];
        while self.pending.len() >= self.frame {
            // Safety: `self.st` is a valid, owned handle; `frame_buf` and
            // the drained slice of `pending` are both exactly `self.frame`
            // long, matching what rnnoise_process_frame requires.
            unsafe {
                rnnoise_process_frame(
                    self.st,
                    frame_buf.as_mut_ptr(),
                    self.pending[..self.frame].as_ptr(),
                );
            }
            self.out_buf.extend_from_slice(&frame_buf);
            self.pending.drain(..self.frame);
        }

        // Hand back exactly `block.len()` samples, scaled back to
        // full-scale-normalised range -- whatever isn't ready yet stays in
        // out_buf for the next call, same "steady real-time pacing, not
        // bursty" reasoning as spectrum.rs's own RADE RX jitter buffer.
        let n = block.len().min(self.out_buf.len());
        for (dst, src) in block.iter_mut().zip(self.out_buf.drain(..n)) {
            *dst = (src / 32_768.0).clamp(-1.0, 1.0);
        }
        for dst in block.iter_mut().skip(n) {
            *dst = 0.0;
        }
    }
}

impl Drop for RadeDenoiser {
    fn drop(&mut self) {
        // Safety: `self.st` came from rnnoise_create and is destroyed once.
        unsafe { rnnoise_destroy(self.st) };
    }
}

impl Default for RadeDenoiser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_is_480() {
        assert_eq!(frame_size(), 480);
    }

    /// A block shorter than one RNNoise frame produces no output yet
    /// (silence, not garbage) and doesn't panic.
    #[test]
    fn short_block_yields_silence_until_a_full_frame_accumulates() {
        let mut den = RadeDenoiser::new();
        let mut block = vec![0.1f32; 100];
        den.process(&mut block);
        assert!(block.iter().all(|&s| s == 0.0), "no full frame yet -- should be silence");
    }

    /// Once enough samples have accumulated, real (non-panicking, in-range)
    /// output comes back.
    #[test]
    fn a_full_frame_of_audio_round_trips_in_range() {
        let mut den = RadeDenoiser::new();
        // Several frames of a simple tone -- enough to exercise both the
        // "accumulate" and "drain" paths across multiple process() calls.
        for i in 0..10 {
            let mut block: Vec<f32> =
                (0..200).map(|n| ((i * 200 + n) as f32 * 0.05).sin() * 0.3).collect();
            den.process(&mut block);
            assert!(block.iter().all(|&s| (-1.0..=1.0).contains(&s)), "output out of range");
        }
    }

    /// Silence in should stay silence-ish out (RNNoise shouldn't invent a
    /// loud signal from nothing).
    #[test]
    fn silence_stays_quiet() {
        let mut den = RadeDenoiser::new();
        for _ in 0..20 {
            let mut block = vec![0.0f32; 480];
            den.process(&mut block);
            let peak = block.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
            assert!(peak < 0.05, "silence produced an unexpectedly loud output ({peak})");
        }
    }
}
