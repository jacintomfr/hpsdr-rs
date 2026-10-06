//! Per-mode equalizer defaults/grouping and mic-profile files (deskHPSDR audio profiles).

use crate::spectrum::{EqualizerParams, Mode, EQ12_DEFAULT_RX_HZ, EQ12_DEFAULT_TX_HZ};

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

/// Prefix of every key of a deskHPSDR audio profile (it stores only the LSB mode settings, mode index 0).
const P: &str = "modeset.0.";

/// One deskHPSDR audio profile (`audio_profile_<n>.prop`). The description is NOT in the file (it is kept in
/// `Config::mic_profile_descs`). hpsdr-rs-only items (leveler decay, TX denoiser, dexp, CTCSS, ...) stay out.
#[derive(Clone, Debug, PartialEq)]
pub struct MicProfile {
    pub tx_eq: EqualizerParams,
    pub rx_eq: EqualizerParams,
    pub leveler_enabled: bool,
    pub leveler_gain_db: f32,
    pub compressor_enabled: bool,
    pub compressor_gain_db: f32,
    /// "Use Pre-CFC" (`cfc` key).
    pub cfc_enabled: bool,
    /// Phase rotator, CFC arrays/curves/weights, Post-CFC, EQ ctfmode, TX filter, add. mic gain.
    pub extra: crate::tx::TxExtra,
}

impl Default for MicProfile {
    fn default() -> Self {
        Self {
            tx_eq: EqualizerParams::default_tx(),
            rx_eq: EqualizerParams::default(),
            leveler_enabled: false,
            leveler_gain_db: 0.0,
            compressor_enabled: false,
            compressor_gain_db: 0.0,
            cfc_enabled: false,
            extra: crate::tx::TxExtra::default(),
        }
    }
}

type Props = std::collections::HashMap<String, String>;

fn put_i(out: &mut String, key: &str, v: i64) {
    out.push_str(&format!("{key}={v}\n"));
}
fn put_f(out: &mut String, key: &str, v: f64) {
    out.push_str(&format!("{key}={v:.6}\n"));
}
fn get_f(p: &Props, key: &str) -> Option<f64> {
    p.get(key).and_then(|v| v.trim().parse::<f64>().ok())
}
fn get_i(p: &Props, key: &str) -> Option<i64> {
    let v = p.get(key)?.trim();
    v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().map(|f| f.round() as i64))
}
fn get_b(p: &Props, key: &str, dst: &mut bool) {
    if let Some(v) = get_i(p, key) {
        *dst = v != 0;
    }
}
fn weight_x10(w: f64) -> u16 {
    ((w * 10.0).round() as i64).clamp(1, 999) as u16
}

fn write_eq(out: &mut String, tag: &str, eq: &EqualizerParams) {
    put_i(out, &format!("{P}{tag}eq_curve_degree"), eq.curve_deg as i64);
    put_i(out, &format!("{P}{tag}eq_curve_r"), eq.nurbs_r as i64);
    put_i(out, &format!("{P}{tag}eq_curve_umethod"), 0);
    // deskHPSDR arrays have 13 entries: index 0 = frequency-independent gain (freq 0), 1..=12 = the 12 bands.
    for j in 0..13 {
        let (g, f) = if j == 0 { (eq.preamp12_db, 0) } else { (eq.bands_12_db[j - 1], eq.freqs_12_hz[j - 1]) };
        put_f(out, &format!("{P}{tag}eq.{j}"), g as f64);
        put_f(out, &format!("{P}{tag}eqfrq.{j}"), f as f64);
        if j < 12 {
            put_f(out, &format!("{P}{tag}eq_weight.{j}"), eq.weights_x10[j] as f64 / 10.0);
        }
    }
}

