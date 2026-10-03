// Adapted from SDRoxide (https://github.com/madmedicnl/sdroxide,
// crates/sdroxide-rade/src/lib.rs, upstream commit 63b0b29), licensed
// GPL-3.0-or-later. Combined here under the hpsdr-rs project, whose own
// GPL-2.0-or-later license permits combination with GPL-3.0 code (the
// combined work is GPL-3.0).
//
// Local changes from upstream: this is a directory module
// (`crate::rade`, `src/rade/mod.rs` + siblings) rather than its own crate
// -- upstream's `sdroxide-rade` is a separate crate in a Cargo workspace,
// which this project (a single binary crate, not a workspace) has no
// equivalent of. `sys`/`vocoder`/`worker`/`text` are therefore submodules
// of this one instead of sibling crate-root modules, with import paths
// adjusted accordingly (`crate::X` -> `super::X` inside each submodule).
// The C library itself (vendor/rade_c, a git submodule) and its build
// (build_rade.rs) are unchanged apart from what that file's own doc
// comment describes. The Rust logic in every submodule is otherwise
// unchanged from upstream.

//! Safe Rust wrapper around FreeDV **RADE V1** (Radio Autoencoder) -- a
//! neural speech codec plus OFDM waveform that carries intelligible voice
//! at SNRs where SSB is not usable.
//!
//! The C library is vendored at `vendor/rade_c` (upstream
//! <https://github.com/freedv/rade_c>, a git submodule -- see README for
//! the `git submodule update --init` step a fresh clone needs) and built
//! by `build_rade.rs`.
//!
//! Three layers live here:
//!
//! * [`Rade`] -- the modem: FARGAN feature vectors <-> 8 kHz complex modem IQ.
//! * [`vocoder::VocoderEnc`] / [`vocoder::VocoderDec`] -- 16 kHz speech <->
//!   those feature vectors, via Opus's FARGAN/LPCNet. RADE's own API
//!   deliberately stops short of speech, so this half is needed for
//!   anything end-to-end.
//! * [`RadeWorker`] -- both of the above driven on a dedicated thread with
//!   ring buffers at the edges, which is how main.rs's audio pipeline uses
//!   them (see rade_link.rs).

mod resample;
mod sys;
pub mod text;
pub mod vocoder;
mod worker;

pub use vocoder::{VocoderDec, VocoderEnc};
pub use worker::{RadeStats, RadeTextRx, RadeWorker};

use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use num_complex::Complex32;

/// Modem (IQ) sample rate -- `RADE_MODEM_SAMPLE_RATE`.
pub const MODEM_RATE: f64 = 8_000.0;
/// Speech sample rate -- `RADE_SPEECH_SAMPLE_RATE`.
pub const SPEECH_RATE: f64 = 16_000.0;

/// int16 <-> float scaling the C library documents: nominal unit IQ amplitude
/// maps to 16384, leaving 6 dB of headroom.
pub const INT16_SCALE: f32 = 16_384.0;

/// Multiply full-scale-normalised real audio (`+/-1.0`, i.e. int16/32768) by
/// this before handing it to [`Rade::rx`].
///
/// `rade_api.h` specifies real-valued RX input as `int16 * (2.0 /
/// RADE_INT16_SCALE)`; the factor of two compensates for `Re{}` halving the
/// positive-frequency component. Our audio is `int16 / 32768`, so the
/// combined factor is `32768 * 2 / 16384`.
pub const RX_REAL_SCALE: f32 = 4.0;

/// Multiply `Re{}` of [`Rade::tx`] output by this to get full-scale-normalised
/// real audio: `RADE_INT16_SCALE / 32768`.
pub const TX_REAL_SCALE: f32 = INT16_SCALE / 32_768.0;

/// Full-scale-normalised audio to the int16 the vocoder consumes.
///
/// Matches rade_c's conversion exactly -- scale by 32768, clamp to `+/-32767`,
/// round half up. The vocoder's output is sensitive to this: a one-LSB shift
/// early in an over feeds back through the encoder and the two signals diverge
/// completely within a few frames.
#[inline]
pub fn f32_to_i16(s: f32) -> i16 {
    let v = (s * 32_768.0).clamp(-32_767.0, 32_767.0);
    (0.5 + v as f64).floor() as i16
}

