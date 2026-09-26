// SSTV (slow-scan TV) modem, adapted from SDRoxide
// (https://github.com/madmedicnl/sdroxide, crates/sdroxide-dsp/src/sstv.rs
// and crates/sdroxide-types/src/sstv.rs, upstream commit 63b0b29),
// licensed GPL-3.0-or-later. Combined here under the hpsdr-rs project,
// whose own GPL-2.0-or-later license permits combination with GPL-3.0 code
// (the combined work is GPL-3.0).
//
// Local changes from upstream: `SstvMode` (upstream: sdroxide-types) is
// folded into this file rather than pulled in as a separate crate
// dependency, unchanged apart from that move. The `fir` helpers this modem
// needs (`bandpass_taps`, `ComplexFir`) are folded into the private `fir`
// submodule at the bottom of this file, same as rtty.rs's own port --
// SDRoxide's SIMD-dispatch kernel is replaced by its plain scalar body, and
// the unused `RealFir`/`lowpass_taps`-adjacent group-delay helpers are
// dropped. `Complex32` is a local alias. The modem logic itself, RX and TX
// alike, is unchanged -- TX is ported now (rather than added later) so a
// future transmit feature does not need a second port against a
// potentially-drifted upstream, even though hpsdr-rs only wires up RX for
// now (see sstv_link.rs's own module doc comment).

//! SSTV modem: image ⇄ audio for the Scottie, Martin, Robot, Wraase SC-2 and
//! PD families.
//!
//! Transmit builds a per-mode timing plan and synthesises tones with a
//! continuous phase accumulator (the same idiom as rtty.rs). Receive runs an
//! FM discriminator to recover instantaneous frequency, detects the VIS
//! calibration header to pick the mode (or locks onto the sync cadence with
//! no VIS at all, for tuning in mid-picture), then samples pixels
//! line-by-line, re-aligning on each line's sync pulse for slant tolerance.
//!
//! Timing follows the canonical N7CXI spec (as used by PySSTV/QSSTV). Colour
//! maps to frequency by black = 1500 Hz, white = 2300 Hz; sync = 1200 Hz.

use std::f64::consts::TAU;

use fir::{bandpass_taps, ComplexFir};

pub type Complex32 = num_complex::Complex<f32>;

// ── mode vocabulary (upstream: sdroxide-types::SstvMode) ───────────────────

/// One SSTV transmission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SstvMode {
    Scottie1,
    Scottie2,
    ScottieDx,
    Martin1,
    Martin2,
    Robot72,
    Robot36,
    /// Wraase SC-2 180 -- 320x256 RGB, 711 ms a line. Very widely used on
    /// 3.730 MHz and 14.230 MHz.
    WraaseSc2_180,
    /// Wraase SC-2 120 -- the same format at 476 ms a line.
    WraaseSc2_120,
    // The PD family (Martin Bruchanov OK2MNM's, after the "PD" in the
    // original program). Four scans between syncs, carrying *two* image
    // lines: luma for each of them either side of one shared pair of
    // chroma scans. That is why they are cheap for the bandwidth -- half
    // the chroma -- and why they need a line structure none of the older
    // modes do.
    Pd50,
    Pd90,
    Pd120,
    Pd160,
    Pd180,
    Pd240,
    Pd290,
}

impl Default for SstvMode {
    fn default() -> Self {
        SstvMode::Scottie1
    }
}

impl SstvMode {
    /// All modes, in a sensible menu order.
    pub const ALL: [SstvMode; 16] = [
        SstvMode::Scottie1,
        SstvMode::Scottie2,
        SstvMode::ScottieDx,
        SstvMode::Martin1,
        SstvMode::Martin2,
        SstvMode::Robot72,
        SstvMode::Robot36,
        SstvMode::WraaseSc2_180,
        SstvMode::WraaseSc2_120,
        SstvMode::Pd50,
        SstvMode::Pd90,
        SstvMode::Pd120,
        SstvMode::Pd160,
        SstvMode::Pd180,
        SstvMode::Pd240,
        SstvMode::Pd290,
    ];

    /// Short human label for buttons/menus.
    pub fn label(self) -> &'static str {
        match self {
            SstvMode::Scottie1 => "Scottie 1",
            SstvMode::Scottie2 => "Scottie 2",
            SstvMode::ScottieDx => "Scottie DX",
            SstvMode::Martin1 => "Martin 1",
            SstvMode::Martin2 => "Martin 2",
            SstvMode::Robot72 => "Robot 72",
            SstvMode::Robot36 => "Robot 36",
            SstvMode::WraaseSc2_180 => "SC2-180",
            SstvMode::WraaseSc2_120 => "SC2-120",
            SstvMode::Pd50 => "PD50",
            SstvMode::Pd90 => "PD90",
            SstvMode::Pd120 => "PD120",
            SstvMode::Pd160 => "PD160",
            SstvMode::Pd180 => "PD180",
            SstvMode::Pd240 => "PD240",
            SstvMode::Pd290 => "PD290",
        }
    }

    /// Transmitted image size in pixels, `(width, height)`.
    pub fn dimensions(self) -> (u16, u16) {
        match self {
            SstvMode::Scottie1
            | SstvMode::Scottie2
            | SstvMode::ScottieDx
            | SstvMode::Martin1
            | SstvMode::Martin2
            | SstvMode::WraaseSc2_180
            | SstvMode::WraaseSc2_120 => (320, 256),
            SstvMode::Robot72 | SstvMode::Robot36 => (320, 240),
            SstvMode::Pd50 | SstvMode::Pd90 => (320, 256),
            SstvMode::Pd160 => (512, 400),
            SstvMode::Pd120 | SstvMode::Pd180 | SstvMode::Pd240 => (640, 496),
            SstvMode::Pd290 => (800, 616),
        }
    }

    /// How many *image* lines one transmitted line carries -- one for every
    /// mode but the PD family, which sends luma for two lines either side of
    /// a single pair of chroma scans and so covers two rows per sync pulse.
    pub fn rows_per_line(self) -> u16 {
        match self {
            SstvMode::Pd50
            | SstvMode::Pd90
            | SstvMode::Pd120
            | SstvMode::Pd160
            | SstvMode::Pd180
            | SstvMode::Pd240
            | SstvMode::Pd290 => 2,
            _ => 1,
        }
    }

    /// The 7-bit VIS code identifying this mode in the calibration header.
    pub fn vis_code(self) -> u8 {
        match self {
            SstvMode::Robot36 => 0x08,
            SstvMode::Robot72 => 0x0C,
            SstvMode::Martin2 => 0x28,
            SstvMode::Martin1 => 0x2C,
            SstvMode::WraaseSc2_180 => 0x37,
            SstvMode::Scottie2 => 0x38,
            SstvMode::Scottie1 => 0x3C,
            SstvMode::WraaseSc2_120 => 0x3F,
            SstvMode::ScottieDx => 0x4C,
            SstvMode::Pd50 => 0x5D,
            SstvMode::Pd290 => 0x5E,
            SstvMode::Pd120 => 0x5F,
            SstvMode::Pd180 => 0x60,
            SstvMode::Pd240 => 0x61,
            SstvMode::Pd160 => 0x62,
            SstvMode::Pd90 => 0x63,
        }
    }

    /// Map a decoded VIS code back to a mode, if recognised.
    pub fn from_vis(code: u8) -> Option<SstvMode> {
        SstvMode::ALL.into_iter().find(|m| m.vis_code() == code)
    }

    /// The name of a mode this build does **not** decode, for a VIS code
    /// that came through with good parity -- only the codes that are
    /// actually assigned; an unassigned one is a misread header rather than
    /// a mode.
    pub fn unsupported_name(code: u8) -> Option<&'static str> {
        Some(match code {
            0x02 => "Robot 8 B/W",
            0x04 => "Robot 24",
            0x06 => "Robot 12 B/W",
            0x0A => "Robot 24 B/W",
            0x20 => "Martin 4",
            0x24 => "Martin 3",
            0x71 => "Pasokon P3",
            0x72 => "Pasokon P5",
            0x73 => "Pasokon P7",
            _ => return None,
        })
    }
}

const BLACK_HZ: f64 = 1500.0;
const WHITE_HZ: f64 = 2300.0;
const SYNC_HZ: f64 = 1200.0;
const VIS_LEADER_HZ: f64 = 1900.0;
const VIS_BIT1_HZ: f64 = 1100.0;
const VIS_BIT0_HZ: f64 = 1300.0;

// ── FSK ID: the station's callsign, sent as tones after the picture ──
//
// The format is JE3HHT's, as published with MMSSTV and implemented by every
// SSTV program and unattended repeater that reads one. It is a 45.45 baud FSK
// stream -- 22 ms a bit -- of six-bit symbols, most significant bit first,
// carrying ASCII $20..$5F shifted down to $00..$3F. A whole ID is
// `$2A C1..CN $01 XSUM`, where the checksum is the XOR of the characters
// alone. It is preceded by a 300 ms tone and a 100 ms tone that give a
// receiver something to arm on, and the transition out of the second of
// those is what times every bit that follows.