fn read_eq(p: &Props, tag: &str, enable_key: &str, eq: &mut EqualizerParams) {
    // Backward-compat defaults (deskHPSDR audioLoadProfile): legacy curve, weights 1.0.
    eq.curve_deg = 0;
    eq.nurbs_r = false;
    eq.weights_x10 = [10; 12];
    get_b(p, &format!("{P}{enable_key}"), &mut eq.enabled);
    if let Some(v) = get_i(p, &format!("{P}{tag}eq_curve_degree")) {
        eq.curve_deg = v.clamp(0, 255) as u8;
    }
    get_b(p, &format!("{P}{tag}eq_curve_r"), &mut eq.nurbs_r);
    for j in 0..13 {
        if let Some(g) = get_f(p, &format!("{P}{tag}eq.{j}")) {
            let g = g.round() as i32;
            if j == 0 {
                eq.preamp12_db = g;
            } else {
                eq.bands_12_db[j - 1] = g;
            }
        }
        if j > 0 {
            if let Some(f) = get_f(p, &format!("{P}{tag}eqfrq.{j}")) {
                eq.freqs_12_hz[j - 1] = f.round() as i32;
            }
        }
        if j < 12 {
            if let Some(w) = get_f(p, &format!("{P}{tag}eq_weight.{j}")) {
                eq.weights_x10[j] = weight_x10(w);
            }
        }
    }
}

/// Like toolset.c `sort_cfc_profile`: sorts CFC points 1..=12 by frequency, carrying level, post gain and both
/// weights (weight j belongs to point j+1); index 0 (frequency-independent values) is untouched.
fn sort_cfc(x: &mut crate::tx::TxExtra) {
    let mut idx: [usize; 12] = std::array::from_fn(|i| i);
    idx.sort_by_key(|&i| x.cfc_freq_hz[i + 1]); // stable
    let (f, l, po, cw, pw) = (x.cfc_freq_hz, x.cfc_lvl_db, x.cfc_post_db, x.cfc_comp_w_x10, x.cfc_post_w_x10);
    for (k, &i) in idx.iter().enumerate() {
        x.cfc_freq_hz[k + 1] = f[i + 1];
        x.cfc_lvl_db[k + 1] = l[i + 1];
        x.cfc_post_db[k + 1] = po[i + 1];
        x.cfc_comp_w_x10[k] = cw[i];
        x.cfc_post_w_x10[k] = pw[i];
    }
    for k in 0..13 {
        x.cfc_lvl_db[k] = x.cfc_lvl_db[k].clamp(0, 20);
        x.cfc_post_db[k] = x.cfc_post_db[k].clamp(-20, 20);
    }
    for k in 1..13 {
        x.cfc_freq_hz[k] = x.cfc_freq_hz[k].clamp(10, 16000);
    }
    for k in 0..12 {
        x.cfc_comp_w_x10[k] = x.cfc_comp_w_x10[k].clamp(1, 999);
        x.cfc_post_w_x10[k] = x.cfc_post_w_x10[k].clamp(1, 999);
    }
}

fn valid_deg(d: i64) -> u8 {
    if matches!(d, 0 | 1 | 3 | 5 | 7) { d as u8 } else { 0 }
}

impl MicProfile {
    /// Snapshot of the current TX chain plus the caller's RX EQ.
    pub fn capture(tx: &crate::tx::TxHandle, rx_eq: EqualizerParams) -> MicProfile {
        MicProfile {
            tx_eq: tx.eq(),
            rx_eq,
            leveler_enabled: tx.leveler_enabled(),
            leveler_gain_db: tx.leveler_gain_db(),
            compressor_enabled: tx.compressor_enabled(),
            compressor_gain_db: tx.compressor_gain_db(),
            cfc_enabled: tx.cfc_enabled(),
            extra: tx.tx_extra(),
        }
    }