/// Errors crossing the C boundary.
#[derive(Debug, thiserror::Error)]
pub enum RadeError {
    /// A [`Rade`] is already live in this process. The C library documents a
    /// single context per process, so a second one is refused rather than
    /// risking shared state.
    #[error("a RADE instance is already open in this process")]
    AlreadyOpen,
    /// `rade_open()` returned NULL.
    #[error("rade_open failed")]
    OpenFailed,
    /// A caller-supplied buffer was not the length the C API requires.
    #[error("expected {want} samples, got {got}")]
    BadLength { want: usize, got: usize },
    /// `rade_rx()` reported failure.
    #[error("rade_rx failed ({0})")]
    Rx(i32),
    /// The vocoder could not be constructed.
    #[error("vocoder allocation failed")]
    VocoderAlloc,
}

static INIT: Once = Once::new();
static OPEN: AtomicBool = AtomicBool::new(false);

/// One RADE V1 context: one transmitter and one receiver.
///
/// `Send` but deliberately **not** `Sync` -- one instance belongs to one
/// thread, which for hpsdr-rs is [`RadeWorker`]'s thread.
pub struct Rade {
    r: *mut sys::rade,
    n_features: usize,
    n_tx_out: usize,
    n_tx_eoo_out: usize,
    nin_max: usize,
    n_eoo_bits: usize,
}

// Safety: `struct rade` holds no thread-affine state and the library keeps no
// globals (rade_initialize/rade_finalize are no-ops in this version), so the
// handle can move between threads. It is not Sync: every entry point takes
// `&mut self` conceptually, and the C code is not internally synchronised.
unsafe impl Send for Rade {}

/// What one [`Rade::rx`] call produced.
#[derive(Debug, Clone, Copy, Default)]
pub struct RxOut {
    /// Floats written to the features buffer. Zero means "no output yet".
    pub n_features: usize,
    /// An End-of-Over frame was detected: the far end stopped transmitting.
    pub has_eoo: bool,
}

impl Rade {
    /// Open the single RADE V1 context, using the weights compiled into the
    /// library.
    ///
    /// Returns [`RadeError::AlreadyOpen`] if one is already live; drop that one
    /// first.
    pub fn open_v1() -> Result<Self, RadeError> {
        if OPEN.swap(true, Ordering::AcqRel) {
            return Err(RadeError::AlreadyOpen);
        }
        INIT.call_once(|| unsafe { sys::rade_initialize() });

        // Safety: an empty, NUL-terminated model path selects the built-in
        // weights; the C side copies nothing and only reads it for a log line.
        // `c_char` is signed on x86 and unsigned on ARM, so it is spelled out
        // rather than written as `0i8`.
        let mut model = [0 as std::os::raw::c_char; 1];
        // NOTE: RADE_VERBOSE_TERSE was tried as a diagnostic for the
        // "callsign never received" report and confirmed to hang the RX
        // worker thread on this system (its native printf appears to
        // conflict with the app's own stdout redirection, backing up the
        // audio pipeline). Do not re-enable without a different capture
        // strategy (e.g. a separate unredirected process).
        let r = unsafe { sys::rade_open(model.as_mut_ptr(), sys::RADE_VERBOSE_0 as i32) };
        if r.is_null() {
            OPEN.store(false, Ordering::Release);
            return Err(RadeError::OpenFailed);
        }

        // Safety: `r` is non-null and freshly opened.
        let (n_features, n_tx_out, n_tx_eoo_out, nin_max, n_eoo_bits) = unsafe {
            (
                sys::rade_n_features_in_out(r) as usize,
                sys::rade_n_tx_out(r) as usize,
                sys::rade_n_tx_eoo_out(r) as usize,
                sys::rade_nin_max(r) as usize,
                sys::rade_n_eoo_bits(r) as usize,
            )
        };
        Ok(Rade { r, n_features, n_tx_out, n_tx_eoo_out, nin_max, n_eoo_bits })
    }

