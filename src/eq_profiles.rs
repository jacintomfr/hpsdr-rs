//! Per-mode equalizer defaults/grouping and mic-profile files (deskHPSDR audio profiles).

use crate::spectrum::{EqualizerParams, Mode, EQ12_DEFAULT_RX_HZ, EQ12_DEFAULT_TX_HZ};
use serde::{Deserialize, Serialize};

/// Key of the per-mode EQ store, mirroring deskHPSDR's `copy_mode_settings` (vfo.c): LSB/USB/DSB share one
/// entry, CWL/CWU one, DIGL/DIGU one, every other mode has its own.
pub fn eq_mode_group(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb | Mode::Usb | Mode::Dsb => "SSB",
        Mode::Cwl | Mode::Cwu => "CW",
        Mode::Digl | Mode::Digu => "DIG",
        Mode::Fmn => "FM",
        Mode::Am => "AM",
        Mode::Sam => "SAM",
        Mode::Spec => "SPEC",
        Mode::Drm => "DRM",
    }
}

/// deskHPSDR's per-mode EQ defaults (vfo.c). RX: all gains 0 on the RX frequencies. TX: SSB-type modes
/// (LSB/USB/DSB) get the voice-shaping gains -9,-6,-6,-9,0,0,3,3,3,3,0,0 on the TX frequencies; every other mode
/// gains 0. vfo.c:552-553 sets the last two SSB TX gains to +3 whereas transmitter.c (and `default_tx()`) says 0;
/// we follow transmitter.c (0, 0). Curve = legacy, weights 1.0, EQ disabled, as in vfo.c.
pub fn default_eq_for_mode(mode: Mode, tx: bool) -> EqualizerParams {
    let mut eq = EqualizerParams::default();
    if tx {
        eq.freqs_12_hz = EQ12_DEFAULT_TX_HZ;
        if matches!(mode, Mode::Lsb | Mode::Usb | Mode::Dsb) {
            eq.bands_12_db = [-9, -6, -6, -9, 0, 0, 3, 3, 3, 3, 0, 0];
        }
    } else {
        eq.freqs_12_hz = EQ12_DEFAULT_RX_HZ;
    }
    eq
}

/// Like toolset.c `sort_eq_profile` when loading: sorts the 12 points by frequency (gains and weights follow),
/// clamps frequency 10..16000 Hz, gains -20..20 dB (also the preamp), weights 1..=999 tenths, curve_deg to
/// one of 0,1,3,5,7, and keeps neighbouring points at least 10 Hz apart.
pub fn sorted_eq(mut eq: EqualizerParams) -> EqualizerParams {
    let mut idx: [usize; 12] = std::array::from_fn(|i| i);
    idx.sort_by_key(|&i| eq.freqs_12_hz[i]); // stable
    let f = eq.freqs_12_hz;
    let g = eq.bands_12_db;
    let w = eq.weights_x10;
    for (k, &i) in idx.iter().enumerate() {
        eq.freqs_12_hz[k] = f[i];
        eq.bands_12_db[k] = g[i];
        eq.weights_x10[k] = w[i];
    }
    for k in 0..12 {
        eq.freqs_12_hz[k] = eq.freqs_12_hz[k].clamp(10, 16000);
        eq.bands_12_db[k] = eq.bands_12_db[k].clamp(-20, 20);
        eq.weights_x10[k] = eq.weights_x10[k].clamp(1, 999);
    }
    for k in 1..12 {
        let min_allowed = eq.freqs_12_hz[k - 1] + 10;
        if eq.freqs_12_hz[k] < min_allowed {
            eq.freqs_12_hz[k] = min_allowed;
        }
    }
    if eq.freqs_12_hz[11] > 16000 {
        eq.freqs_12_hz[11] = 16000;
        for k in (0..11).rev() {
            let max_allowed = eq.freqs_12_hz[k + 1] - 10;
            if eq.freqs_12_hz[k] > max_allowed {
                eq.freqs_12_hz[k] = max_allowed;
            }
        }
    }
    eq.preamp12_db = eq.preamp12_db.clamp(-20, 20);
    if !matches!(eq.curve_deg, 0 | 1 | 3 | 5 | 7) {
        eq.curve_deg = 0;
    }
    eq
}

/// One deskHPSDR-style audio ("mic") profile: both EQs plus the TX audio-chain settings hpsdr-rs has.
/// Not carried (no getter/field in TxHandle): mic gain, CFC curve, dexp, phase rotator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MicProfile {
    pub desc: String,
    pub tx_eq: EqualizerParams,
    pub rx_eq: EqualizerParams,
    pub leveler_enabled: bool,
    pub leveler_gain_db: f32,
    pub leveler_decay_ms: i32,
    pub compressor_enabled: bool,
    pub compressor_gain_db: f32,
    pub cfc_enabled: bool,
    pub tx_denoiser_enabled: bool,
}

