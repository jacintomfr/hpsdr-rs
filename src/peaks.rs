//! Panadapter peak labels and Peaks & Hold, ported from deskHPSDR (rx_panadapter.c / tx_panadapter.c / meter.c).
//! Pure functions and small state structs, std only. See docs/display-menu.md.

use std::time::{Duration, Instant};

/// The analyzer produces this many spectrum rows per second (spectrum.rs SPECTRUM_FPS): deskHPSDR's `rx->fps` for
/// the hold/decay arithmetic (the UI redraw rate is not the data rate here).
pub const ANALYZER_FPS: f32 = 10.0;

#[derive(Clone, Debug)]
pub struct PeakParams {
    pub on: bool,
    pub in_passband: bool,
    pub hide_noise: bool,
    pub num_peaks: i32,
    pub ignore_range_divider: i32,
    pub noise_percentile: i32,
}

impl Default for PeakParams {
    fn default() -> Self {
        Self {
            on: false,
            in_passband: false,
            hide_noise: true,
            num_peaks: 4,
            ignore_range_divider: 24,
            noise_percentile: 80,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeakLabel {
    /// Horizontal centre of the label.
    pub x_px: f32,
    /// Text baseline.
    pub y_px: f32,
    #[allow(dead_code)]
    pub value_db: f32,
    pub text: String,
}

/// deskHPSDR's peak-display noise threshold: the `percentile` of the row plus 3 dB.
pub fn noise_level_of(samples: &[f32], percentile: i32) -> f64 {
    let w = samples.len();
    if w == 0 {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples.iter().map(|&v| v as f64).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((percentile as f64 / 100.0) * w as f64) as usize;
    sorted[idx.min(w - 1)] + 3.0
}

/// Returns (index, dB) of up to `num_peaks` peaks, strongest first.
/// Returns an empty vector when `p.on` is false. The noise level is computed here (use `find_peaks_nl` with a cached
/// level in the drawing code).
#[allow(dead_code)] // tests and callers that do not cache the noise level
pub fn find_peaks(samples_px: &[f32], width_px: usize, passband_px: Option<(usize, usize)>, p: &PeakParams) -> Vec<(usize, f32)> {
    let width = width_px.min(samples_px.len());
    let nl = if p.hide_noise && width > 0 { Some(noise_level_of(&samples_px[..width], p.noise_percentile)) } else { None };
    find_peaks_nl(samples_px, width_px, passband_px, p, nl)
}

/// As `find_peaks`, with the noise level given (None = do not hide noise), like deskHPSDR's cached per-second level.
pub fn find_peaks_nl(samples_px: &[f32], width_px: usize, passband_px: Option<(usize, usize)>, p: &PeakParams, noise_level: Option<f64>) -> Vec<(usize, f32)> {
    let width = width_px.min(samples_px.len());
    let num_peaks = p.num_peaks.max(0) as usize;
    if !p.on || num_peaks == 0 || width == 0 {
        return Vec::new();
    }
    let divider = p.ignore_range_divider.max(1) as i64;
    let w = width as i64;
    let ignore_range = (w + divider - 1) / divider;

    let mut peaks = vec![-200.0f64; num_peaks];
    let mut pos = vec![0i64; num_peaks];

    let hide_noise = noise_level.is_some();
    let noise_level = noise_level.unwrap_or(0.0);

    let (left, right) = match (p.in_passband, passband_px) {
        (true, Some((l, r))) => (l as i64, r as i64),
        _ => (0, w),
    };
    // deskHPSDR forces the first and the last sample to -200 before it looks for peaks.
    let at = |i: i64| -> f64 {
        if i == 0 || i == w - 1 {
            -200.0
        } else {
            samples_px[i as usize] as f64
        }
    };

    for i in 1..(w - 1).max(1) {
        if i < left || i > right {
            continue;
        }
        let s = at(i);
        if (!hide_noise || s >= noise_level) && s > at(i - 1) && s > at(i + 1) {
            let mut replace_index: i32 = -1;
            let start = i - ignore_range;
            let end = i + ignore_range;
            for j in 0..num_peaks {
                if pos[j] >= start && pos[j] <= end {
                    replace_index = if s > peaks[j] { j as i32 } else { -2 };
                    break;
                }
            }
            if replace_index >= 0 {
                let r = replace_index as usize;
                peaks[r] = s;
                pos[r] = i;
            } else if replace_index == -1 {
                let mut lowest = 0;
                for j in 1..num_peaks {
                    if peaks[j] < peaks[lowest] {
                        lowest = j;
                    }
                }
                if s > peaks[lowest] {
                    peaks[lowest] = s;
                    pos[lowest] = i;
                }
            }
        }
    }

    // Same exchange sort as the C code.
    for i in 0..num_peaks.saturating_sub(1) {
        for j in (i + 1)..num_peaks {
            if peaks[i] < peaks[j] {
                peaks.swap(i, j);
                pos.swap(i, j);
            }
        }
    }

    (0..num_peaks).filter(|&j| pos[j] > 0).map(|j| (pos[j] as usize, peaks[j] as f32)).collect()
}

/// deskHPSDR's label text: "%d dBm" (the value truncated to an integer).
pub fn dbm_text(db: f32) -> String {
    format!("{} dBm", db as i32)
}

// ----------------------------------------------------------------------- S-meter text (meter.c)

const NUM_SWERTE: usize = 19;
const LOW_HF: [i32; NUM_SWERTE] = [-200, -121, -115, -109, -103, -97, -91, -85, -79, -73, -68, -63, -58, -53, -48, -43, -33, -23, -13];
const UP_HF: [i32; NUM_SWERTE] = [-122, -116, -110, -104, -98, -92, -86, -80, -74, -69, -64, -59, -54, -49, -44, -34, -24, -14, 0];
const LOW_UKW: [i32; NUM_SWERTE] = [-200, -141, -135, -129, -123, -117, -111, -105, -99, -93, -88, -83, -78, -73, -68, -63, -53, -43, -33];
const UP_UKW: [i32; NUM_SWERTE] = [-142, -136, -130, -124, -118, -112, -106, -100, -94, -89, -84, -79, -74, -69, -64, -54, -44, -34, 0];
const DBM2SMETER: [&str; NUM_SWERTE + 1] = [
    "no signal", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S9+5db", "S9+10db", "S9+15db", "S9+20db", "S9+25db", "S9+30db", "S9+40db", "S9+50db", "S9+60db", "out of range",
];

/// deskHPSDR `dbm2smeter[get_SWert(dbm)]`: above 30 MHz the S9 reference is -93 dBm, otherwise -73 dBm.
pub fn smeter_text(freq_hz: u64, dbm: f32) -> &'static str {
    let d = dbm as i32;
    let (lo, up) = if freq_hz > 30_000_000 { (&LOW_UKW, &UP_UKW) } else { (&LOW_HF, &UP_HF) };
    for i in 0..NUM_SWERTE {
        if d >= lo[i] && d <= up[i] {
            return DBM2SMETER[i];
        }
    }
    DBM2SMETER[NUM_SWERTE]
}

/// Places labels above the peaks, shifting them like the C code does.
/// `text_height_px` is the cairo extents.height of the label text.
#[allow(dead_code)]
pub fn layout_labels(
    peaks: &[(usize, f32)],
    panadapter_high: f32,
    panadapter_low: f32,
    height_px: f32,
    text_height_px: f32,
    text_width_fn: &dyn Fn(&str) -> f32,
) -> Vec<PeakLabel> {
    layout_labels_w(peaks, panadapter_high, panadapter_low, height_px, f32::INFINITY, text_height_px, text_width_fn, &dbm_text)
}

/// Same as `layout_labels` but with the panadapter width, used for the
/// horizontal fallback (C: `text_x + width < mywidth`), and the label text function.
#[allow(clippy::too_many_arguments)]
pub fn layout_labels_w(
    peaks: &[(usize, f32)],
    panadapter_high: f32,
    panadapter_low: f32,
    height_px: f32,
    width_px: f32,
    text_height_px: f32,
    text_width_fn: &dyn Fn(&str) -> f32,
    text_fn: &dyn Fn(f32) -> String,
) -> Vec<PeakLabel> {
    let (hi, lo) = (panadapter_high as f64, panadapter_low as f64);
    let h = height_px as f64;
    let w = width_px as f64;
    let th = text_height_px as f64;
    let mut out: Vec<PeakLabel> = Vec::with_capacity(peaks.len());
    for &(px, db) in peaks {
        let text = text_fn(db);
        let tw = text_width_fn(&text) as f64;
        let mut x = px as f64;
        let mut y = ((hi - db as f64) * h / (hi - lo)).floor() - 5.0;
        if y < th {
            y = th;
        }
        for prev in &out {
            let (px_, py_) = (prev.x_px as f64, prev.y_px as f64);
            if px_ >= 0.0 && py_ >= 0.0 {
                let dx = (x - px_).abs();
                let dy = (y - py_).abs();
                if dy < th && dx < tw {
                    if y + th < h {
                        y += th + 5.0;
                    } else if y - th > 0.0 {
                        y -= th + 5.0;
                    } else if x + tw < w {
                        x += tw + 5.0;
                    } else if x - tw > 0.0 {
                        x -= tw + 5.0;
                    }
                }
            }
        }
        out.push(PeakLabel { x_px: x as f32, y_px: y as f32, value_db: db, text });
    }
    out
}

// ----------------------------------------------------------------------- state

struct NoiseCache {
    level: f64,
    percentile: i32,
    at: Instant,
    tx: bool,
}

/// Per-panadapter runtime state of the peak labels (noise level cache) and of Peaks & Hold (one buffer for RX, one for TX).
#[derive(Default)]
pub struct PeakState {
    pub hold_rx: PeakHold,
    pub hold_tx: PeakHold,
    noise: Option<NoiseCache>,
}

impl PeakState {
    /// deskHPSDR's pan_peak_noise_*: recomputed when the percentile changed or at most once per second.
    pub fn noise_level(&mut self, row: &[f32], percentile: i32, tx: bool) -> f64 {
        let stale = match &self.noise {
            None => true,
            Some(c) => c.percentile != percentile || c.tx != tx || c.at.elapsed() >= Duration::from_secs(1),
        };
        if stale {
            self.noise = Some(NoiseCache { level: noise_level_of(row, percentile), percentile, at: Instant::now(), tx });
        }
        self.noise.as_ref().map(|c| c.level).unwrap_or(0.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HoldParams {
    /// 1 = Peaks hold, 2 = Peaks decay.
    pub mode: u8,
    pub hold_sec: f32,
    pub drop_db_per_sec: f32,
}

/// Peaks & Hold buffer (rx_panadapter.c PAN_PEAK_HOLD): one value and one age per displayed bin.
#[derive(Default)]
pub struct PeakHold {
    buf: Vec<f32>,
    age: Vec<u16>,
    /// Frequency of the left edge of the display and Hz per bin when the buffer was last aligned.
    min_display: f64,
    hz_per_bin: f64,
    /// 0 = no valid buffer; 1, 2 = RX modes; 3 = TX (fast attack, slow release).
    mode: u8,
    last_rev: Option<u64>,
}

impl PeakHold {
    /// Called while the feature is off: the next enable starts from an empty buffer.
    pub fn reset(&mut self) {
        if self.mode != 0 || !self.buf.is_empty() {
            self.buf.clear();
            self.age.clear();
            self.mode = 0;
            self.last_rev = None;
        }
    }

    pub fn values(&self) -> &[f32] {
        &self.buf
    }

    /// `row` = the displayed bins (with the display correction), `rev` the analyzer's row counter (one update per new
    /// row), `min_display_hz`/`hz_per_bin` the frequency mapping of the bins. `tx`: the TX variant of tx_panadapter.c.
    pub fn update(&mut self, row: &[f32], rev: u64, p: &HoldParams, min_display_hz: f64, hz_per_bin: f64, tx: bool) {
        let n = row.len();
        if n == 0 {
            return;
        }
        let mode = if tx { 3 } else if p.mode == 1 { 1 } else { 2 };
        let span_changed = (self.hz_per_bin - hz_per_bin).abs() > 1e-6 * hz_per_bin.abs().max(1e-9);
        if self.buf.len() != n || self.mode != mode || span_changed {
            // Cleared: mode change, feature toggled, bin count / span / zoom change.
            self.buf.clear();
            self.age.clear();
            if tx {
                self.buf.extend_from_slice(row); // avoids a ramp-in artifact, like tx_panadapter.c
            } else {
                self.buf.resize(n, -200.0);
            }
            self.age.resize(n, u16::MAX);
            self.mode = mode;
            self.min_display = min_display_hz;
            self.hz_per_bin = hz_per_bin;
            self.last_rev = None;
        } else if hz_per_bin > 0.0 {
            // Follow the displayed bins: shift when the left edge moved by whole bins.
            let dp = ((min_display_hz - self.min_display) / hz_per_bin).round() as i64;
            if dp != 0 {
                self.shift(dp);
                self.min_display = min_display_hz;
            }
        }
        if self.last_rev == Some(rev) {
            return;
        }
        self.last_rev = Some(rev);
        match mode {
            1 => {
                for i in 0..n {
                    if row[i] > self.buf[i] {
                        self.buf[i] = row[i];
                    }
                    self.age[i] = u16::MAX;
                }
            }
            2 => {
                let hold_frames = (p.hold_sec * ANALYZER_FPS + 0.5) as i32;
                let decay = p.drop_db_per_sec / ANALYZER_FPS;
                for i in 0..n {
                    let cur = row[i];
                    let mut peak = self.buf[i];
                    let mut age = self.age[i];
                    if cur > peak {
                        peak = cur;
                        age = 0;
                    } else {
                        if age < u16::MAX {
                            age += 1;
                        }
                        if age as i32 > hold_frames {
                            peak -= decay;
                        }
                    }
                    self.buf[i] = peak;
                    self.age[i] = age;
                }
            }
            _ => {
                let decay = (p.drop_db_per_sec / ANALYZER_FPS).max(0.0);
                for i in 0..n {
                    let cur = row[i];
                    let prev = self.buf[i];
                    self.buf[i] = if cur >= prev {
                        cur
                    } else {
                        let v = prev - decay;
                        if v > cur {
                            v
                        } else {
                            cur
                        }
                    };
                }
            }
        }
    }

    /// dp > 0: the content moves left (new[x] = old[x + dp]); dp < 0: right. Vacated bins are empty (-200).
    fn shift(&mut self, dp: i64) {
        let n = self.buf.len() as i64;
        if dp.abs() >= n {
            self.buf.iter_mut().for_each(|v| *v = -200.0);
            self.age.iter_mut().for_each(|a| *a = u16::MAX);
            return;
        }
        let k = dp.unsigned_abs() as usize;
        let len = n as usize;
        if dp > 0 {
            self.buf.copy_within(k.., 0);
            self.age.copy_within(k.., 0);
            self.buf[len - k..].iter_mut().for_each(|v| *v = -200.0);
            self.age[len - k..].iter_mut().for_each(|a| *a = u16::MAX);
        } else {
            self.buf.copy_within(..len - k, k);
            self.age.copy_within(..len - k, k);
            self.buf[..k].iter_mut().for_each(|v| *v = -200.0);
            self.age[..k].iter_mut().for_each(|a| *a = u16::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum() -> Vec<f32> {
        let mut s = vec![-120.0f32; 480];
        for (i, v) in s.iter_mut().enumerate() {
            *v += (i % 3) as f32 * 0.5; // low ripple
        }
        s[100] = -50.0;
        s[105] = -40.0; // neighbour of 100, within ignore range (20)
        s[200] = -60.0;
        s[300] = -70.0;
        s[400] = -80.0;
        s
    }

    fn params() -> PeakParams {
        PeakParams { on: true, ..Default::default() }
    }

    #[test]
    fn top_n_and_neighbour_suppression() {
        let s = spectrum();
        let r = find_peaks(&s, 480, None, &params());
        let px: Vec<usize> = r.iter().map(|p| p.0).collect();
        assert_eq!(px, vec![105, 200, 300, 400]);
        assert!(r.windows(2).all(|w| w[0].1 >= w[1].1));
        let mut p = params();
        p.num_peaks = 2;
        let r = find_peaks(&s, 480, None, &p);
        assert_eq!(r.iter().map(|p| p.0).collect::<Vec<_>>(), vec![105, 200]);
    }

    #[test]
    fn noise_floor_hides() {
        let mut s = spectrum();
        s[300] = -118.0; // below noise level (80th percentile -119 + 3 = -116)
        let r = find_peaks(&s, 480, None, &params());
        assert!(!r.iter().any(|p| p.0 == 300));
        let mut p = params();
        p.hide_noise = false;
        p.num_peaks = 8;
        p.ignore_range_divider = 480;
        let r = find_peaks(&s, 480, None, &p);
        assert!(r.iter().any(|p| p.0 == 300));
    }

    #[test]
    fn passband_restriction() {
        let s = spectrum();
        let mut p = params();
        p.in_passband = true;
        let r = find_peaks(&s, 480, Some((150, 350)), &p);
        assert_eq!(r.iter().map(|p| p.0).collect::<Vec<_>>(), vec![200, 300]);
    }

    #[test]
    fn off_gives_nothing() {
        assert!(find_peaks(&spectrum(), 480, None, &PeakParams::default()).is_empty());
    }

    #[test]
    fn label_y_mapping() {
        let l = layout_labels(&[(200, -60.0)], 0.0, -120.0, 300.0, 10.0, &|_| 40.0);
        // floor(60*300/120) - 5 = 145
        assert_eq!(l[0].y_px, 145.0);
        assert_eq!(l[0].x_px, 200.0);
        assert_eq!(l[0].text, "-60 dBm");
        // clamp to text height at the top
        let l = layout_labels(&[(200, 0.0)], 0.0, -120.0, 300.0, 10.0, &|_| 40.0);
        assert_eq!(l[0].y_px, 10.0);
    }

    #[test]
    fn labels_do_not_overlap() {
        // three close peaks of similar level
        let peaks = [(100usize, -60.0f32), (110, -60.5), (120, -61.0)];
        let (th, tw) = (10.0f32, 40.0f32);
        let l = layout_labels(&peaks, 0.0, -120.0, 300.0, th, &|_| tw);
        for a in 0..l.len() {
            for b in (a + 1)..l.len() {
                let dx = (l[a].x_px - l[b].x_px).abs();
                let dy = (l[a].y_px - l[b].y_px).abs();
                assert!(dy >= th || dx >= tw, "overlap {:?} {:?}", l[a], l[b]);
            }
        }
    }

    #[test]
    fn smeter_labels() {
        assert_eq!(smeter_text(7_000_000, -73.0), "S9");
        assert_eq!(smeter_text(7_000_000, -60.0), "S9+15db");
        assert_eq!(smeter_text(145_000_000, -93.0), "S9");
        assert_eq!(smeter_text(7_000_000, -130.0), "no signal");
    }

    #[test]
    fn hold_decay_and_shift() {
        let p = HoldParams { mode: 2, hold_sec: 0.2, drop_db_per_sec: 10.0 };
        let mut h = PeakHold::default();
        let mut row = vec![-100.0f32; 8];
        row[3] = -50.0;
        h.update(&row, 1, &p, 1000.0, 10.0, false);
        assert_eq!(h.values()[3], -50.0);
        row[3] = -100.0;
        for rev in 2..8 {
            h.update(&row, rev, &p, 1000.0, 10.0, false);
        }
        assert!(h.values()[3] < -50.0 && h.values()[3] > -100.0);
        // the display moved by 2 bins: the peak follows
        h.update(&row, 8, &p, 1020.0, 10.0, false);
        assert!(h.values()[1] < -50.0 && h.values()[1] > -100.0);
        // mode change clears
        let p1 = HoldParams { mode: 1, ..p };
        h.update(&row, 9, &p1, 1020.0, 10.0, false);
        assert_eq!(h.values()[1], -100.0);
    }
}