    /// Floats per feature block -- the unit both [`Rade::tx`] and [`Rade::rx`]
    /// work in. For V1 this is 432: twelve 36-float vocoder frames per 120 ms
    /// modem frame.
    pub fn n_features(&self) -> usize {
        self.n_features
    }

    /// IQ samples one [`Rade::tx`] call produces (960 for V1 = 120 ms).
    pub fn n_tx_out(&self) -> usize {
        self.n_tx_out
    }

    /// IQ samples [`Rade::tx_eoo`] produces.
    pub fn n_tx_eoo_out(&self) -> usize {
        self.n_tx_eoo_out
    }

    /// Upper bound on [`Rade::nin`], for sizing input buffers once.
    pub fn nin_max(&self) -> usize {
        self.nin_max
    }

    /// Soft-decision bits carried by an End-of-Over frame.
    pub fn n_eoo_bits(&self) -> usize {
        self.n_eoo_bits
    }

    /// IQ samples the next [`Rade::rx`] call wants.
    ///
    /// This varies call to call as timing tracking pulls the sample clock, so
    /// it must be re-read before every `rx`.
    pub fn nin(&self) -> usize {
        // Safety: `self.r` is valid for the lifetime of `self`.
        unsafe { sys::rade_nin(self.r) as usize }
    }

    /// Demodulate exactly [`Rade::nin`] IQ samples.
    ///
    /// `features` is cleared and refilled with whatever this call produced;
    /// an empty result is normal and means the modem is still acquiring or
    /// mid-frame.
    ///
    /// `eoo` is likewise cleared, and refilled with [`Rade::n_eoo_bits`]
    /// soft-decision floats (interleaved I/Q) **only** when the returned
    /// [`RxOut::has_eoo`] is set -- `rade_api.h` guarantees nothing about the
    /// buffer's contents otherwise, so it is left empty rather than holding
    /// something that looks like data. Pass it to [`text::decode`] to
    /// recover the far end's callsign.
    pub fn rx(
        &mut self,
        rx_in: &[Complex32],
        features: &mut Vec<f32>,
        eoo: &mut Vec<f32>,
    ) -> Result<RxOut, RadeError> {
        let nin = self.nin();
        if rx_in.len() != nin {
            return Err(RadeError::BadLength { want: nin, got: rx_in.len() });
        }
        features.clear();
        features.resize(self.n_features, 0.0);
        eoo.clear();
        eoo.resize(self.n_eoo_bits, 0.0);
        let mut has_eoo: i32 = 0;

        // Safety: `rx_in` holds exactly `nin` samples, `features` and `eoo` are
        // sized from the library's own getters, and `RADE_COMP` is
        // layout-compatible with `Complex32` (asserted below).
        let n = unsafe {
            sys::rade_rx(
                self.r,
                features.as_mut_ptr(),
                &mut has_eoo,
                eoo.as_mut_ptr(),
                rx_in.as_ptr() as *mut sys::RADE_COMP,
            )
        };
        if n < 0 {
            features.clear();
            eoo.clear();
            return Err(RadeError::Rx(n));
        }
        features.truncate(n as usize);
        if has_eoo == 0 {
            eoo.clear();
        }
        Ok(RxOut { n_features: n as usize, has_eoo: has_eoo != 0 })
    }

    /// Set the soft-decision bits the next [`Rade::tx_eoo`] will carry.
    ///
    /// `bits` must be [`Rade::n_eoo_bits`] long; build it with
    /// [`text::encode`]. The library `memcpy`s the array, so the buffer
    /// need not outlive the call.
    ///
    /// Without this the End-of-Over frame's data symbols are all zero (the
    /// library's own initial value), which carries no callsign *and* denies the
    /// far end the known sequence it estimates its noise variance from. Set it
    /// on every over, even for an empty callsign.
    /// V1 only -- live on/off for the C library's own Tx bandpass filter,
    /// built from the real OFDM carrier geometry (see rade_tx.c's own
    /// comment: bandwidth/centre computed from the actual carrier span,
    /// 1.2x margin). Off by default (rade_open() passes bpf_en=0), same
    /// as freedv-gui itself -- confirmed against its own source, which
    /// never enables this either and relies entirely on the radio's own
    /// external SSB Tx filter. Exposed here so the two can be A/B'd live
    /// against each other on real hardware (a real request) instead of
    /// needing a fresh rade_open() to change it.
    pub fn set_tx_bpf(&mut self, enable: bool) {
        unsafe { sys::rade_set_tx_bpf(self.r, enable as i32) };
    }

