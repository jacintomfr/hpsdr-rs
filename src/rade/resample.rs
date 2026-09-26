// Adapted from SDRoxide (https://github.com/madmedicnl/sdroxide,
// crates/sdroxide-dsp/src/resample.rs, upstream commit 63b0b29), licensed
// GPL-3.0-or-later. Combined here under the hpsdr-rs project, whose own
// GPL-2.0-or-later license permits combination with GPL-3.0 code (the
// combined work is GPL-3.0).
//
// Local changes from upstream: only `MonoResampler` is ported -- worker.rs
// (this module's only caller) needs nothing but mono conversion at its
// four sample-rate boundaries (tap-rate<->8kHz, 16kHz<->audio-rate).
// Upstream's `StereoResampler`/`ComplexResampler` (used elsewhere in
// SDRoxide for stereo audio and raw IQ) are not needed here and are not
// ported. Otherwise unchanged.

//! Mono audio resampler (rubato) for RADE V1's clock-domain boundaries --
//! the engine's demod-tap rate to/from RADE's fixed 8kHz modem rate, and
//! RADE's fixed 16kHz speech rate to/from the app's own audio rate.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};

const CHUNK: usize = 1024;

pub struct MonoResampler {
    inner: Async<f32>,
    pending: Vec<f32>,
}

impl MonoResampler {
    /// `None` when the rates already match (within 0.01 Hz).
    pub fn new(in_rate: f64, out_rate: f64) -> Option<Self> {
        if (in_rate - out_rate).abs() < 0.01 {
            return None;
        }
        let inner = Async::new_poly(
            out_rate / in_rate,
            1.1,
            PolynomialDegree::Septic,
            CHUNK,
            1,
            FixedAsync::Input,
        )
        .expect("resampler construction");
        Some(MonoResampler { inner, pending: Vec::new() })
    }

    /// Feed input samples; appends resampled output to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);
        while self.pending.len() >= CHUNK {
            let adapter = InterleavedSlice::new(&self.pending[..CHUNK], 1, CHUNK).expect("adapter");
            let produced = self.inner.process(&adapter, None).expect("resample");
            out.extend_from_slice(&produced.take_data());
            self.pending.drain(..CHUNK);
        }
    }
}