    /// Applies the TX side to `tx` and returns the (sorted/clamped) RX EQ for the caller to apply. Of the
    /// `TxExtra` only the fields a deskHPSDR profile carries are taken; the other live options (dexp, CTCSS,
    /// CESSB, FM/AM, phase-rotator stage/freq) are kept. A profile defines the TX filter, so `use_rx_filter`
    /// is switched off.
    pub fn apply(&self, tx: &crate::tx::TxHandle) -> EqualizerParams {
        tx.set_eq(sorted_eq(self.tx_eq));
        tx.set_leveler_enabled(self.leveler_enabled);
        tx.set_leveler_gain_db(self.leveler_gain_db);
        tx.set_compressor_enabled(self.compressor_enabled);
        tx.set_compressor_gain_db(self.compressor_gain_db);
        tx.set_cfc_enabled(self.cfc_enabled);
        let mut x = tx.tx_extra();
        let s = &self.extra;
        x.use_rx_filter = false;
        x.tx_filter_low_hz = s.tx_filter_low_hz;
        x.tx_filter_high_hz = s.tx_filter_high_hz;
        x.phrot_enable = s.phrot_enable;
        x.eq_ctfmode = s.eq_ctfmode;
        x.addgain_enable = s.addgain_enable;
        x.addgain_gain_db = s.addgain_gain_db;
        x.cfc_post_enabled = s.cfc_post_enabled;
        x.cfc_freq_hz = s.cfc_freq_hz;
        x.cfc_lvl_db = s.cfc_lvl_db;
        x.cfc_post_db = s.cfc_post_db;
        x.cfc_comp_deg = s.cfc_comp_deg;
        x.cfc_comp_r = s.cfc_comp_r;
        x.cfc_comp_w_x10 = s.cfc_comp_w_x10;
        x.cfc_post_deg = s.cfc_post_deg;
        x.cfc_post_r = s.cfc_post_r;
        x.cfc_post_w_x10 = s.cfc_post_w_x10;
        tx.set_tx_extra(x);
        sorted_eq(self.rx_eq)
    }

    /// deskHPSDR property-file text (`key=value` lines, first line `PGNAME=deskHPSDR`).
    pub fn to_prop_string(&self) -> String {
        let mut o = String::new();
        o.push_str("PGNAME=deskHPSDR\n");
        let x = &self.extra;
        put_i(&mut o, &format!("{P}en_txeq"), self.tx_eq.enabled as i64);
        put_i(&mut o, &format!("{P}en_rxeq"), self.rx_eq.enabled as i64);
        put_i(&mut o, &format!("{P}compressor"), self.compressor_enabled as i64);
        put_f(&mut o, &format!("{P}compressor_level"), self.compressor_gain_db as f64);
        put_i(&mut o, &format!("{P}lev_enable"), self.leveler_enabled as i64);
        put_f(&mut o, &format!("{P}lev_gain"), self.leveler_gain_db as f64);
        put_i(&mut o, &format!("{P}phrot_enable"), x.phrot_enable as i64);
        put_i(&mut o, &format!("{P}cfc"), self.cfc_enabled as i64);
        put_i(&mut o, &format!("{P}cfc_eq"), x.cfc_post_enabled as i64);
        put_i(&mut o, &format!("{P}cfc_comp_curve_degree"), x.cfc_comp_deg as i64);
        put_i(&mut o, &format!("{P}cfc_comp_curve_r"), x.cfc_comp_r as i64);
        put_i(&mut o, &format!("{P}cfc_comp_curve_umethod"), 0);
        put_i(&mut o, &format!("{P}cfc_post_curve_degree"), x.cfc_post_deg as i64);
        put_i(&mut o, &format!("{P}cfc_post_curve_r"), x.cfc_post_r as i64);
        put_i(&mut o, &format!("{P}cfc_post_curve_umethod"), 0);
        write_eq(&mut o, "tx", &self.tx_eq);
        write_eq(&mut o, "rx", &self.rx_eq);
        for j in 0..13 {
            put_f(&mut o, &format!("{P}cfc_frq.{j}"), x.cfc_freq_hz[j] as f64);
            put_f(&mut o, &format!("{P}cfc_lvl.{j}"), x.cfc_lvl_db[j] as f64);
            put_f(&mut o, &format!("{P}cfc_post.{j}"), x.cfc_post_db[j] as f64);
            if j < 12 {
                put_f(&mut o, &format!("{P}cfc_comp_weight.{j}"), x.cfc_comp_w_x10[j] as f64 / 10.0);
                put_f(&mut o, &format!("{P}cfc_post_weight.{j}"), x.cfc_post_w_x10[j] as f64 / 10.0);
            }
        }
        put_i(&mut o, "transmitter.addgain_enable", x.addgain_enable as i64);
        put_f(&mut o, "transmitter.addgain_gain", x.addgain_gain_db);
        put_i(&mut o, "transmitter.tx_filter_high", x.tx_filter_high_hz as i64);
        put_i(&mut o, "transmitter.tx_filter_low", x.tx_filter_low_hz as i64);
        put_i(&mut o, "transmitter.eq_ctfmode", x.eq_ctfmode as i64);
        o
    }

