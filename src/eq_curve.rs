//! Equalizer plot maths (deskHPSDR "WDSP EQ Menu" port).
//!
//! Pure functions (std only) porting `tx_eq_graph.c` / `rx_eq_graph.c` of deskHPSDR and the
//! NURBS code path of WDSP (`nurbs.c`, `eq_impulse()` in `eq.c`). Plot coordinates are
//! "plot-local": origin (0,0) is the top-left corner of the plotting rectangle, i.e. the
//! deskHPSDR widget coordinate minus (`PAD_LEFT`, `PAD_TOP`); `plotw`/`ploth` are the
//! rectangle's size (widget size minus paddings, see `plot_size`).
#![allow(dead_code)]

/// `TX_EQ_FMIN` -- lowest plotted frequency, Hz.
pub const FMIN: f64 = 10.0;
/// `TX_EQ_FMAX` -- highest plotted frequency, Hz.
pub const FMAX: f64 = 16000.0;
/// `TX_EQ_GMIN` -- lowest plotted gain, dB.
pub const GMIN: f64 = -20.0;
/// `TX_EQ_GMAX` -- highest plotted gain, dB.
pub const GMAX: f64 = 20.0;
/// `TX_EQ_PAD_LEFT`
pub const PAD_LEFT: f64 = 46.0;
/// `TX_EQ_PAD_RIGHT`
pub const PAD_RIGHT: f64 = 14.0;
/// `TX_EQ_PAD_TOP`
pub const PAD_TOP: f64 = 12.0;
/// `TX_EQ_PAD_BOTTOM`
pub const PAD_BOTTOM: f64 = 30.0;
/// `TX_EQ_POINT_RADIUS` -- drawn radius of a control point.
pub const POINT_RADIUS: f64 = 5.0;
/// Hit radius of `point_at` (`11.0 * 11.0` squared distance).
pub const HIT_RADIUS: f64 = 11.0;
/// `TX_EQ_POINTS`
pub const POINTS: usize = 12;
/// `TX_EQ_DRAW_POINTS` -- number of samples of the NURBS curve.
pub const DRAW_POINTS: usize = 1024;

/// Gain grid lines (dB) of `draw_cb`.
pub const GAIN_LINES: [i32; 5] = [-20, -10, 0, 10, 20];
/// Their labels (`"%+d"`).
pub const GAIN_LABELS: [&str; 5] = ["-20", "-10", "+0", "+10", "+20"];
/// Frequency grid lines (Hz) of `draw_cb`.
pub const FREQ_LINES: [i32; 8] = [10, 30, 100, 300, 1000, 3000, 10000, 16000];
/// Their labels.
pub const FREQ_LABELS: [&str; 8] = ["10", "30", "100", "300", "1k", "3k", "10k", "16k"];

/// `degrees[]` of `degree_changed_cb`: combo index -> WDSP curve degree.
pub const CURVE_DEGREES: [u8; 5] = [0, 1, 3, 5, 7];
/// Combo labels of the "Curve:" box.
pub const CURVE_LABELS: [&str; 5] =
    ["Legacy linear", "Linear (1)", "Cubic (3)", "Degree 5", "Degree 7"];

/// Combo index -> degree (out of range -> 0).
pub fn curve_degree_for_index(i: usize) -> u8 {
    CURVE_DEGREES.get(i).copied().unwrap_or(0)
}

/// Degree -> combo index (unknown degree -> 0, "Legacy linear", as in `tx_eq_graph_create`).
pub fn index_for_curve_degree(d: u8) -> usize {
    CURVE_DEGREES.iter().position(|&x| x == d).unwrap_or(0)
}

/// Plot rectangle size from the widget size (`width - PAD_LEFT - PAD_RIGHT`, `height - PAD_TOP - PAD_BOTTOM`).
pub fn plot_size(width: f64, height: f64) -> (f64, f64) {
    (width - PAD_LEFT - PAD_RIGHT, height - PAD_TOP - PAD_BOTTOM)
}

