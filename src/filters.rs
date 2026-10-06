//! deskHPSDR's filter tables (filter.c): per mode 17 entries: 15 fixed filters (empty title = unused) and Var1 / Var2.
//! The edges are audio-domain edges as deskHPSDR keeps them: LSB/DIGL negative, USB/DIGU positive, CW/AM/DSB/SAM/SPEC/DRM
//! symmetric around zero (CW: around the pitch).

use crate::spectrum::Mode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterEntry {
    pub low: i32,
    pub high: i32,
    pub title: &'static str,
}

const fn e(low: i32, high: i32, title: &'static str) -> FilterEntry {
    FilterEntry { low, high, title }
}

const NONE: FilterEntry = e(0, 0, "");

const LSB: [FilterEntry; 15] = [
    e(-5150, -150, "5.0k"),
    e(-4550, -150, "4.4k"),
    e(-3950, -150, "3.8k"),
    e(-3450, -150, "3.3k"),
    e(-3050, -150, "2.9k"),
    e(-2850, -150, "2.7k"),
    e(-2550, -150, "2.4k"),
    e(-2250, -150, "2.1k"),
    e(-1950, -150, "1.8k"),
    e(-1150, -150, "1.0k"),
    e(-2950, -50, "2.9k/ESSB"),
    e(-3450, -50, "3.4k/ESSB"),
    e(-3950, -50, "3.9k/ESSB"),
    e(-5150, -50, "5.1k/ESSB"),
    e(-5950, -50, "5.9k/ESSB"),
];
const USB: [FilterEntry; 15] = [
    e(150, 5150, "5.0k"),
    e(150, 4550, "4.4k"),
    e(150, 3950, "3.8k"),
    e(150, 3450, "3.3k"),
    e(150, 3050, "2.9k"),
    e(150, 2850, "2.7k"),
    e(150, 2550, "2.4k"),
    e(150, 2250, "2.1k"),
    e(150, 1950, "1.8k"),
    e(150, 1150, "1.0k"),
    e(50, 2950, "2.9k/ESSB"),
    e(50, 3450, "3.4k/ESSB"),
    e(50, 3950, "3.9k/ESSB"),
    e(50, 5150, "5.1k/ESSB"),
    e(50, 5950, "5.9k/ESSB"),
];
const DIGL: [FilterEntry; 15] = [
    e(-5000, 0, "5.0k"),
    e(-4000, 0, "4.0k"),
    e(-3000, 0, "3.0k"),
    e(-2750, -250, "2.5k"),
    e(-2500, -500, "2.0k"),
    e(-2250, -750, "1.5k"),
    e(-2000, -1000, "1.0k"),
    e(-1875, -1125, "750"),
    e(-1750, -1250, "500"),
    e(-1625, -1375, "250"),
    e(-2300, -700, "FreeDV/RADEV1"),
    e(-2500, -1900, "RTTY-W"),
    e(-2420, -2000, "RTTY-N"),
    e(-2370, -2050, "RTTY-S"),
    NONE,
];
const DIGU: [FilterEntry; 15] = [
    e(0, 5000, "5.0k"),
    e(0, 4000, "4.0k"),
    e(0, 3000, "3.0k"),
    e(250, 2750, "2.5k"),
    e(500, 2500, "2.0k"),
    e(750, 2250, "1.5k"),
    e(1000, 2000, "1.0k"),
    e(1125, 1875, "750"),
    e(1250, 1750, "500"),
    e(1375, 1625, "250"),
    e(700, 2300, "FreeDV/RADEV1"),
    e(1200, 1850, "RTTY-W"),
    e(1300, 1800, "RTTY-N"),
    e(1400, 1750, "RTTY-S"),
    NONE,
];
const CW: [FilterEntry; 15] = [
    e(-500, 500, "1.0k"),
    e(-400, 400, "800"),
    e(-375, 375, "750"),
    e(-300, 300, "600"),
    e(-250, 250, "500"),
    e(-200, 200, "400"),
    e(-125, 125, "250"),
    e(-50, 50, "100"),
    e(-25, 25, "50"),
    e(-13, 13, "25"),
    NONE,
    NONE,
    NONE,
    NONE,
    NONE,
];
const AMLIKE: [FilterEntry; 15] = [
    e(-8000, 8000, "16k"),
    e(-6000, 6000, "12k"),
    e(-5000, 5000, "10k"),
    e(-4000, 4000, "8k"),
    e(-3300, 3300, "6.6k"),
    e(-2600, 2600, "5.2k"),
    e(-2000, 2000, "4.0k"),
    e(-1550, 1550, "3.1k"),
    e(-1450, 1450, "2.9k"),
    e(-1200, 1200, "2.4k"),
    NONE,
    NONE,
    NONE,
    NONE,
    NONE,
];

/// The 15 fixed filters of a mode (empty title = not shown).
pub fn fixed_filters(mode: Mode) -> &'static [FilterEntry; 15] {
    match mode {
        Mode::Lsb => &LSB,
        Mode::Usb => &USB,
        Mode::Digl => &DIGL,
        Mode::Digu => &DIGU,
        Mode::Cwl | Mode::Cwu => &CW,
        _ => &AMLIKE,
    }
}

/// Default (low, high) of Var1 and Var2 for a mode (deskHPSDR `varN_default_low/high`).
pub fn var_defaults(mode: Mode) -> [(i32, i32); 2] {
    match mode {
        Mode::Lsb => [(-2850, -150), (-2850, -150)],
        Mode::Usb => [(150, 2850), (150, 2850)],
        Mode::Digl => [(-3000, 0), (-2000, -1000)],
        Mode::Digu => [(0, 3000), (1000, 2000)],
        Mode::Cwl | Mode::Cwu => [(-125, 125), (-250, 250)],
        _ => [(-3300, 3300), (-3300, 3300)],
    }
}

/// The passband (Hz relative to the dial frequency) the DSP needs for audio-domain edges `low`..`high` in `mode`.
pub fn passband_for_edges(mode: Mode, low: i32, high: i32, cw_pitch_hz: f64) -> (f64, f64) {
    let (l, h) = (low as f64, high as f64);
    match mode {
        Mode::Cwu => (cw_pitch_hz + l, cw_pitch_hz + h),
        Mode::Cwl => (-(cw_pitch_hz + h), -(cw_pitch_hz + l)),
        _ => (l, h),
    }
}

/// True for the modes whose Var edges are edited as width / shift (deskHPSDR: CW, DSB, AM, SAM, SPEC, DRM).
pub fn width_shift_mode(mode: Mode) -> bool {
    matches!(mode, Mode::Cwl | Mode::Cwu | Mode::Dsb | Mode::Am | Mode::Sam | Mode::Spec | Mode::Drm)
}