    /// Parses a deskHPSDR audio profile. Errors if `PGNAME` is not `deskHPSDR`. Missing keys keep the defaults
    /// (curve legacy, weights 1.0, ...); EQ and CFC points are sorted/clamped like deskHPSDR does on load.
    pub fn from_prop_str(text: &str) -> Result<MicProfile, String> {
        let mut p = Props::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                p.insert(k.to_string(), v.to_string());
            }
        }
        if p.get("PGNAME").map(|s| s.trim()) != Some("deskHPSDR") {
            return Err("Not a deskHPSDR audio profile".to_string());
        }
        let mut m = MicProfile::default();
        read_eq(&p, "tx", "en_txeq", &mut m.tx_eq);
        read_eq(&p, "rx", "en_rxeq", &mut m.rx_eq);
        m.tx_eq = sorted_eq(m.tx_eq);
        m.rx_eq = sorted_eq(m.rx_eq);
        get_b(&p, &format!("{P}compressor"), &mut m.compressor_enabled);
        if let Some(v) = get_f(&p, &format!("{P}compressor_level")) {
            m.compressor_gain_db = v as f32;
        }
        get_b(&p, &format!("{P}lev_enable"), &mut m.leveler_enabled);
        if let Some(v) = get_f(&p, &format!("{P}lev_gain")) {
            m.leveler_gain_db = v as f32;
        }
        get_b(&p, &format!("{P}cfc"), &mut m.cfc_enabled);
        let x = &mut m.extra;
        get_b(&p, &format!("{P}phrot_enable"), &mut x.phrot_enable);
        get_b(&p, &format!("{P}cfc_eq"), &mut x.cfc_post_enabled);
        x.cfc_comp_deg = 0;
        x.cfc_comp_r = false;
        x.cfc_post_deg = 0;
        x.cfc_post_r = false;
        x.cfc_comp_w_x10 = [10; 12];
        x.cfc_post_w_x10 = [10; 12];
        if let Some(v) = get_i(&p, &format!("{P}cfc_comp_curve_degree")) {
            x.cfc_comp_deg = valid_deg(v);
        }
        get_b(&p, &format!("{P}cfc_comp_curve_r"), &mut x.cfc_comp_r);
        if let Some(v) = get_i(&p, &format!("{P}cfc_post_curve_degree")) {
            x.cfc_post_deg = valid_deg(v);
        }
        get_b(&p, &format!("{P}cfc_post_curve_r"), &mut x.cfc_post_r);
        for j in 0..13 {
            if let Some(v) = get_f(&p, &format!("{P}cfc_frq.{j}")) {
                x.cfc_freq_hz[j] = v.round() as i32;
            }
            if let Some(v) = get_f(&p, &format!("{P}cfc_lvl.{j}")) {
                x.cfc_lvl_db[j] = v.round() as i32;
            }
            if let Some(v) = get_f(&p, &format!("{P}cfc_post.{j}")) {
                x.cfc_post_db[j] = v.round() as i32;
            }
            if j < 12 {
                if let Some(v) = get_f(&p, &format!("{P}cfc_comp_weight.{j}")) {
                    x.cfc_comp_w_x10[j] = weight_x10(v);
                }
                if let Some(v) = get_f(&p, &format!("{P}cfc_post_weight.{j}")) {
                    x.cfc_post_w_x10[j] = weight_x10(v);
                }
            }
        }
        sort_cfc(x);
        get_b(&p, "transmitter.addgain_enable", &mut x.addgain_enable);
        if let Some(v) = get_f(&p, "transmitter.addgain_gain") {
            x.addgain_gain_db = v;
        }
        if let Some(v) = get_i(&p, "transmitter.tx_filter_high") {
            x.tx_filter_high_hz = v as i32;
        }
        if let Some(v) = get_i(&p, "transmitter.tx_filter_low") {
            x.tx_filter_low_hz = v as i32;
        }
        get_b(&p, "transmitter.eq_ctfmode", &mut x.eq_ctfmode);
        Ok(m)
    }

    /// Writes the profile to `path` (a `.prop` extension is appended if missing), atomically.
    pub fn export_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let path = with_prop_ext(path);
        let tmp = path.with_extension("prop.tmp");
        std::fs::write(&tmp, self.to_prop_string()).and_then(|_| std::fs::rename(&tmp, &path))
    }

    /// Reads a deskHPSDR audio profile file.
    pub fn import_from(path: &std::path::Path) -> Result<MicProfile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        MicProfile::from_prop_str(&text)
    }
}

