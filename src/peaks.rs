//! Panadapter peak labels, ported from deskHPSDR (tx_panadapter.c / rx_panadapter.c).
//! Pure functions, std only.

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
            noise_percentile: 50,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeakLabel {
    /// Horizontal centre of the label.
    pub x_px: f32,
    /// Text baseline.
    pub y_px: f32,
    pub value_db: f32,
    pub text: String,
}

/// Returns (pixel, dB) of up to `num_peaks` peaks, strongest first.
/// Returns an empty vector when `p.on` is false.
pub fn find_peaks(
    samples_px: &[f32],
    width_px: usize,
    passband_px: Option<(usize, usize)>,
    p: &PeakParams,
) -> Vec<(usize, f32)> {
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

    let mut noise_level = 0.0f64;
    if p.hide_noise {
        let mut sorted: Vec<f64> = samples_px[..width].iter().map(|&v| v as f64).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((p.noise_percentile as f64 / 100.0) * width as f64) as usize;
        noise_level = sorted[idx.min(width - 1)] + 3.0;
    }

    let (left, right) = match (p.in_passband, passband_px) {
        (true, Some((l, r))) => (l as i64, r as i64),
        _ => (0, w),
    };

    for i in 1..(w - 1).max(1) {
        if i < left || i > right {
            continue;
        }
        let iu = i as usize;
        let s = samples_px[iu] as f64;
        if (!p.hide_noise || s >= noise_level)
            && s > samples_px[iu - 1] as f64
            && s > samples_px[iu + 1] as f64
        {
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

    (0..num_peaks)
        .filter(|&j| pos[j] > 0)
        .map(|j| (pos[j] as usize, peaks[j] as f32))
        .collect()
}

/// Places labels above the peaks, shifting them like the C code does.
/// `text_height_px` is the cairo extents.height of the label text.
pub fn layout_labels(
    peaks: &[(usize, f32)],
    panadapter_high: f32,
    panadapter_low: f32,
    height_px: f32,
    text_height_px: f32,
    text_width_fn: &dyn Fn(&str) -> f32,
) -> Vec<PeakLabel> {
    layout_labels_w(
        peaks,
        panadapter_high,
        panadapter_low,
        height_px,
        f32::INFINITY,
        text_height_px,
        text_width_fn,
    )
}

/// Same as `layout_labels` but with the panadapter width, used for the
/// horizontal fallback (C: `text_x + width < mywidth`). With an infinite
/// width the "move right" branch is always taken, as it would be for a
/// wide display.
pub fn layout_labels_w(
    peaks: &[(usize, f32)],
    panadapter_high: f32,
    panadapter_low: f32,
    height_px: f32,
    width_px: f32,
    text_height_px: f32,
    text_width_fn: &dyn Fn(&str) -> f32,
) -> Vec<PeakLabel> {
    let (hi, lo) = (panadapter_high as f64, panadapter_low as f64);
    let h = height_px as f64;
    let w = width_px as f64;
    let th = text_height_px as f64;
    let mut out: Vec<PeakLabel> = Vec::new();
    for &(px, db) in peaks {
        let text = format!("{:.1}", db);
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
        out.push(PeakLabel {
            x_px: x as f32,
            y_px: y as f32,
            value_db: db,
            text,
        });
    }
    out
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
        s[300] = -118.0; // below noise level (median -119 + 3 = -116)
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
        assert_eq!(l[0].text, "-60.0");
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
}