impl Default for MicProfile {
    fn default() -> Self {
        Self {
            desc: String::new(),
            tx_eq: EqualizerParams::default_tx(),
            rx_eq: EqualizerParams::default(),
            leveler_enabled: false,
            leveler_gain_db: 0.0,
            leveler_decay_ms: 100,
            compressor_enabled: false,
            compressor_gain_db: 0.0,
            cfc_enabled: false,
            tx_denoiser_enabled: false,
        }
    }
}

impl MicProfile {
    /// Snapshot of the current TX chain plus the caller's RX EQ. `desc` is left empty for the caller to fill in.
    pub fn capture(tx: &crate::tx::TxHandle, rx_eq: EqualizerParams) -> MicProfile {
        MicProfile {
            desc: String::new(),
            tx_eq: tx.eq(),
            rx_eq,
            leveler_enabled: tx.leveler_enabled(),
            leveler_gain_db: tx.leveler_gain_db(),
            leveler_decay_ms: tx.leveler_decay_ms(),
            compressor_enabled: tx.compressor_enabled(),
            compressor_gain_db: tx.compressor_gain_db(),
            cfc_enabled: tx.cfc_enabled(),
            tx_denoiser_enabled: tx.tx_denoiser_enabled(),
        }
    }

    /// Applies the TX side to `tx` and returns the (sorted/clamped) RX EQ for the caller to apply.
    pub fn apply(&self, tx: &crate::tx::TxHandle) -> EqualizerParams {
        tx.set_eq(sorted_eq(self.tx_eq));
        tx.set_leveler_enabled(self.leveler_enabled);
        tx.set_leveler_gain_db(self.leveler_gain_db);
        tx.set_leveler_decay_ms(self.leveler_decay_ms);
        tx.set_compressor_enabled(self.compressor_enabled);
        tx.set_compressor_gain_db(self.compressor_gain_db);
        tx.set_cfc_enabled(self.cfc_enabled);
        tx.set_tx_denoiser_enabled(self.tx_denoiser_enabled);
        sorted_eq(self.rx_eq)
    }
}

/// Slots are 1..=3 (as shown to the user).
fn slot_path(n: usize) -> Option<std::path::PathBuf> {
    if !(1..=3).contains(&n) {
        return None;
    }
    let mut p = crate::config::settings_dir()?;
    p.push(format!("mic-profile-{n}.json"));
    Some(p)
}

pub fn load_slot(n: usize) -> Option<MicProfile> {
    let text = std::fs::read_to_string(slot_path(n)?).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_slot(n: usize, p: &MicProfile) -> std::io::Result<()> {
    let path = slot_path(n)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad mic profile slot or no settings dir"))?;
    let json = serde_json::to_string_pretty(p).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path))
}

/// Descriptions of slots 1..=3; an empty/missing slot shows "(empty)".
pub fn slot_descriptions() -> [String; 3] {
    std::array::from_fn(|i| match load_slot(i + 1) {
        Some(p) if !p.desc.trim().is_empty() => p.desc,
        Some(_) => "(no description)".to_string(),
        None => "(empty)".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        assert_eq!(eq_mode_group(Mode::Lsb), eq_mode_group(Mode::Dsb));
        assert_eq!(eq_mode_group(Mode::Usb), eq_mode_group(Mode::Lsb));
        assert_eq!(eq_mode_group(Mode::Cwl), eq_mode_group(Mode::Cwu));
        assert_eq!(eq_mode_group(Mode::Digl), eq_mode_group(Mode::Digu));
        assert_ne!(eq_mode_group(Mode::Am), eq_mode_group(Mode::Sam));
        assert_ne!(eq_mode_group(Mode::Cwl), eq_mode_group(Mode::Lsb));
    }

    #[test]
    fn defaults() {
        assert_eq!(default_eq_for_mode(Mode::Usb, true).bands_12_db, [-9, -6, -6, -9, 0, 0, 3, 3, 3, 3, 0, 0]);
        assert_eq!(default_eq_for_mode(Mode::Usb, true), EqualizerParams::default_tx());
        assert_eq!(default_eq_for_mode(Mode::Am, true).bands_12_db, [0; 12]);
        assert_eq!(default_eq_for_mode(Mode::Am, true).freqs_12_hz, EQ12_DEFAULT_TX_HZ);
        assert_eq!(default_eq_for_mode(Mode::Usb, false), EqualizerParams::default());
    }

    #[test]
    fn sorting() {
        let mut e = EqualizerParams::default();
        e.freqs_12_hz = [3000, 50, 100, 200, 500, 1000, 1500, 2000, 2500, 5000, 6000, 99999];
        e.bands_12_db = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 99];
        e.weights_x10 = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 0];
        e.curve_deg = 4;
        let s = sorted_eq(e);
        assert_eq!(s.freqs_12_hz[0], 50);
        assert_eq!(s.bands_12_db[0], 2);
        assert_eq!(s.weights_x10[0], 20);
        assert_eq!(s.freqs_12_hz[11], 16000);
        assert_eq!(s.bands_12_db[11], 20);
        assert_eq!(s.weights_x10[11], 1);
        assert_eq!(s.curve_deg, 0);
        assert!(s.freqs_12_hz.windows(2).all(|w| w[1] - w[0] >= 10));
    }
}