fn with_prop_ext(path: &std::path::Path) -> std::path::PathBuf {
    if path.extension().map(|e| e == "prop").unwrap_or(false) {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".prop");
        std::path::PathBuf::from(s)
    }
}

/// Slots are 0..=2 (deskHPSDR numbering).
fn slot_path(n: usize) -> Option<std::path::PathBuf> {
    if n >= crate::config::MIC_PROFILE_SLOTS {
        return None;
    }
    let mut p = crate::config::settings_dir()?;
    p.push(format!("audio_profile_{n}.prop"));
    Some(p)
}

/// Old hpsdr-rs JSON slot (1-based file number).
fn legacy_slot_path(n: usize) -> Option<std::path::PathBuf> {
    let mut p = crate::config::settings_dir()?;
    p.push(format!("mic-profile-{}.json", n + 1));
    Some(p)
}

/// Old JSON format, only used for the one-time migration.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct LegacyMicProfile {
    tx_eq: Option<EqualizerParams>,
    rx_eq: Option<EqualizerParams>,
    leveler_enabled: bool,
    leveler_gain_db: f32,
    compressor_enabled: bool,
    compressor_gain_db: f32,
    cfc_enabled: bool,
}

fn load_legacy(n: usize) -> Option<MicProfile> {
    let text = std::fs::read_to_string(legacy_slot_path(n)?).ok()?;
    let l: LegacyMicProfile = serde_json::from_str(&text).ok()?;
    let d = MicProfile::default();
    Some(MicProfile {
        tx_eq: sorted_eq(l.tx_eq.unwrap_or(d.tx_eq)),
        rx_eq: sorted_eq(l.rx_eq.unwrap_or(d.rx_eq)),
        leveler_enabled: l.leveler_enabled,
        leveler_gain_db: l.leveler_gain_db,
        compressor_enabled: l.compressor_enabled,
        compressor_gain_db: l.compressor_gain_db,
        cfc_enabled: l.cfc_enabled,
        extra: d.extra,
    })
}

/// True if slot `n` (0..=2) has an `audio_profile_<n>.prop` (or a not yet migrated old JSON file).
pub fn slot_exists(n: usize) -> bool {
    slot_path(n).map(|p| p.is_file()).unwrap_or(false)
        || (n < crate::config::MIC_PROFILE_SLOTS && legacy_slot_path(n).map(|p| p.is_file()).unwrap_or(false))
}

/// Loads slot `n` (0..=2). If only the old `mic-profile-<n+1>.json` exists it is converted once into the new
/// `.prop` file (the old file is kept).
pub fn load_slot(n: usize) -> Option<MicProfile> {
    let path = slot_path(n)?;
    if path.is_file() {
        return MicProfile::import_from(&path).ok();
    }
    let m = load_legacy(n)?;
    let _ = m.export_to(&path);
    Some(m)
}

pub fn save_slot(n: usize, p: &MicProfile) -> std::io::Result<()> {
    let path = slot_path(n)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad mic profile slot or no settings dir"))?;
    p.export_to(&path)
}