fn clampd(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// `freq_to_x` (without the `PAD_LEFT` offset): log scale, frequency clamped to FMIN..FMAX.
pub fn x_for_freq(f: f64, plotw: f64) -> f64 {
    let lo = FMIN.log10();
    let hi = FMAX.log10();
    let f = clampd(f, FMIN, FMAX);
    (f.log10() - lo) * plotw / (hi - lo)
}

/// `x_to_freq` (x relative to the plot's left edge); `t` clamped to 0..1.
pub fn freq_for_x(x: f64, plotw: f64) -> f64 {
    let lo = FMIN.log10();
    let hi = FMAX.log10();
    let t = clampd(x / plotw, 0.0, 1.0);
    10f64.powf(lo + t * (hi - lo))
}

/// `gain_to_y` (without the `PAD_TOP` offset): +20 dB at y=0, -20 dB at y=ploth.
pub fn y_for_gain(g: f64, ploth: f64) -> f64 {
    let g = clampd(g, GMIN, GMAX);
    (GMAX - g) * ploth / (GMAX - GMIN)
}

/// `y_to_gain` (y relative to the plot's top edge); `t` clamped to 0..1.
pub fn gain_for_y(y: f64, ploth: f64) -> f64 {
    let t = clampd(y / ploth, 0.0, 1.0);
    GMAX - t * (GMAX - GMIN)
}

/// `motion_cb` drag rule. `idx` is 0..12 (deskHPSDR's `selected - 1`). Frequency is clamped to
/// `[prev + 10, next - 10]` (10 Hz for the first low limit, 16000 for the last high limit), THEN
/// rounded to a multiple of 10 Hz (`round(f / 10.0) * 10.0`), gain rounded to whole dB (clamped
/// -20..20 as `y_to_gain` does in the C code).
///
/// Quirk, exactly as in C: when a neighbour is not a multiple of 10 (e.g. next = 1005, so the
/// limit is 995), the final rounding can move the result 5 Hz beyond the limit (995 -> 1000),
/// leaving only 5 Hz spacing to that neighbour. The result itself is always a multiple of 10.
pub fn drag_clamp(freqs: &[i32; 12], idx: usize, f_raw: f64, g_raw: f64) -> (i32, i32) {
    let idx = idx.min(POINTS - 1);
    let flo = if idx > 0 { freqs[idx - 1] as f64 + 10.0 } else { FMIN };
    let fhi = if idx < POINTS - 1 { freqs[idx + 1] as f64 - 10.0 } else { FMAX };
    let mut f = clampd(f_raw, flo, fhi);
    f = (f / 10.0).round() * 10.0;
    let g = clampd(g_raw, GMIN, GMAX).round();
    (f as i32, g as i32)
}

/// `freq_changed_cb` clamp of the frequency spin box: `[prev + 10, next - 10]`
/// (10 / 16000 at the ends). Not snapped.
pub fn clamp_freq_for_spin(freqs: &[i32; 12], idx: usize, f: i32) -> i32 {
    let idx = idx.min(POINTS - 1);
    let lo = if idx > 0 { freqs[idx - 1] + 10 } else { 10 };
    let hi = if idx < POINTS - 1 { freqs[idx + 1] - 10 } else { 16000 };
    // C order: raise to lo first, then cap at hi (hi wins when lo > hi).
    let mut c = f;
    if c < lo {
        c = lo;
    }
    if c > hi {
        c = hi;
    }
    c
}

/// `point_at`: nearest point within `radius` (squared distance `<=`, so later index wins ties).
pub fn nearest_point(points: &[(f64, f64)], x: f64, y: f64, radius: f64) -> Option<usize> {
    let mut best = None;
    let mut best_d2 = radius * radius;
    for (i, &(px, py)) in points.iter().enumerate() {
        let (dx, dy) = (x - px, y - py);
        let d2 = dx * dx + dy * dy;
        if d2 <= best_d2 {
            best = Some(i);
            best_d2 = d2;
        }
    }
    best
}

/// Legacy mode (degree 0): the 12 points joined in index order, (Hz, dB).
pub fn legacy_polyline(freqs: &[i32; 12], gains: &[i32; 12]) -> Vec<(f64, f64)> {
    (0..POINTS).map(|i| (freqs[i] as f64, gains[i] as f64)).collect()
}

// ---- WDSP nurbs.c port ----------------------------------------------------------------------

fn find_span(n: usize, p: usize, uk: &[f64], u: f64) -> usize {
    if u >= uk[n + 1] {
        return n;
    }
    let mut s = p;
    while s < n && u >= uk[s + 1] {
        s += 1;
    }
    s
}

fn basis_funs(s: usize, p: usize, uk: &[f64], u: f64) -> [f64; 32] {
    let mut prev = [0.0f64; 32];
    let mut curr = [0.0f64; 32];
    prev[0] = 1.0;
    for d in 1..=p {
        for c in curr.iter_mut().take(d + 1) {
            *c = 0.0;
        }
        for r in 0..=d {
            if r > 0 {
                let u_lo = uk[s - d + r];
                let u_hi = uk[s + r];
                let denom = u_hi - u_lo;
                let alpha = if denom > 1e-300 { (u - u_lo) / denom } else { 0.0 };
                curr[r] += alpha * prev[r - 1];
            }
            if r < d {
                let u_lo = uk[s - d + r + 1];
                let u_hi = uk[s + r + 1];
                let denom = u_hi - u_lo;
                let beta = if denom > 1e-300 { (u - u_lo) / denom } else { 0.0 };
                curr[r] += (1.0 - beta) * prev[r];
            }
        }
        prev[..=d].copy_from_slice(&curr[..=d]);
    }
    prev
}

/// `NURBSpoint()`.
fn nurbs_point(
    n: usize,
    p: usize,
    rational: bool,
    uk: &[f64],
    cp: &[(f64, f64)],
    w: &[f64],
    u: f64,
) -> (f64, f64) {
    let s = find_span(n, p, uk, u);
    let nn = basis_funs(s, p, uk, u);
    let (mut x, mut y) = (0.0, 0.0);
    if rational {
        let mut wsum = 0.0;
        for i in 0..=p {
            wsum += nn[i] * w[s - p + i];
        }
        if wsum < 1e-300 {
            wsum = 1e-300;
        }
        for i in 0..=p {
            let ri = nn[i] * w[s - p + i] / wsum;
            x += ri * cp[s - p + i].0;
            y += ri * cp[s - p + i].1;
        }
    } else {
        for i in 0..=p {
            x += nn[i] * cp[s - p + i].0;
            y += nn[i] * cp[s - p + i].1;
        }
    }
    (x, y)
}

/// `Ucalc()` with umethod 0 (knots from the control points' x positions).
fn ucalc0(n: usize, p: usize, cp: &[(f64, f64)]) -> Vec<f64> {
    let m = n + p + 1;
    let total = m + 1;
    let unique = total - 2 * p;
    let mut uk = vec![0.0; total];
    let x_range = cp[n].0 - cp[0].0;
    let mut i = 0;
    while i <= p {
        uk[i] = 0.0;
        i += 1;
    }
    let mut j = 1;
    while i < p + unique - 1 {
        let frac = j as f64 / (unique - 1) as f64;
        let pos = frac * n as f64;
        let fl = pos as usize; // (int) truncation, pos >= 0
        let pf = pos - fl as f64;
        let hi = cp[(fl + 1).min(n)].0; // C reads CP[2*fl+2]; same unless fl == n (pf == 0)
        uk[i] = ((1.0 - pf) * cp[fl].0 + pf * hi - cp[0].0) / x_range;
        i += 1;
        j += 1;
    }
    while i < total {
        uk[i] = 1.0;
        i += 1;
    }
    uk
}

/// WDSP NURBS EQ curve as returned by `GetTXAEQDraw`/`GetRXAEQDraw` (the `Xs`,`Ys` arrays, 1024 points
/// at u = i/1023), converted to (Hz, dB) like the plotting loop of `draw_cb`
/// (`f = X * 0.5 * samplerate`, non-finite and outside FMIN..FMAX dropped). The dB EXCLUDE the preamp.
///
/// `eq_impulse()`: control points `fp = clamp(2*F/samplerate, 0, 1)`, pairs sorted by F (gain follows),
/// weights NOT re-sorted; umethod 0, degree `deg`. Returns empty on invalid input
/// (`checkSplineInputs`: deg >= number of points, rational with a weight <= 0; `eq_impulse`: deg > 16),
/// and for fewer than 2 points or a non-positive sample rate.
pub fn nurbs_curve(
    freqs: &[i32; 12],
    gains: &[i32; 12],
    deg: u8,
    rational: bool,
    weights: &[f64; 12],
    samplerate_hz: f64,
) -> Vec<(f64, f64)> {
    let ncp = freqs.len();
    let p = deg as usize;
    if p < 1 || p > 16 || p >= ncp || ncp < 2 || !(samplerate_hz > 0.0) {
        return Vec::new();
    }
    if rational && weights.iter().any(|&w| !(w > 0.0)) {
        return Vec::new();
    }
    let mut pairs: Vec<(f64, f64)> = (0..ncp)
        .map(|i| ((2.0 * freqs[i] as f64 / samplerate_hz).clamp(0.0, 1.0), gains[i] as f64))
        .collect();
    // qsort by x only; stable sort keeps equal-x order (qsort order is unspecified for ties).
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = ncp - 1;
    let uk = ucalc0(n, p, &pairs);
    let m = n + p + 1;
    let mut out = Vec::with_capacity(DRAW_POINTS);
    for i in 0..DRAW_POINTS {
        let u = (i as f64 / (DRAW_POINTS - 1) as f64) * (uk[m] - uk[0]);
        let (x, y) = nurbs_point(n, p, rational, &uk, &pairs, weights, u);
        let f = x * (0.5 * samplerate_hz);
        if !f.is_finite() || !y.is_finite() || f < FMIN || f > FMAX {
            continue;
        }
        out.push((f, y));
    }
    out
}

// ---- wrappers on EqualizerParams --------------------------------------------------------------

/// Curve to draw for the given parameters: legacy polyline when `curve_deg == 0`, otherwise the NURBS
/// curve at `samplerate_hz` (the DSP rate of the channel). Excludes the 12-band preamp.
pub fn curve_for_params(p: &crate::spectrum::EqualizerParams, samplerate_hz: f64) -> Vec<(f64, f64)> {
    if p.curve_deg == 0 {
        legacy_polyline(&p.freqs_12_hz, &p.bands_12_db)
    } else {
        nurbs_curve(&p.freqs_12_hz, &p.bands_12_db, p.curve_deg, p.nurbs_r, &p.weights12(), samplerate_hz)
    }
}

/// `drag_clamp` on the parameters' frequencies.
pub fn drag_clamp_params(p: &crate::spectrum::EqualizerParams, idx: usize, f_raw: f64, g_raw: f64) -> (i32, i32) {
    drag_clamp(&p.freqs_12_hz, idx, f_raw, g_raw)
}

/// `clamp_freq_for_spin` on the parameters' frequencies.
pub fn clamp_freq_for_spin_params(p: &crate::spectrum::EqualizerParams, idx: usize, f: i32) -> i32 {
    clamp_freq_for_spin(&p.freqs_12_hz, idx, f)
}

/// Plot-local positions of the 12 control points, for `nearest_point`.
pub fn point_positions(p: &crate::spectrum::EqualizerParams, plotw: f64, ploth: f64) -> Vec<(f64, f64)> {
    (0..POINTS)
        .map(|i| (x_for_freq(p.freqs_12_hz[i] as f64, plotw), y_for_gain(p.bands_12_db[i] as f64, ploth)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: [i32; 12] = [30, 100, 200, 300, 500, 800, 1200, 1800, 2600, 3500, 5000, 8000];
    const G: [i32; 12] = [-5, 3, 6, 2, -4, 0, 5, 9, -3, 4, -8, 1];
    const W1: [f64; 12] = [1.0; 12];

    #[test]
    fn mapping_round_trip() {
        let (w, h) = (500.0, 140.0);
        for f in [10.0, 30.0, 123.0, 1000.0, 9999.0, 16000.0] {
            let x = x_for_freq(f, w);
            assert!((freq_for_x(x, w) - f).abs() < 1e-6 * f);
        }
        assert_eq!(x_for_freq(10.0, w), 0.0);
        assert!((x_for_freq(16000.0, w) - w).abs() < 1e-9);
        for g in [-20.0, -7.0, 0.0, 13.0, 20.0] {
            assert!((gain_for_y(y_for_gain(g, h), h) - g).abs() < 1e-9);
        }
        assert_eq!(y_for_gain(20.0, h), 0.0);
        assert_eq!(y_for_gain(-20.0, h), h);
        assert_eq!(freq_for_x(-5.0, w), 10.0);
        assert_eq!(gain_for_y(1e9, h), -20.0);
        assert_eq!(plot_size(560.0, 180.0), (500.0, 138.0));
    }

    #[test]
    fn drag_examples() {
        let mut f = F;
        assert_eq!(drag_clamp(&f, 2, 5000.0, 99.0), (290, 20));
        assert_eq!(drag_clamp(&f, 2, 0.0, -99.4), (110, -20));
        assert_eq!(drag_clamp(&f, 0, 1.0, 0.4), (10, 0));
        assert_eq!(drag_clamp(&f, 11, 99999.0, 2.6), (16000, 3));
        assert_eq!(drag_clamp(&f, 3, 233.0, 1.5).0 % 10, 0);
        // neighbour at 1005: limit 995, rounds to 1000 -> only 5 Hz spacing (C quirk)
        f[6] = 1005;
        let (r, _) = drag_clamp(&f, 5, 1100.0, 0.0);
        assert_eq!(r % 10, 0);
        assert_eq!(r, 1000);
        assert_eq!(f[6] - r, 5);
        // previous neighbour 1005: limit 1015 -> 1020 (spacing 15, fine)
        f[4] = 1005;
        f[5] = 1500;
        assert_eq!(drag_clamp(&f, 5, 0.0, 0.0).0, 1020);
    }

    #[test]
    fn spin_clamp() {
        assert_eq!(clamp_freq_for_spin(&F, 0, 1), 10);
        assert_eq!(clamp_freq_for_spin(&F, 0, 500), 90);
        assert_eq!(clamp_freq_for_spin(&F, 3, 100), 210);
        assert_eq!(clamp_freq_for_spin(&F, 11, 20000), 16000);
        assert_eq!(clamp_freq_for_spin(&F, 11, 100), 5010);
    }

    #[test]
    fn nearest() {
        let pts = [(10.0, 10.0), (20.0, 10.0), (100.0, 100.0)];
        assert_eq!(nearest_point(&pts, 15.0, 10.0, 11.0), Some(1)); // tie -> later index
        assert_eq!(nearest_point(&pts, 11.0, 10.0, 11.0), Some(0));
        assert_eq!(nearest_point(&pts, 60.0, 60.0, 11.0), None);
    }

    #[test]
    fn combo_table() {
        assert_eq!(curve_degree_for_index(2), 3);
        assert_eq!(index_for_curve_degree(7), 4);
        assert_eq!(index_for_curve_degree(2), 0);
        assert_eq!(CURVE_LABELS[0], "Legacy linear");
    }

    fn interp(c: &[(f64, f64)], x: f64) -> f64 {
        for w in c.windows(2) {
            if x >= w[0].0 && x <= w[1].0 {
                let t = (x - w[0].0) / (w[1].0 - w[0].0);
                return w[0].1 + t * (w[1].1 - w[0].1);
            }
        }
        f64::NAN
    }

    #[test]
    fn degree1_matches_polyline() {
        let c = nurbs_curve(&F, &G, 1, false, &W1, 192000.0);
        assert!(c.len() > 100);
        for i in 0..12 {
            let v = interp(&c, F[i] as f64);
            assert!((v - G[i] as f64).abs() < 0.2, "i={} v={} g={}", i, v, G[i]);
        }
    }

    #[test]
    fn degree3_endpoints_and_flat() {
        let sr = 96000.0;
        let c = nurbs_curve(&F, &G, 3, false, &W1, sr);
        let first = c.first().unwrap();
        let last = c.last().unwrap();
        assert!((first.0 - 30.0).abs() < 1e-6 && (first.1 - -5.0).abs() < 1e-9);
        assert!((last.0 - 8000.0).abs() < 1e-6 && (last.1 - 1.0).abs() < 1e-9);
        for deg in [1u8, 3, 5, 7] {
            let flat = nurbs_curve(&F, &[4; 12], deg, false, &W1, sr);
            assert!(!flat.is_empty());
            assert!(flat.iter().all(|p| (p.1 - 4.0).abs() < 1e-9));
        }
    }

    #[test]
    fn rational_unit_weights_equal_plain() {
        for deg in [1u8, 3, 5, 7] {
            let a = nurbs_curve(&F, &G, deg, false, &W1, 48000.0);
            let b = nurbs_curve(&F, &G, deg, true, &W1, 48000.0);
            assert_eq!(a.len(), b.len());
            for (p, q) in a.iter().zip(&b) {
                assert!((p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9);
            }
        }
        let mut w = W1;
        w[5] = 5.0;
        let c = nurbs_curve(&F, &G, 3, true, &w, 48000.0);
        let a = nurbs_curve(&F, &G, 3, false, &W1, 48000.0);
        assert!(c.iter().zip(&a).any(|(p, q)| (p.1 - q.1).abs() > 1e-3));
    }

    #[test]
    fn invalid_inputs() {
        assert!(nurbs_curve(&F, &G, 0, false, &W1, 48000.0).is_empty());
        assert!(nurbs_curve(&F, &G, 12, false, &W1, 48000.0).is_empty());
        assert!(nurbs_curve(&F, &G, 17, false, &W1, 48000.0).is_empty());
        let mut w = W1;
        w[3] = 0.0;
        assert!(nurbs_curve(&F, &G, 3, true, &w, 48000.0).is_empty());
        assert!(!nurbs_curve(&F, &G, 3, false, &w, 48000.0).is_empty());
        assert!(nurbs_curve(&F, &G, 3, false, &W1, 0.0).is_empty());
    }
}