/// The tone a `1` bit is sent on -- and the ID's start bit.
const FSKID_ONE_HZ: f64 = 1900.0;
/// The tone a `0` bit is sent on, and the 100 ms block ahead of the start bit.
const FSKID_ZERO_HZ: f64 = 2100.0;
/// The 300 ms tone that opens the ID.
const FSKID_LEADER_HZ: f64 = 1500.0;
const FSKID_LEADER_S: f64 = 0.300;
const FSKID_SYNC_S: f64 = 0.100;
/// 45.45 baud.
const FSKID_BIT_S: f64 = 0.022;
/// Header symbol: "an ID starts here".
const FSKID_HEAD: u8 = 0x2A;
/// Terminator symbol, ahead of the checksum.
const FSKID_END: u8 = 0x01;
/// How many characters of the ID go on the air.
const FSKID_MAX_CHARS: usize = 20;

/// The six-bit symbols of an FSK ID for `text`, ready to be keyed -- header,
/// characters, terminator and checksum. Empty when there is nothing to send.
#[must_use]
pub fn fsk_id_symbols(text: &str) -> Vec<u8> {
    let chars: Vec<u8> = text
        .trim()
        .to_ascii_uppercase()
        .bytes()
        .filter(|b| (0x20..=0x5F).contains(b))
        .take(FSKID_MAX_CHARS)
        .map(|b| b - 0x20)
        .collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let xsum = chars.iter().fold(0u8, |a, &c| a ^ c);
    let mut out = Vec::with_capacity(chars.len() + 3);
    out.push(FSKID_HEAD);
    out.extend_from_slice(&chars);
    out.push(FSKID_END);
    out.push(xsum);
    out
}

/// The text of a complete FSK ID symbol stream, or `None` when it is not one.
#[must_use]
pub fn fsk_id_text(symbols: &[u8]) -> Option<String> {
    let (head, rest) = symbols.split_first()?;
    if *head != FSKID_HEAD {
        return None;
    }
    let end = rest.iter().position(|&s| s == FSKID_END)?;
    let (chars, tail) = rest.split_at(end);
    if chars.is_empty() || chars.len() > FSKID_MAX_CHARS {
        return None;
    }
    let &xsum = tail.get(1)?;
    if chars.iter().fold(0u8, |a, &c| a ^ c) != xsum {
        return None;
    }
    Some(chars.iter().map(|&c| (c + 0x20) as char).collect::<String>().trim().to_string())
}

/// Frequency (Hz) for an 8-bit intensity, black->white.
fn value_to_hz(v: u8) -> f64 {
    BLACK_HZ + (v as f64 / 255.0) * (WHITE_HZ - BLACK_HZ)
}

/// Inverse of [`value_to_hz`], clamped to a byte.
fn hz_to_value(hz: f64) -> u8 {
    let v = ((hz - BLACK_HZ) / (WHITE_HZ - BLACK_HZ)) * 255.0;
    v.round().clamp(0.0, 255.0) as u8
}

// ───────────────────────────── mode timing ─────────────────────────────

/// A colour channel within a scan segment.
#[derive(Clone, Copy, PartialEq)]
enum Chan {
    R,
    G,
    B,
    /// Luma.
    Y,
    /// Luma of the *second* image row a PD line carries.
    Y2,
    /// R-Y chroma (Cr).
    Cr,
    /// B-Y chroma (Cb).
    Cb,
}

/// One timed segment of a scan line.
#[derive(Clone, Copy)]
enum Seg {
    /// Constant tone for `dur` seconds at `hz`.
    Tone { hz: f64, dur: f64 },
    /// A pixel scan of `width` samples of channel `chan`, `px` seconds each.
    Scan { chan: Chan, width: u16, px: f64 },
}

/// Per-mode parameters used to build a line plan.
struct Timing {
    sync: f64,
    sync_hz: f64,
    sep: f64,
    sep_hz: f64,
    /// Colour-channel pixel time, seconds.
    px: f64,
}

fn scottie_timing(px: f64) -> Timing {
    Timing { sync: 0.009, sync_hz: SYNC_HZ, sep: 0.0015, sep_hz: 1500.0, px }
}

fn martin_timing(px: f64) -> Timing {
    Timing { sync: 0.004_862, sync_hz: SYNC_HZ, sep: 0.000_572, sep_hz: 1500.0, px }
}

/// The PD family's pixel time, seconds. Everything else about a PD line is
/// shared: a 20 ms sync, a 2.08 ms porch, and four full-width scans.
fn pd_pixel_time(mode: SstvMode) -> f64 {
    match mode {
        SstvMode::Pd50 => 0.000_286,
        SstvMode::Pd90 => 0.000_532,
        SstvMode::Pd120 => 0.000_190,
        SstvMode::Pd160 => 0.000_382,
        SstvMode::Pd180 => 0.000_286,
        SstvMode::Pd240 => 0.000_382,
        _ => 0.000_286, // Pd290
    }
}

/// The ordered segments for one scan line of `mode` at image width `w`.
/// Robot modes carry their (per-line-varying) chroma channel via `line`.
fn line_segments(mode: SstvMode, w: u16, line: u16) -> Vec<Seg> {
    use Chan::*;
    match mode {
        SstvMode::Scottie1 | SstvMode::Scottie2 | SstvMode::ScottieDx => {
            let px = match mode {
                SstvMode::Scottie1 => 0.000_432,
                SstvMode::Scottie2 => 0.000_275_2,
                _ => 0.001_08,
            };
            let t = scottie_timing(px);
            // Scottie order: sep . G . sep . B . SYNC . sep . R.
            vec![
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: G, width: w, px: t.px },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: B, width: w, px: t.px },
                Seg::Tone { hz: t.sync_hz, dur: t.sync },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: R, width: w, px: t.px },
            ]
        }
        SstvMode::Martin1 | SstvMode::Martin2 => {
            let px = if mode == SstvMode::Martin1 { 0.000_457_6 } else { 0.000_228_8 };
            let t = martin_timing(px);
            // Martin order: SYNC . porch . G . sep . B . sep . R . sep.
            vec![
                Seg::Tone { hz: t.sync_hz, dur: t.sync },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: G, width: w, px: t.px },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: B, width: w, px: t.px },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
                Seg::Scan { chan: R, width: w, px: t.px },
                Seg::Tone { hz: t.sep_hz, dur: t.sep },
            ]
        }
        SstvMode::Robot72 => {
            // Y full width; Cr, Cb half width. 300 ms/line.
            let cw = w / 2;
            vec![
                Seg::Tone { hz: SYNC_HZ, dur: 0.009 },
                Seg::Tone { hz: 1500.0, dur: 0.003 },
                Seg::Scan { chan: Y, width: w, px: 0.000_431_25 },
                Seg::Tone { hz: 1500.0, dur: 0.0045 },
                Seg::Tone { hz: 1900.0, dur: 0.0015 },
                Seg::Scan { chan: Cr, width: cw, px: 0.000_431_25 },
                Seg::Tone { hz: 2300.0, dur: 0.0045 },
                Seg::Tone { hz: 1900.0, dur: 0.0015 },
                Seg::Scan { chan: Cb, width: cw, px: 0.000_431_25 },
            ]
        }
        SstvMode::Robot36 => {
            // Y full width; one chroma per line, alternating even=Cr / odd=Cb
            // (4:2:0). 150 ms/line. Separator frequency signals which chroma.
            let cw = w / 2;
            let even = line % 2 == 0;
            let (chan, sep_hz) = if even { (Cr, 1500.0) } else { (Cb, 2300.0) };
            vec![
                Seg::Tone { hz: SYNC_HZ, dur: 0.009 },
                Seg::Tone { hz: 1500.0, dur: 0.003 },
                Seg::Scan { chan: Y, width: w, px: 0.000_275 },
                Seg::Tone { hz: sep_hz, dur: 0.0045 },
                Seg::Tone { hz: 1900.0, dur: 0.0015 },
                Seg::Scan { chan, width: cw, px: 0.000_275 },
            ]
        }
        SstvMode::WraaseSc2_180 | SstvMode::WraaseSc2_120 => {
            // Wraase SC-2: sync, a short porch, then R, G, B at full width and
            // no separators between them -- the one common family that sends
            // red first.
            let px =
                if mode == SstvMode::WraaseSc2_180 { 0.235 / 320.0 } else { 0.156_502_506 / 320.0 };
            vec![
                Seg::Tone { hz: SYNC_HZ, dur: 0.005_522_5 },
                Seg::Tone { hz: 1500.0, dur: 0.000_5 },
                Seg::Scan { chan: R, width: w, px },
                Seg::Scan { chan: G, width: w, px },
                Seg::Scan { chan: B, width: w, px },
            ]
        }
        SstvMode::Pd50
        | SstvMode::Pd90
        | SstvMode::Pd120
        | SstvMode::Pd160
        | SstvMode::Pd180
        | SstvMode::Pd240
        | SstvMode::Pd290 => {
            // Two image rows per sync: this row's luma, then one pair of
            // chroma scans shared between the two, then the next row's luma.
            let px = pd_pixel_time(mode);
            vec![
                Seg::Tone { hz: SYNC_HZ, dur: 0.020 },
                Seg::Tone { hz: 1500.0, dur: 0.002_08 },
                Seg::Scan { chan: Y, width: w, px },
                Seg::Scan { chan: Cr, width: w, px },
                Seg::Scan { chan: Cb, width: w, px },
                Seg::Scan { chan: Y2, width: w, px },
            ]
        }
    }
}