/// The `*.prop` files of the settings dir, sorted by name (file list for the touch UI).
pub fn list_prop_files() -> Vec<std::path::PathBuf> {
    let Some(dir) = crate::config::settings_dir() else { return Vec::new() };
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map(|e| e == "prop").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
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

    #[test]
    fn prop_round_trip() {
        let mut m = MicProfile::default();
        m.tx_eq.enabled = true;
        m.tx_eq.curve_deg = 3;
        m.tx_eq.nurbs_r = true;
        m.tx_eq.preamp12_db = 4;
        m.tx_eq.bands_12_db = [-9, -6, -6, -9, 0, 0, 3, 3, 3, 3, 1, 2];
        m.tx_eq.weights_x10 = [10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65];
        m.rx_eq.enabled = true;
        m.rx_eq.bands_12_db[2] = 5;
        m.compressor_enabled = true;
        m.compressor_gain_db = 6.5;
        m.leveler_enabled = true;
        m.leveler_gain_db = 3.0;
        m.cfc_enabled = true;
        m.extra.phrot_enable = true;
        m.extra.cfc_post_enabled = true;
        m.extra.cfc_comp_deg = 5;
        m.extra.cfc_post_r = true;
        m.extra.eq_ctfmode = true;
        m.extra.addgain_enable = true;
        m.extra.addgain_gain_db = 12.5;
        m.extra.tx_filter_low_hz = 150;
        m.extra.tx_filter_high_hz = 3000;
        m.extra.cfc_comp_w_x10[3] = 77;
        let text = m.to_prop_string();
        assert!(text.starts_with("PGNAME=deskHPSDR\n"));
        assert!(text.contains("modeset.0.txeq.0=4.000000"));
        assert!(text.contains("modeset.0.txeq_weight.1=1.500000"));
        let r = MicProfile::from_prop_str(&text).unwrap();
        assert_eq!(r, m);
    }

    #[test]
    fn prop_refuses_foreign_and_defaults() {
        assert!(MicProfile::from_prop_str("PGNAME=piHPSDR\nmodeset.0.en_txeq=1\n").is_err());
        assert!(MicProfile::from_prop_str("modeset.0.en_txeq=1\n").is_err());
        let r = MicProfile::from_prop_str("PGNAME=deskHPSDR\nmodeset.0.en_txeq=1\n").unwrap();
        assert!(r.tx_eq.enabled);
        assert_eq!(r.tx_eq.weights_x10, [10; 12]);
        assert_eq!(r.tx_eq.curve_deg, 0);
    }

    #[test]
    fn prop_sorts_points() {
        let text = "PGNAME=deskHPSDR\n\
            modeset.0.txeqfrq.1=7777.000000\nmodeset.0.txeq.1=7.000000\n\
            modeset.0.txeqfrq.2=77.000000\nmodeset.0.txeq.2=-3.000000\n\
            modeset.0.cfc_frq.1=2222.000000\nmodeset.0.cfc_lvl.1=9.000000\nmodeset.0.cfc_comp_weight.0=3.000000\n\
            modeset.0.cfc_frq.2=555.000000\nmodeset.0.cfc_lvl.2=4.000000\n";
        let r = MicProfile::from_prop_str(text).unwrap();
        assert!(r.tx_eq.freqs_12_hz.windows(2).all(|w| w[1] > w[0]));
        let i = r.tx_eq.freqs_12_hz.iter().position(|&f| f == 77).unwrap();
        assert_eq!(r.tx_eq.bands_12_db[i], -3);
        let k = r.tx_eq.freqs_12_hz.iter().position(|&f| f == 7777).unwrap();
        assert_eq!(r.tx_eq.bands_12_db[k], 7);
        let x = &r.extra;
        assert!(x.cfc_freq_hz[1..].windows(2).all(|w| w[0] <= w[1]));
        let a = x.cfc_freq_hz.iter().position(|&f| f == 555).unwrap();
        assert_eq!(x.cfc_lvl_db[a], 4);
        let b = x.cfc_freq_hz.iter().position(|&f| f == 2222).unwrap();
        assert_eq!(x.cfc_lvl_db[b], 9);
        assert_eq!(x.cfc_comp_w_x10[b - 1], 30);
    }
}