    pub fn set_tx_eoo_bits(&mut self, bits: &[f32]) -> Result<(), RadeError> {
        if bits.len() != self.n_eoo_bits {
            return Err(RadeError::BadLength { want: self.n_eoo_bits, got: bits.len() });
        }
        // Safety: the C side copies exactly `n_eoo_bits` floats out of `bits`
        // and retains no pointer to it.
        unsafe { sys::rade_tx_set_eoo_bits(self.r, bits.as_ptr() as *mut f32) };
        Ok(())
    }

    /// Modulate one feature block into [`Rade::n_tx_out`] IQ samples, appended
    /// to `out`.
    pub fn tx(&mut self, features: &[f32], out: &mut Vec<Complex32>) -> Result<(), RadeError> {
        if features.len() != self.n_features {
            return Err(RadeError::BadLength { want: self.n_features, got: features.len() });
        }
        let base = out.len();
        out.resize(base + self.n_tx_out, Complex32::default());

        // Safety: `features` is the exact length the library asks for, and
        // `out` has `n_tx_out` writable slots from `base`. The C side does not
        // retain either pointer.
        let n = unsafe {
            sys::rade_tx(
                self.r,
                out[base..].as_mut_ptr() as *mut sys::RADE_COMP,
                features.as_ptr() as *mut f32,
            )
        };
        out.truncate(base + n.max(0) as usize);
        Ok(())
    }

    /// Append the End-of-Over frame that tells the far end this over has
    /// finished. Send it once, after the last [`Rade::tx`].
    pub fn tx_eoo(&mut self, out: &mut Vec<Complex32>) -> Result<(), RadeError> {
        let base = out.len();
        out.resize(base + self.n_tx_eoo_out, Complex32::default());
        // Safety: `out` has `n_tx_eoo_out` writable slots from `base`.
        let n =
            unsafe { sys::rade_tx_eoo(self.r, out[base..].as_mut_ptr() as *mut sys::RADE_COMP) };
        out.truncate(base + n.max(0) as usize);
        Ok(())
    }

    /// True while the receiver is locked to a signal.
    pub fn sync(&self) -> bool {
        // Safety: `self.r` is valid for the lifetime of `self`.
        unsafe { sys::rade_sync(self.r) != 0 }
    }

    /// Frequency offset of the received signal, valid while [`Rade::sync`].
    pub fn freq_offset_hz(&self) -> f32 {
        // Safety: as above.
        unsafe { sys::rade_freq_offset(self.r) }
    }

    /// SNR estimate in a 3 kHz noise bandwidth, valid while [`Rade::sync`].
    pub fn snr_3k_db(&self) -> f32 {
        // Safety: as above.
        unsafe { sys::rade_snrdB_3k_est(self.r) }
    }
}

impl Drop for Rade {
    fn drop(&mut self) {
        // Safety: `self.r` came from `rade_open` and is closed exactly once.
        unsafe { sys::rade_close(self.r) };
        // `rade_finalize()` is deliberately not called: it is process-global
        // teardown, and a later `open_v1()` would have no way to undo it.
        OPEN.store(false, Ordering::Release);
    }
}