// BT.601-ish YUV used by the Robot modes (MMSSTV coefficients).
fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (r as f64, g as f64, b as f64);
    let y = 16.0 + (65.738 * r + 129.057 * g + 25.064 * b) / 256.0;
    let cr = 128.0 + (112.439 * r - 94.154 * g - 18.285 * b) / 256.0;
    let cb = 128.0 + (-37.945 * r - 74.494 * g + 112.439 * b) / 256.0;
    (
        y.round().clamp(0.0, 255.0) as u8,
        cr.round().clamp(0.0, 255.0) as u8,
        cb.round().clamp(0.0, 255.0) as u8,
    )
}

fn yuv_to_rgb(y: u8, cr: u8, cb: u8) -> (u8, u8, u8) {
    let y = y as f64 - 16.0;
    let cr = cr as f64 - 128.0;
    let cb = cb as f64 - 128.0;
    let r = 1.164 * y + 1.596 * cr;
    let g = 1.164 * y - 0.392 * cb - 0.813 * cr;
    let b = 1.164 * y + 2.017 * cb;
    (
        r.round().clamp(0.0, 255.0) as u8,
        g.round().clamp(0.0, 255.0) as u8,
        b.round().clamp(0.0, 255.0) as u8,
    )
}

// ─────────────────────────────── transmit ──────────────────────────────

/// SSTV transmitter: turns an RGB image into a stream of audio samples.
///
/// Not wired into hpsdr-rs yet (RX-only for now -- see sstv_link.rs's module
/// doc comment) but ported now, complete, so a future TX feature is glue and
/// UI work rather than a second DSP port.
pub struct SstvTx {
    rate: f64,
    /// Flattened plan of (frequency, sample-count) tone runs. Scans are
    /// expanded to one entry per pixel up front -- a 320x256 image is
    /// ~250k entries, a few MB, produced once per transmission.
    plan: Vec<(f64, u32)>,
    idx: usize,
    left: u32,
    cur_hz: f64,
    phase: f64,
    total: u64,
    done: u64,
}

impl SstvTx {
    /// Build a transmitter for `mode` from interleaved RGB (`rgb.len() ==
    /// w*h*3`) at output sample `rate`, with an optional transmit clock trim
    /// `ppm` (parts-per-million; stretches/compresses the image time-scale to
    /// null out slant against a receiver whose clock differs -- tone
    /// frequencies are unaffected).
    pub fn new(mode: SstvMode, rgb: &[u8], w: u16, h: u16, rate: f64, ppm: f32) -> Self {
        let mut plan: Vec<(f64, u32)> = Vec::new();
        let timing_rate = rate * (1.0 + ppm as f64 / 1_000_000.0);
        let mut emitted: i64 = 0;
        let mut t_exact: f64 = 0.0;
        let mut push = |plan: &mut Vec<(f64, u32)>, hz: f64, dur: f64| {
            t_exact += dur * timing_rate;
            let target = t_exact.round() as i64;
            let n = (target - emitted).max(0);
            emitted = target;
            if n > 0 {
                plan.push((hz, n as u32));
            }
        };
        let px_at = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * w as usize + x) * 3;
            (rgb[i], rgb[i + 1], rgb[i + 2])
        };

        // VIS calibration header.
        push(&mut plan, VIS_LEADER_HZ, 0.300);
        push(&mut plan, SYNC_HZ, 0.010);
        push(&mut plan, VIS_LEADER_HZ, 0.300);
        push(&mut plan, SYNC_HZ, 0.030); // start bit
        let code = mode.vis_code();
        let mut parity = 0u8;
        for bit in 0..7 {
            let one = (code >> bit) & 1 == 1;
            parity ^= one as u8;
            push(&mut plan, if one { VIS_BIT1_HZ } else { VIS_BIT0_HZ }, 0.030);
        }
        push(&mut plan, if parity == 1 { VIS_BIT1_HZ } else { VIS_BIT0_HZ }, 0.030);
        push(&mut plan, SYNC_HZ, 0.030); // stop bit

        // Scottie sends a 9 ms starting sync before the very first line.
        if matches!(mode, SstvMode::Scottie1 | SstvMode::Scottie2 | SstvMode::ScottieDx) {
            push(&mut plan, SYNC_HZ, 0.009);
        }

        // One pass per *transmitted* line, which is two image rows in the PD
        // family and one everywhere else.
        let rows = mode.rows_per_line().max(1) as usize;
        for y in (0..h as usize).step_by(rows) {
            let y2 = (y + 1).min(h as usize - 1);
            for seg in line_segments(mode, w, y as u16) {
                match seg {
                    Seg::Tone { hz, dur } => push(&mut plan, hz, dur),
                    Seg::Scan { chan, width, px } => {
                        for x in 0..width as usize {
                            let sx = if width == w { x } else { (x * 2).min(w as usize - 1) };
                            let (r, g, b) = px_at(sx, y);
                            let v = match chan {
                                Chan::R => r,
                                Chan::G => g,
                                Chan::B => b,
                                Chan::Y => rgb_to_yuv(r, g, b).0,
                                Chan::Y2 => {
                                    let (r2, g2, b2) = px_at(sx, y2);
                                    rgb_to_yuv(r2, g2, b2).0
                                }
                                Chan::Cr | Chan::Cb => {
                                    let (r2, g2, b2) = px_at(sx, y2);
                                    let a = rgb_to_yuv(r, g, b);
                                    let c = rgb_to_yuv(r2, g2, b2);
                                    let (u, v2) =
                                        if chan == Chan::Cr { (a.1, c.1) } else { (a.2, c.2) };
                                    ((u as u16 + v2 as u16) / 2) as u8
                                }
                            };
                            push(&mut plan, value_to_hz(v), px);
                        }
                    }
                }
            }
        }

        let total: u64 = plan.iter().map(|&(_, n)| n as u64).sum();
        SstvTx { rate, plan, idx: 0, left: 0, cur_hz: 0.0, phase: 0.0, total, done: 0 }
    }

    /// Fill `out` with audio; returns the number of real samples written
    /// before the transmission ended (the rest of `out`, if any, is zeroed).
    pub fn next_block(&mut self, out: &mut [f32]) -> usize {
        let mut written = 0;
        for s in out.iter_mut() {
            if self.left == 0 {
                match self.plan.get(self.idx) {
                    Some(&(hz, n)) => {
                        self.cur_hz = hz;
                        self.left = n;
                        self.idx += 1;
                    }
                    None => {
                        *s = 0.0;
                        continue;
                    }
                }
            }
            self.phase += TAU * self.cur_hz / self.rate;
            if self.phase > TAU {
                self.phase -= TAU;
            }
            *s = (self.phase.sin() as f32) * 0.5;
            self.left -= 1;
            self.done += 1;
            written += 1;
        }
        written
    }

    /// True once every planned sample has been emitted.
    pub fn done(&self) -> bool {
        self.idx >= self.plan.len() && self.left == 0
    }

    /// Total number of audio samples this transmission will emit.
    pub fn total_samples(&self) -> u64 {
        self.total
    }

    /// Transmission progress, 0.0..=1.0.
    pub fn progress(&self) -> f32 {
        if self.total == 0 { 1.0 } else { (self.done as f32 / self.total as f32).clamp(0.0, 1.0) }
    }

    /// Append an FSK identification for `id` to the end of the transmission.
    /// Nothing is added for an empty (or unsendable) `id`.
    #[must_use]
    pub fn with_fsk_id(mut self, id: &str) -> Self {
        let symbols = fsk_id_symbols(id);
        if symbols.is_empty() {
            return self;
        }
        let mut emitted: i64 = 0;
        let mut t_exact: f64 = 0.0;
        let mut push = |plan: &mut Vec<(f64, u32)>, hz: f64, dur: f64| {
            t_exact += dur * self.rate;
            let target = t_exact.round() as i64;
            let n = (target - emitted).max(0);
            emitted = target;
            if n > 0 {
                plan.push((hz, n as u32));
            }
        };
        push(&mut self.plan, FSKID_LEADER_HZ, FSKID_LEADER_S);
        push(&mut self.plan, FSKID_ZERO_HZ, FSKID_SYNC_S);
        push(&mut self.plan, FSKID_ONE_HZ, FSKID_BIT_S);
        for sym in symbols {
            for bit in (0..6).rev() {
                let one = (sym >> bit) & 1 == 1;
                push(&mut self.plan, if one { FSKID_ONE_HZ } else { FSKID_ZERO_HZ }, FSKID_BIT_S);
            }
        }
        self.total = self.plan.iter().map(|&(_, n)| n as u64).sum();
        self
    }
}

// ─────────────────────────────── receive ───────────────────────────────

/// A decoded output from the receiver.
pub enum SstvEvent {
    /// A VIS header identified the mode; a new image is starting.
    ModeDetected(SstvMode),
    /// A finished scan line: `rgb` is `3 * width` bytes at row `y`.
    Line { y: u16, rgb: Vec<u8> },
    /// The current image reached its last line.
    ImageComplete,
    /// A station identified itself in tones after its picture.
    FskId(String),
    /// A header arrived, with good parity, for a mode this decoder does not
    /// have. `name` is the mode name where the code is an assigned one.
    UnsupportedMode { code: u8, name: Option<&'static str> },
}

#[derive(PartialEq)]
enum RxPhase {
    /// Hunting for the VIS leader / decoding VIS.
    Hunt,
    /// Decoding image lines for `mode`.
    Image,
}

/// SSTV receiver. Feed audio with [`SstvRx::process`]; it emits [`SstvEvent`]s.
pub struct SstvRx {
    rate: f64,
    // Down-mix + baseband filter for the discriminator.
    mix_ph: f32,
    mix_inc: f32,
    lpf: ComplexFir,
    prev: Complex32,
    // Instantaneous frequency (Hz), lightly smoothed.
    inst_hz: f64,
    // Smoothed raw-input level (mean |audio|) for the UI activity meter.
    in_level: f32,
    have_prev: bool,

    phase: RxPhase,
    mode: SstvMode,
    // Rolling ring of recent instantaneous-frequency samples, so we can look
    // back over a whole line once its trailing sync arrives.
    hist: Vec<f64>,
    // Samples of history to guarantee (~1.2 s); `hist` runs a little past it
    // between compactions. See `push_hist`.
    hist_cap: usize,
    // Absolute sample index of hist[0].
    hist_base: u64,
    sample_idx: u64,

    // VIS bit accumulation.
    vis_state: VisState,
    // The FSK ID hunt, which runs whenever no picture is being decoded.
    fsk_state: FskIdState,

    // Image decode bookkeeping.
    line: u16,
    // Sample index where the current line's decoding should start.
    line_start: u64,
    // Length of the current line, cached because `step_image` runs per sample.
    line_samples: u64,
    // Robot 4:2:0 chroma carried between lines.
    last_cr: Vec<u8>,
    last_cb: Vec<u8>,

    // Free-run (decode without VIS): lock onto a regular 1200 Hz sync cadence.
    // `expected` = a specific operator-selected mode, or `None` for auto
    // (match the cadence + sync length against every mode).
    expected: Option<SstvMode>,
    sync_run: u32,
    // Recent sync pulses as (centre sample, pulse length in samples).
    sync_hist: Vec<(u64, u32)>,

    // Local addition (not upstream): how clean the most recently decoded
    // line's sync pulse was, 0.0..=1.0 -- see realign_sync's own doc
    // comment and sync_quality's public getter for why this exists (a
    // tuning aid, the same idea as QSSTV's "Sync" meter on its receive
    // window: a strong, cleanly-centred sync pulse every line means the
    // operator is tuned correctly; a weak or absent one means to nudge the
    // VFO).
    last_sync_quality: f32,
}

/// The hunt for an FSK ID: what has been seen of one so far.
struct FskIdState {
    /// Consecutive samples of the 100 ms block that arms the hunt.
    arm_run: u32,
    /// Set once that block has run long enough to be one.
    armed: bool,
    /// Sample index of the transition out of it -- bit zero's leading edge,
    /// and the origin every bit below is timed from.
    start: Option<u64>,
    /// Data bits taken so far (the start bit is not among them).
    bits: Vec<bool>,
    /// How many of them have been sampled, so each is taken exactly once.
    taken: u32,
    /// Consecutive samples since arming that were on neither tone.
    gap: u32,
}

impl FskIdState {
    fn reset() -> Self {
        FskIdState { arm_run: 0, armed: false, start: None, bits: Vec::new(), taken: 0, gap: 0 }
    }
}

struct VisState {
    // Running count of consecutive ~1900 Hz leader samples.
    leader: u32,
    // A full (>150 ms) leader has been seen at least once.
    leader_seen: bool,
    // Previous sample was ~1200 Hz (rising-edge detection).
    was_sync: bool,
    // Candidate start-bit sample indices awaiting a decode attempt.
    cands: Vec<u64>,
}

impl VisState {
    fn reset() -> Self {
        VisState { leader: 0, leader_seen: false, was_sync: false, cands: Vec::new() }
    }
}

impl SstvRx {
    pub fn new(rate: f64) -> Self {
        let mix_hz = 1900.0f32;
        // Keep ~1.2 s of history (enough for the slowest line + sync search).
        let hist_cap = (rate * 1.2) as usize;
        SstvRx {
            rate,
            mix_ph: 0.0,
            mix_inc: (TAU as f32) * mix_hz / rate as f32,
            lpf: ComplexFir::new(bandpass_taps(129, -1100.0, 1100.0, rate)),
            prev: Complex32::new(0.0, 0.0),
            inst_hz: 1900.0,
            in_level: 0.0,
            have_prev: false,
            phase: RxPhase::Hunt,
            mode: SstvMode::Scottie1,
            hist: Vec::with_capacity(hist_cap + hist_cap / 4 + 1),
            hist_cap,
            hist_base: 0,
            sample_idx: 0,
            vis_state: VisState::reset(),
            fsk_state: FskIdState::reset(),
            line: 0,
            line_start: 0,
            line_samples: 0,
            last_cr: Vec::new(),
            last_cb: Vec::new(),
            expected: None,
            sync_run: 0,
            sync_hist: Vec::new(),
            last_sync_quality: 0.0,
        }
    }

    /// Set the mode used for free-run (no-VIS) decoding, or `None` for auto
    /// (detect the mode from the sync cadence).
    pub fn set_expected(&mut self, mode: Option<SstvMode>) {
        self.expected = mode;
    }

    /// Abandon whatever is being decoded and go back to hunting for a header.
    pub fn restart(&mut self) {
        self.phase = RxPhase::Hunt;
        self.vis_state = VisState::reset();
        self.fsk_state = FskIdState::reset();
        self.line = 0;
        self.line_start = 0;
        self.line_samples = 0;
        self.last_cr.clear();
        self.last_cb.clear();
        self.sync_run = 0;
        self.sync_hist.clear();
        self.hist.clear();
        self.hist_base = self.sample_idx;
    }

    /// The mode currently being decoded (or last detected).
    pub fn mode(&self) -> SstvMode {
        self.mode
    }

    /// Smoothed raw-input level (mean |sample|), for a UI activity meter so
    /// the operator can set their receive gain.
    pub fn level(&self) -> f32 {
        self.in_level
    }

    /// True while an image is being decoded (VIS locked).
    pub fn receiving(&self) -> bool {
        self.phase == RxPhase::Image
    }

    /// Local addition (not upstream): how clean the most recently decoded
    /// line's sync pulse was, 0.0..=1.0 -- a tuning aid, the same role
    /// QSSTV's own "Sync" meter plays on its receive window. Stays at the
    /// last real picture's value while hunting (there is no sync pulse to
    /// grade yet); 0.0 before the first line of a fresh picture has been
    /// decoded.
    pub fn sync_quality(&self) -> f32 {
        self.last_sync_quality
    }

    /// Fraction of the current image decoded, 0.0..=1.0.
    pub fn progress(&self) -> f32 {
        if self.phase != RxPhase::Image {
            return 0.0;
        }
        let (_, h) = self.mode.dimensions();
        (self.line as f32 / h.max(1) as f32).clamp(0.0, 1.0)
    }