const _: () = {
    assert!(size_of::<sys::RADE_COMP>() == size_of::<Complex32>());
    assert!(align_of::<sys::RADE_COMP>() == align_of::<Complex32>());
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The library allows one context per process, so tests that open one must
    /// not overlap -- cargo runs them on threads within a single process.
    static LOCK: Mutex<()> = Mutex::new(());
    fn exclusive() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn geometry_matches_rade_v1() {
        let _g = exclusive();
        let r = Rade::open_v1().expect("open");
        // Values the library prints at open: V1 n_features_in=432 Nmf=960
        // Neoo=1152 n_eoo_bits=180.
        assert_eq!(r.n_features(), 432);
        assert_eq!(r.n_tx_out(), 960);
        assert_eq!(r.n_tx_eoo_out(), 1152);
        assert_eq!(r.n_eoo_bits(), 180);
        assert!(r.nin() > 0 && r.nin() <= r.nin_max());
    }

    #[test]
    fn second_open_is_refused_then_allowed_after_drop() {
        let _g = exclusive();
        let first = Rade::open_v1().expect("open");
        assert!(matches!(Rade::open_v1(), Err(RadeError::AlreadyOpen)));
        drop(first);
        let _second = Rade::open_v1().expect("reopen after drop");
    }

    #[test]
    fn wrong_input_length_is_an_error_not_a_crash() {
        let _g = exclusive();
        let mut r = Rade::open_v1().expect("open");
        let mut feats = Vec::new();
        let mut eoo = Vec::new();
        let short = vec![Complex32::default(); r.nin() - 1];
        assert!(matches!(r.rx(&short, &mut feats, &mut eoo), Err(RadeError::BadLength { .. })));
        let long = vec![Complex32::default(); r.nin() + 1];
        assert!(matches!(r.rx(&long, &mut feats, &mut eoo), Err(RadeError::BadLength { .. })));
        assert!(matches!(r.tx(&[0.0; 8], &mut Vec::new()), Err(RadeError::BadLength { .. })));
    }

    #[test]
    fn silence_decodes_without_sync() {
        let _g = exclusive();
        let mut r = Rade::open_v1().expect("open");
        let mut feats = Vec::new();
        let mut eoo = Vec::new();
        for _ in 0..50 {
            let block = vec![Complex32::default(); r.nin()];
            let out = r.rx(&block, &mut feats, &mut eoo).expect("rx");
            assert_eq!(out.n_features, 0);
            assert!(eoo.is_empty(), "no end-of-over, so no bits");
        }
        assert!(!r.sync());
    }

    /// Software loopback of the whole transmit chain's End-of-Over frame: our
    /// callsign encoder -> rade_tx_eoo -> (scaling as the app does) -> rade_rx
    /// -> text::decode. If this passes, a callsign that fails to decode over the
    /// air is not a software encoding problem on the transmit side.
    #[test]
    fn eoo_callsign_survives_the_modem_loopback() {
        let _g = exclusive();
        let mut r = Rade::open_v1().expect("open");
        let mut bits = vec![0.0f32; r.n_eoo_bits()];
        crate::rade::text::encode("CU2ED", &mut bits);
        r.set_tx_eoo_bits(&bits).expect("eoo bits");
        let mut sig = Vec::new();
        let feats = vec![0.0f32; r.n_features()];
        for _ in 0..40 {
            r.tx(&feats, &mut sig).expect("tx");
        }
        r.tx_eoo(&mut sig).expect("tx_eoo");
        sig.extend(std::iter::repeat(Complex32::default()).take(8000));
        let mut pos = 0;
        let mut feats_out = Vec::new();
        let mut eoo = Vec::new();
        let mut decoded = None;
        let mut saw_eoo = false;
        while pos + r.nin() <= sig.len() {
            let nin = r.nin();
            // TX_REAL_SCALE (0.5) then RX_REAL_SCALE (4.0): the app's real-valued path.
            let block: Vec<Complex32> = sig[pos..pos + nin]
                .iter()
                .map(|z| Complex32::new(z.re * TX_REAL_SCALE * RX_REAL_SCALE, 0.0))
                .collect();
            pos += nin;
            let out = r.rx(&block, &mut feats_out, &mut eoo).expect("rx");
            if out.has_eoo {
                saw_eoo = true;
                decoded = crate::rade::text::decode(&eoo);
            }
        }
        assert!(saw_eoo, "the receiver never reported an End-of-Over frame");
        assert_eq!(decoded.as_deref(), Some("CU2ED"));
    }
}