    /// Feed audio; push any decoded events.
    pub fn process(&mut self, audio: &[f32], out: &mut Vec<SstvEvent>) {
        // Down-mix by 1900 Hz to complex baseband, then low-pass the whole block.
        let mut mixed = Vec::with_capacity(audio.len());
        for &a in audio {
            self.in_level += 0.001 * (a.abs() - self.in_level);
            let z = Complex32::new(a * self.mix_ph.cos(), -a * self.mix_ph.sin());
            self.mix_ph += self.mix_inc;
            if self.mix_ph > std::f32::consts::TAU {
                self.mix_ph -= std::f32::consts::TAU;
            }
            mixed.push(z);
        }
        let mut bb = Vec::with_capacity(audio.len());
        self.lpf.process(&mixed, &mut bb);

        for z in bb {
            // Instantaneous frequency via the discriminator.
            let raw_hz = if self.have_prev {
                let d = z * self.prev.conj();
                1900.0 + (d.arg() as f64) * self.rate / TAU
            } else {
                1900.0
            };
            self.prev = z;
            self.have_prev = true;
            self.inst_hz += 0.5 * (raw_hz - self.inst_hz);

            self.push_hist(self.inst_hz);
            self.sample_idx += 1;

            match self.phase {
                RxPhase::Hunt => self.step_hunt(out),
                RxPhase::Image => self.step_image(out),
            }
        }
    }

    fn push_hist(&mut self, hz: f64) {
        self.hist.push(hz);
        // Compact in blocks, never per sample -- see rtty.rs's own
        // equivalent buffers for the identical reasoning.
        if self.hist.len() > self.hist_cap + self.hist_cap / 4 {
            let drop = self.hist.len() - self.hist_cap;
            self.hist.drain(0..drop);
            self.hist_base += drop as u64;
        }
    }

    fn hz_at(&self, idx: u64) -> f64 {
        if idx < self.hist_base {
            return 1900.0;
        }
        let i = (idx - self.hist_base) as usize;
        self.hist.get(i).copied().unwrap_or(1900.0)
    }

    // ── FSK ID detection ──
    fn step_fsk_id(&mut self, out: &mut Vec<SstvEvent>) {
        let near = |a: f64, b: f64| (a - b).abs() < 80.0;
        let bit_samples = FSKID_BIT_S * self.rate;

        if self.fsk_state.start.is_none() {
            if near(self.inst_hz, FSKID_ZERO_HZ) {
                self.fsk_state.arm_run += 1;
                self.fsk_state.gap = 0;
                if self.fsk_state.arm_run as f64 > 0.066 * self.rate {
                    self.fsk_state.armed = true;
                }
                return;
            }
            if !self.fsk_state.armed {
                self.fsk_state.arm_run = 0;
                return;
            }
            if near(self.inst_hz, FSKID_ONE_HZ) {
                self.fsk_state.start = Some(self.sample_idx);
                self.fsk_state.bits.clear();
                self.fsk_state.taken = 0;
                return;
            }
            self.fsk_state.gap += 1;
            if self.fsk_state.gap as f64 > 3.0 * bit_samples {
                self.fsk_state = FskIdState::reset();
            }
            return;
        }

        let Some(start) = self.fsk_state.start else { return };
        let k = self.fsk_state.taken + 1;
        let at = start + ((k as f64 + 0.5) * bit_samples) as u64;
        if self.sample_idx <= at {
            return;
        }
        self.fsk_state.taken += 1;
        let hz = self.hz_at(at);
        if near(hz, FSKID_ONE_HZ) {
            self.fsk_state.bits.push(true);
        } else if near(hz, FSKID_ZERO_HZ) {
            self.fsk_state.bits.push(false);
        } else {
            self.fsk_state = FskIdState::reset();
            return;
        }

        if self.fsk_state.bits.len() % 6 != 0 {
            return;
        }
        let symbols: Vec<u8> = self
            .fsk_state
            .bits
            .chunks_exact(6)
            .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b)))
            .collect();
        if symbols[0] != FSKID_HEAD {
            self.fsk_state = FskIdState::reset();
            return;
        }
        if let Some(text) = fsk_id_text(&symbols) {
            out.push(SstvEvent::FskId(text));
            self.fsk_state = FskIdState::reset();
            return;
        }
        if symbols.len() > FSKID_MAX_CHARS + 3 {
            self.fsk_state = FskIdState::reset();
        }
    }

    // ── VIS detection ──
    fn step_hunt(&mut self, out: &mut Vec<SstvEvent>) {
        self.step_fsk_id(out);
        let near = |a: f64, b: f64| (a - b).abs() < 90.0;
        let is_leader = near(self.inst_hz, VIS_LEADER_HZ);
        let is_sync = near(self.inst_hz, SYNC_HZ);
        if is_leader {
            self.vis_state.leader = (self.vis_state.leader + 1).min((self.rate) as u32);
            if self.vis_state.leader as f64 > 0.12 * self.rate {
                self.vis_state.leader_seen = true;
            }
        } else {
            self.vis_state.leader = self.vis_state.leader.saturating_sub(3);
            if self.vis_state.leader == 0 {
                self.vis_state.leader_seen = false;
            }
        }
        // Rising edge into a 1200 Hz pulse after a leader -> candidate start bit.
        if is_sync && !self.vis_state.was_sync && self.vis_state.leader_seen {
            self.vis_state.cands.push(self.sample_idx);
            if self.vis_state.cands.len() > 8 {
                self.vis_state.cands.remove(0);
            }
        }
        self.vis_state.was_sync = is_sync;

        // Try the oldest candidate once its 8 VIS bits have elapsed.
        let bit = 0.030 * self.rate;
        if let Some(&start) = self.vis_state.cands.first() {
            if (self.sample_idx as f64) >= start as f64 + 9.5 * bit {
                self.vis_state.cands.remove(0);
                let mut code = 0u8;
                let mut parity = 0u8;
                let mut looks_like_vis = true;
                for b in 0..7 {
                    let centre = start as f64 + (1.5 + b as f64) * bit;
                    let hz = self.hz_at(centre as u64);
                    if hz > 1600.0 {
                        looks_like_vis = false;
                    }
                    if hz < 1200.0 {
                        code |= 1 << b; // 1100 Hz = 1
                        parity ^= 1;
                    }
                }
                let phz = self.hz_at((start as f64 + 8.5 * bit) as u64);
                if phz > 1600.0 {
                    looks_like_vis = false;
                }
                let pbit = if phz < 1200.0 { 1 } else { 0 };
                if looks_like_vis && parity == pbit {
                    match SstvMode::from_vis(code) {
                        Some(mode) => {
                            let mut first = start as f64 + 10.0 * bit;
                            if matches!(
                                mode,
                                SstvMode::Scottie1 | SstvMode::Scottie2 | SstvMode::ScottieDx
                            ) {
                                first += 0.009 * self.rate;
                            }
                            self.begin_image(mode, first as u64, out);
                        }
                        None if code != 0 && self.preceded_by_leader(start) => {
                            out.push(SstvEvent::UnsupportedMode {
                                code,
                                name: SstvMode::unsupported_name(code),
                            })
                        }
                        None => {}
                    }
                }
            }
        }

        // No VIS yet? Try to lock onto the sync cadence of the selected mode.
        self.try_freerun(out);
    }

    /// Whether the 20 ms before `start` really is the 1900 Hz leader.
    fn preceded_by_leader(&self, start: u64) -> bool {
        let mut near = 0u32;
        for k in 1..=5u64 {
            let back = (k as f64 * 0.004 * self.rate) as u64;
            let idx = start.saturating_sub(back);
            if (self.hz_at(idx) - VIS_LEADER_HZ).abs() < 120.0 {
                near += 1;
            }
        }
        near >= 3
    }

    /// Total samples per scan line for `mode`, at line index `line`.
    fn line_period_samples(&self, mode: SstvMode, line: u16) -> f64 {
        let (w, _) = mode.dimensions();
        line_segments(mode, w, line)
            .iter()
            .map(|s| match s {
                Seg::Tone { dur, .. } => *dur * self.rate,
                Seg::Scan { width, px, .. } => *width as f64 * *px * self.rate,
            })
            .sum()
    }

    /// Count how many recent sync gaps are an integer multiple of `mode`'s
    /// line period (within tolerance).
    fn cadence_hits(&self, mode: SstvMode) -> u32 {
        let period = self.line_period_samples(mode, 0);
        let mut hits = 0;
        for w in self.sync_hist.windows(2) {
            let gap = w[1].0 as f64 - w[0].0 as f64;
            let k = (gap / period).round();
            if k >= 1.0 && (gap - k * period).abs() < period * 0.04 {
                hits += 1;
            }
        }
        hits
    }

    /// Free-run lock: when 1200 Hz sync pulses arrive at a regular line
    /// cadence, start decoding (no VIS needed -- handles tuning into a
    /// picture already in progress).
    fn try_freerun(&mut self, out: &mut Vec<SstvEvent>) {
        let is_sync = self.inst_hz > 1050.0 && self.inst_hz < 1350.0;
        if is_sync {
            self.sync_run += 1;
            return;
        }
        let run = self.sync_run;
        self.sync_run = 0;
        if (run as f64) < 0.003 * self.rate || (run as f64) > 0.028 * self.rate {
            return;
        }
        let center = self.sample_idx.saturating_sub((run / 2) as u64);
        self.sync_hist.push((center, run));
        if self.sync_hist.len() > 10 {
            self.sync_hist.remove(0);
        }

        let locked = match self.expected {
            Some(m) => (self.cadence_hits(m) >= 2).then_some(m),
            None => {
                let mut best: Option<SstvMode> = None;
                let mut best_err = f64::INFINITY;
                for &m in &SstvMode::ALL {
                    if self.cadence_hits(m) < 2 {
                        continue;
                    }
                    let (_, sdur) = self.sync_span(m, m.dimensions().0, 0);
                    let dur_err = (run as f64 - sdur).abs() / sdur;
                    if dur_err > 0.4 {
                        continue; // sync length must also match (Scottie vs Martin)
                    }
                    if dur_err < best_err {
                        best_err = dur_err;
                        best = Some(m);
                    }
                }
                best
            }
        };
        if let Some(mode) = locked {
            let (soff, sdur) = self.sync_span(mode, mode.dimensions().0, 0);
            let line_start = (center as f64 - (soff + sdur * 0.5)).max(0.0) as u64;
            self.sync_hist.clear();
            self.begin_image(mode, line_start, out);
        }
    }

    fn begin_image(&mut self, mode: SstvMode, first_line_start: u64, out: &mut Vec<SstvEvent>) {
        self.mode = mode;
        self.phase = RxPhase::Image;
        self.line = 0;
        self.line_start = first_line_start;
        self.line_samples = self.line_period_samples(mode, 0) as u64;
        let (w, _) = mode.dimensions();
        self.last_cr = vec![128u8; (w / 2) as usize];
        self.last_cb = vec![128u8; (w / 2) as usize];
        self.sync_run = 0;
        self.sync_hist.clear();
        out.push(SstvEvent::ModeDetected(mode));
    }

    // ── image line decode ──
    fn step_image(&mut self, out: &mut Vec<SstvEvent>) {
        let line_samples = self.line_samples;
        if self.sample_idx < self.line_start + line_samples + (0.02 * self.rate) as u64 {
            return;
        }

        // Re-align each line to its 1200 Hz sync pulse (corrects timing error
        // and clock slant on real off-air signals).
        let (w, h) = self.mode.dimensions();
        let start = self.realign_sync(self.line_start, self.mode, w, self.line);
        for (n, rgb) in self.decode_line(self.mode, w, self.line, start).into_iter().enumerate() {
            let y = self.line + n as u16;
            if y < h {
                out.push(SstvEvent::Line { y, rgb });
            }
        }

        self.line += self.mode.rows_per_line().max(1);
        self.line_start = start + line_samples;
        if self.line >= h {
            out.push(SstvEvent::ImageComplete);
            self.phase = RxPhase::Hunt;
            self.vis_state = VisState::reset();
        } else {
            self.line_samples = self.line_period_samples(self.mode, self.line) as u64;
        }
    }

    /// Offset (in samples) from a line's start to the centre of its 1200 Hz
    /// sync pulse, plus the pulse duration in samples.
    fn sync_span(&self, mode: SstvMode, w: u16, line: u16) -> (f64, f64) {
        let mut t = 0.0;
        for seg in line_segments(mode, w, line) {
            match seg {
                Seg::Tone { hz, dur } => {
                    let d = dur * self.rate;
                    if (hz - SYNC_HZ).abs() < 1.0 {
                        return (t, d);
                    }
                    t += d;
                }
                Seg::Scan { width, px, .. } => t += width as f64 * px * self.rate,
            }
        }
        (0.0, 0.009 * self.rate)
    }

    /// Correct the line-start sample index by locking to the line's 1200 Hz
    /// sync pulse.
    ///
    /// Local addition (not upstream): also updates `last_sync_quality` with
    /// what fraction of the pulse's expected duration was actually seen
    /// below 1380 Hz in the search window -- 1.0 for a full, clean sync
    /// pulse right where expected, 0.0 for none at all. Piggybacks on work
    /// this function already does (the same `cnt`/`sdur` the realignment
    /// decision itself is based on) rather than a second pass over `hist`.
    fn realign_sync(&mut self, nominal: u64, mode: SstvMode, w: u16, line: u16) -> u64 {
        let (soff, sdur) = self.sync_span(mode, w, line);
        let centre_off = soff + sdur * 0.5;
        let expected = nominal as f64 + centre_off;
        let win = (0.012 * self.rate) as i64;
        let mut sum = 0.0f64;
        let mut cnt = 0u32;
        for d in -win..=win {
            let idx = expected as i64 + d;
            if idx < 0 {
                continue;
            }
            if self.hz_at(idx as u64) < 1380.0 {
                sum += idx as f64;
                cnt += 1;
            }
        }
        self.last_sync_quality = (cnt as f64 / sdur.max(1.0)).clamp(0.0, 1.0) as f32;
        if (cnt as f64) > sdur * 0.4 {
            let centre = sum / cnt as f64;
            (centre - centre_off).max(0.0) as u64
        } else {
            nominal
        }
    }

    /// Decode one transmitted line into its picture rows: one for every mode
    /// but the PD family, two for that.
    fn decode_line(&mut self, mode: SstvMode, w: u16, line: u16, start: u64) -> Vec<Vec<u8>> {
        let mut r = vec![0u8; w as usize];
        let mut g = vec![0u8; w as usize];
        let mut b = vec![0u8; w as usize];
        let mut y = vec![0u8; w as usize];
        let mut y2 = vec![0u8; w as usize];
        let mut cr = self.last_cr.clone();
        let mut cb = self.last_cb.clone();
        if mode.rows_per_line() > 1 {
            cr = vec![128u8; w as usize];
            cb = vec![128u8; w as usize];
        }

        let mut t = start as f64;
        for seg in line_segments(mode, w, line) {
            match seg {
                Seg::Tone { dur, .. } => t += dur * self.rate,
                Seg::Scan { chan, width, px } => {
                    let step = px * self.rate;
                    for x in 0..width as usize {
                        let idx = (t + (x as f64 + 0.5) * step) as u64;
                        let v = hz_to_value(self.hz_at(idx));
                        let cri = x.min(cr.len().saturating_sub(1));
                        let cbi = x.min(cb.len().saturating_sub(1));
                        match chan {
                            Chan::R => r[x] = v,
                            Chan::G => g[x] = v,
                            Chan::B => b[x] = v,
                            Chan::Y => y[x] = v,
                            Chan::Y2 => y2[x] = v,
                            Chan::Cr => cr[cri] = v,
                            Chan::Cb => cb[cbi] = v,
                        }
                    }
                    t += width as f64 * step;
                }
            }
        }

        let robot = matches!(mode, SstvMode::Robot72 | SstvMode::Robot36);
        if robot {
            self.last_cr = cr.clone();
            self.last_cb = cb.clone();
        }
        let pd = mode.rows_per_line() > 1;

        let mut rows: Vec<Vec<u8>> = Vec::with_capacity(if pd { 2 } else { 1 });
        for luma in [&y, &y2].into_iter().take(if pd { 2 } else { 1 }) {
            let mut rgb = vec![0u8; w as usize * 3];
            for x in 0..w as usize {
                let (rr, gg, bb) = if robot {
                    let cx = (x / 2).min(cr.len() - 1);
                    yuv_to_rgb(y[x], cr[cx], cb[cx])
                } else if pd {
                    yuv_to_rgb(luma[x], cr[x], cb[x])
                } else {
                    (r[x], g[x], b[x])
                };
                rgb[x * 3] = rr;
                rgb[x * 3 + 1] = gg;
                rgb[x * 3 + 2] = bb;
            }
            rows.push(rgb);
        }
        rows
    }
}

mod fir {
    use std::f64::consts::{PI, TAU};

    use super::Complex32;

    fn sinc(x: f64) -> f64 {
        if x.abs() < 1e-12 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        }
    }

    fn blackman_harris_f64(n: usize, i: usize) -> f64 {
        const A: [f64; 4] = [0.35875, 0.48829, 0.14128, 0.01168];
        let x = TAU * i as f64 / (n as f64 - 1.0);
        A[0] - A[1] * x.cos() + A[2] * (2.0 * x).cos() - A[3] * (3.0 * x).cos()
    }

    /// Windowed-sinc lowpass, DC gain 1. `cutoff` is normalized to the input
    /// sample rate (0.5 = Nyquist).
    fn lowpass_taps(ntaps: usize, cutoff: f64) -> Vec<f32> {
        let center = (ntaps - 1) as f64 / 2.0;
        let mut taps: Vec<f64> = (0..ntaps)
            .map(|i| {
                2.0 * cutoff
                    * sinc(2.0 * cutoff * (i as f64 - center))
                    * blackman_harris_f64(ntaps, i)
            })
            .collect();
        let sum: f64 = taps.iter().sum();
        taps.iter_mut().for_each(|t| *t /= sum);
        taps.into_iter().map(|t| t as f32).collect()
    }

    /// Complex band-pass taps with passband [lo_hz, hi_hz] relative to DC
    /// (either or both edges may be negative -- that selects the sideband).
    pub fn bandpass_taps(
        ntaps: usize,
        lo_hz: f64,
        hi_hz: f64,
        sample_rate: f64,
    ) -> Vec<Complex32> {
        let bw = (hi_hz - lo_hz).abs().max(50.0);
        let center = (lo_hz + hi_hz) / 2.0;
        let lp = lowpass_taps(ntaps, (bw / 2.0) / sample_rate);
        let mid = (ntaps - 1) as f64 / 2.0;
        lp.iter()
            .enumerate()
            .map(|(i, &t)| {
                let ph = -TAU * center * (i as f64 - mid) / sample_rate;
                Complex32::new((ph.cos() * t as f64) as f32, (ph.sin() * t as f64) as f32)
            })
            .collect()
    }

    /// Independent running sums, so the dot product isn't one long
    /// dependency chain.
    const FIR_ACCUMULATORS: usize = 8;

    fn complex_fir(dst: &mut [Complex32], buf: &[Complex32], taps: &[Complex32]) {
        let n = taps.len();
        for (o, d) in dst.iter_mut().enumerate() {
            let w = &buf[o..o + n];
            let mut acc = [Complex32::default(); FIR_ACCUMULATORS];
            let mut ws = w.chunks_exact(FIR_ACCUMULATORS);
            let mut ts = taps.chunks_exact(FIR_ACCUMULATORS);
            for (xs, tt) in (&mut ws).zip(&mut ts) {
                for i in 0..FIR_ACCUMULATORS {
                    acc[i] += xs[i] * tt[i];
                }
            }
            let mut sum = Complex32::default();
            for a in acc {
                sum += a;
            }
            for (x, &t) in ws.remainder().iter().zip(ts.remainder()) {
                sum += x * t;
            }
            *d = sum;
        }
    }

    /// Streaming complex FIR (complex in, complex out).
    pub struct ComplexFir {
        taps: Vec<Complex32>,
        buf: Vec<Complex32>,
    }

    impl ComplexFir {
        pub fn new(taps: Vec<Complex32>) -> Self {
            ComplexFir { taps, buf: Vec::new() }
        }

        pub fn process(&mut self, input: &[Complex32], out: &mut Vec<Complex32>) {
            self.buf.extend_from_slice(input);
            let n = self.taps.len();
            if self.buf.len() < n {
                return;
            }
            let count = self.buf.len() - n + 1;
            let start = out.len();
            out.resize(start + count, Complex32::default());
            complex_fir(&mut out[start..], &self.buf, &self.taps);
            self.buf.drain(..count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_symbols_are_the_published_ones() {
        assert_eq!(
            fsk_id_symbols("OE1XYZ"),
            vec![0x2A, 0x2F, 0x25, 0x11, 0x38, 0x39, 0x3A, 0x01, 0x2F ^ 0x25 ^ 0x11 ^ 0x38 ^ 0x39 ^ 0x3A]
        );
        assert_eq!(fsk_id_symbols("oe1xyz"), fsk_id_symbols("OE1XYZ"));
        assert_eq!(fsk_id_symbols("OE1XYZ\u{00fc}"), fsk_id_symbols("OE1XYZ"));
        assert!(fsk_id_symbols("").is_empty());
        assert!(fsk_id_symbols("   ").is_empty());
    }

    #[test]
    fn a_stream_that_does_not_check_out_is_not_a_callsign() {
        let good = fsk_id_symbols("OE1XYZ");
        assert_eq!(fsk_id_text(&good).as_deref(), Some("OE1XYZ"));
        let mut bad = good.clone();
        bad[3] ^= 1;
        assert_eq!(fsk_id_text(&bad), None);
        assert_eq!(fsk_id_text(&good[1..]), None);
        assert_eq!(fsk_id_text(&good[..good.len() - 2]), None);
        assert_eq!(fsk_id_text(&good[..good.len() - 1]), None);
    }

    #[test]
    fn the_id_comes_back_off_the_air() {
        let rate = 48_000.0;
        let mode = SstvMode::Robot36;
        let (w, h) = mode.dimensions();
        let rgb = vec![128u8; w as usize * h as usize * 3];
        let mut tx = SstvTx::new(mode, &rgb, w, h, rate, 0.0).with_fsk_id("OE1XYZ");
        let mut rx = SstvRx::new(rate);
        let mut events = Vec::new();
        let mut block = vec![0.0f32; 4096];
        let mut heard = None;
        let mut guard = 0;
        while !tx.done() && guard < 40_000 {
            let n = tx.next_block(&mut block);
            rx.process(&block[..n], &mut events);
            for e in events.drain(..) {
                if let SstvEvent::FskId(id) = e {
                    heard = Some(id);
                }
            }
            guard += 1;
        }
        assert_eq!(heard.as_deref(), Some("OE1XYZ"));
    }

    #[test]
    fn a_restart_lets_go_of_the_picture_and_hunts_again() {
        let rate = 48_000.0;
        let (w, h) = SstvMode::ScottieDx.dimensions();
        let rgb = vec![96u8; w as usize * h as usize * 3];
        let mut tx = SstvTx::new(SstvMode::ScottieDx, &rgb, w, h, rate, 0.0);
        let mut rx = SstvRx::new(rate);
        let mut events = Vec::new();
        let mut block = vec![0.0f32; 4096];

        while !rx.receiving() || rx.progress() < 0.02 {
            let n = tx.next_block(&mut block);
            assert!(n > 0, "the transmission ran out before the picture started");
            rx.process(&block[..n], &mut events);
            events.clear();
        }
        assert_eq!(rx.mode(), SstvMode::ScottieDx);

        rx.restart();
        assert!(!rx.receiving(), "the picture was not let go of");
        assert_eq!(rx.progress(), 0.0);

        for _ in 0..12 {
            let n = tx.next_block(&mut block);
            rx.process(&block[..n], &mut events);
            events.clear();
        }
        assert!(!rx.receiving(), "it re-locked on the transmission it was told to abandon");

        let (w2, h2) = SstvMode::Robot36.dimensions();
        let rgb2 = vec![32u8; w2 as usize * h2 as usize * 3];
        let mut next = SstvTx::new(SstvMode::Robot36, &rgb2, w2, h2, rate, 0.0);
        let mut detected = None;
        let mut guard = 0;
        while detected.is_none() && !next.done() && guard < 2_000 {
            let n = next.next_block(&mut block);
            rx.process(&block[..n], &mut events);
            for e in events.drain(..) {
                if let SstvEvent::ModeDetected(m) = e {
                    detected = Some(m);
                }
            }
            guard += 1;
        }
        assert_eq!(detected, Some(SstvMode::Robot36), "the restarted receiver heard nothing");
    }

    #[test]
    fn scottie1_loopback_recovers_mode() {
        let rate = 48_000.0;
        let mode = SstvMode::Scottie1;
        let (w, h) = mode.dimensions();
        let mut rgb = vec![0u8; w as usize * h as usize * 3];
        for yy in 0..h as usize {
            for xx in 0..w as usize {
                let i = (yy * w as usize + xx) * 3;
                let v = (xx * 255 / w as usize) as u8;
                rgb[i] = v;
                rgb[i + 1] = v;
                rgb[i + 2] = v;
            }
        }
        let mut tx = SstvTx::new(mode, &rgb, w, h, rate, 0.0);
        let mut rx = SstvRx::new(rate);
        let mut events = Vec::new();
        let mut block = vec![0.0f32; 4096];
        let mut detected = None;
        let mut lines = 0;
        let mut guard = 0;
        while !tx.done() && guard < 20_000 {
            let n = tx.next_block(&mut block);
            rx.process(&block[..n], &mut events);
            for e in events.drain(..) {
                match e {
                    SstvEvent::ModeDetected(m) => detected = Some(m),
                    SstvEvent::Line { .. } => lines += 1,
                    SstvEvent::ImageComplete | SstvEvent::FskId(_) | SstvEvent::UnsupportedMode { .. } => {}
                }
            }
            guard += 1;
        }
        rx.process(&[0.0; 48_000], &mut events);
        for e in events.drain(..) {
            if let SstvEvent::ModeDetected(m) = e {
                detected = Some(m);
            } else if let SstvEvent::Line { .. } = e {
                lines += 1;
            }
        }
        assert_eq!(detected, Some(mode), "VIS mode should be recovered");
        assert!(lines > (h as usize) / 2, "should decode most lines, got {lines}");
    }

    #[test]
    fn every_mode_round_trips_through_its_own_decoder() {
        let rate = 48_000.0;
        for mode in SstvMode::ALL {
            let (w, h) = mode.dimensions();
            let mut rgb = vec![0u8; w as usize * h as usize * 3];
            for yy in 0..h as usize {
                for xx in 0..w as usize {
                    let i = (yy * w as usize + xx) * 3;
                    let band = xx * 3 / w as usize;
                    rgb[i + band.min(2)] = 220;
                }
            }
            let mut tx = SstvTx::new(mode, &rgb, w, h, rate, 0.0);
            let mut rx = SstvRx::new(rate);
            let mut events = Vec::new();
            let mut block = vec![0.0f32; 8192];
            let mut detected = None;
            let mut got = vec![0u8; w as usize * h as usize * 3];
            let mut lines = 0usize;
            let mut complete = false;
            let mut guard = 0;
            while !tx.done() && guard < 400_000 {
                let n = tx.next_block(&mut block);
                rx.process(&block[..n], &mut events);
                for e in events.drain(..) {
                    match e {
                        SstvEvent::ModeDetected(m) => detected = Some(m),
                        SstvEvent::Line { y, rgb: row } => {
                            lines += 1;
                            let at = y as usize * w as usize * 3;
                            if at + row.len() <= got.len() {
                                got[at..at + row.len()].copy_from_slice(&row);
                            }
                        }
                        SstvEvent::ImageComplete => complete = true,
                        _ => {}
                    }
                }
                guard += 1;
            }
            rx.process(&vec![0.0f32; 48_000], &mut events);
            for e in events.drain(..) {
                match e {
                    SstvEvent::Line { y, rgb: row } => {
                        lines += 1;
                        let at = y as usize * w as usize * 3;
                        if at + row.len() <= got.len() {
                            got[at..at + row.len()].copy_from_slice(&row);
                        }
                    }
                    SstvEvent::ImageComplete => complete = true,
                    _ => {}
                }
            }
            assert_eq!(detected, Some(mode), "{} VIS not recovered", mode.label());
            assert_eq!(lines, h as usize, "{}: decoded {lines} of {h} rows", mode.label());
            assert!(complete, "{}: ImageComplete never fired", mode.label());
            let yy = h as usize / 3;
            for (band, chan) in [(0usize, 0usize), (1, 1), (2, 2)] {
                let xx = (band * 2 + 1) * w as usize / 6;
                let i = (yy * w as usize + xx) * 3;
                let px = [got[i], got[i + 1], got[i + 2]];
                let others = (0..3).filter(|&c| c != chan).map(|c| px[c]).max().unwrap();
                assert!(
                    px[chan] > 100 && px[chan] as i32 > others as i32 + 40,
                    "{}: bar {band} decoded as {px:?}, expected channel {chan} to dominate",
                    mode.label()
                );
            }
        }
    }

    #[test]
    fn the_line_times_are_the_published_ones() {
        let rx = SstvRx::new(1_000_000.0); // us per sample: read the plan directly
        for (mode, ms) in [
            (SstvMode::Scottie1, 428.22),
            (SstvMode::Scottie2, 277.692),
            (SstvMode::ScottieDx, 1_050.3),
            (SstvMode::Martin1, 446.446),
            (SstvMode::Martin2, 226.798),
            (SstvMode::Robot72, 300.0),
            (SstvMode::Robot36, 150.0),
            (SstvMode::WraaseSc2_180, 711.0225),
            (SstvMode::WraaseSc2_120, 475.53),
            (SstvMode::Pd50, 388.16),
            (SstvMode::Pd90, 703.04),
            (SstvMode::Pd120, 508.48),
            (SstvMode::Pd160, 804.416),
            (SstvMode::Pd180, 754.24),
            (SstvMode::Pd240, 1_000.0),
            (SstvMode::Pd290, 937.28),
        ] {
            let got = rx.line_period_samples(mode, 0) / 1000.0;
            assert!((got - ms).abs() < 0.05, "{}: line is {got:.4} ms, published {ms} ms", mode.label());
        }
    }

    #[test]
    fn only_the_pd_modes_carry_two_rows_a_line() {
        for m in SstvMode::ALL {
            let want = matches!(
                m,
                SstvMode::Pd50
                    | SstvMode::Pd90
                    | SstvMode::Pd120
                    | SstvMode::Pd160
                    | SstvMode::Pd180
                    | SstvMode::Pd240
                    | SstvMode::Pd290
            );
            assert_eq!(m.rows_per_line() == 2, want, "{}", m.label());
        }
    }

    #[test]
    fn an_unimplemented_mode_says_so_instead_of_going_quiet() {
        let rate = 48_000.0;
        let code = 0x71u8;
        let mut audio: Vec<f32> = Vec::new();
        let mut phase = 0.0f64;
        let mut tone = |hz: f64, dur: f64, audio: &mut Vec<f32>| {
            for _ in 0..(dur * rate) as usize {
                phase += TAU * hz / rate;
                audio.push((phase.sin() as f32) * 0.5);
            }
        };
        tone(1900.0, 0.300, &mut audio);
        tone(1200.0, 0.010, &mut audio);
        tone(1900.0, 0.300, &mut audio);
        tone(1200.0, 0.030, &mut audio);
        let mut parity = 0u8;
        for bit in 0..7 {
            let one = (code >> bit) & 1 == 1;
            parity ^= one as u8;
            tone(if one { 1100.0 } else { 1300.0 }, 0.030, &mut audio);
        }
        tone(if parity == 1 { 1100.0 } else { 1300.0 }, 0.030, &mut audio);
        tone(1200.0, 0.030, &mut audio);
        tone(1500.0, 0.500, &mut audio);

        let mut rx = SstvRx::new(rate);
        let mut events = Vec::new();
        for chunk in audio.chunks(4096) {
            rx.process(chunk, &mut events);
        }
        let reported: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SstvEvent::UnsupportedMode { code, name } => Some((*code, *name)),
                _ => None,
            })
            .collect();
        assert_eq!(reported, vec![(0x71, Some("Pasokon P3"))]);
        assert!(
            !events.iter().any(|e| matches!(e, SstvEvent::ModeDetected(_))),
            "nothing may be decoded for a mode we do not have"
        );
    }

    #[test]
    fn vis_codes_roundtrip() {
        for m in SstvMode::ALL {
            assert_eq!(SstvMode::from_vis(m.vis_code()), Some(m));
        }
    }

    #[test]
    fn tx_ppm_scales_duration() {
        let rate = 48_000.0;
        let mode = SstvMode::Martin1;
        let (w, h) = mode.dimensions();
        let rgb = vec![128u8; w as usize * h as usize * 3];
        let base = SstvTx::new(mode, &rgb, w, h, rate, 0.0).total_samples() as f64;
        let trimmed = SstvTx::new(mode, &rgb, w, h, rate, 10_000.0).total_samples() as f64;
        assert!((trimmed / base - 1.01).abs() < 0.0005, "ratio {}", trimmed / base);
    }

    #[test]
    fn freerun_decodes_without_vis() {
        let rate = 48_000.0;
        let mode = SstvMode::Scottie1;
        let (w, h) = mode.dimensions();
        let mut rgb = vec![0u8; w as usize * h as usize * 3];
        for yy in 0..h as usize {
            for xx in 0..w as usize {
                let i = (yy * w as usize + xx) * 3;
                rgb[i] = (xx * 255 / w as usize) as u8;
            }
        }
        let mut tx = SstvTx::new(mode, &rgb, w, h, rate, 0.0);
        let mut audio = Vec::new();
        let mut block = vec![0.0f32; 4096];
        let mut guard = 0;
        while !tx.done() && guard < 20_000 {
            let n = tx.next_block(&mut block);
            audio.extend_from_slice(&block[..n]);
            guard += 1;
        }
        let skip = (rate * 1.1) as usize;
        let mut rx = SstvRx::new(rate);
        rx.set_expected(None);
        let mut events = Vec::new();
        let mut detected = None;
        let mut lines = 0;
        for chunk in audio[skip.min(audio.len())..].chunks(4096) {
            rx.process(chunk, &mut events);
            for e in events.drain(..) {
                match e {
                    SstvEvent::ModeDetected(m) => detected = Some(m),
                    SstvEvent::Line { .. } => lines += 1,
                    SstvEvent::ImageComplete | SstvEvent::FskId(_) | SstvEvent::UnsupportedMode { .. } => {}
                }
            }
        }
        assert_eq!(detected, Some(mode), "free-run should lock the selected mode");
        assert!(lines > 40, "free-run should decode many lines, got {lines}");
    }
}
