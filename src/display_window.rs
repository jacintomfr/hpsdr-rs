//! The "Display" window (deskHPSDR display_menu.c): full-screen, three pages chosen with round radio buttons (General
//! Settings, Peak Blobs & Hold, Peak Labels). The General page edits the same `ConnectedState` fields the old
//! Settings -> Spectrum tab edits, plus the detector / averaging / noise-floor margin / "Display Panadapter" options added
//! with this window. Also holds deskHPSDR's Panadapter / Waterfall Automatic algorithms (`AutoState`).
//! See docs/display-menu.md.

use crate::noise_window::{choice_combo_h, touch_radio};
use crate::spectrum::DisplayAvg;
use crate::tx_window::{child, new_row};
use crate::{chip_button, help_button, kiosk_accent_button, spin_buttons_full, std_checkbox, ConnectedState, Palette, ALL_PALETTES};
use std::time::{Duration, Instant};

/// Row pitch: 34 px controls + 10 px gap.
const GRID_H: f32 = 44.0;
/// Left column x range and right column x range inside the 966 px wide content.
const L0: f32 = 0.0;
const L1: f32 = 470.0;
const R0: f32 = 490.0;
const R1: f32 = 966.0;
/// Spin cell: "-" value "+" = 40 + 70 + 40 + 2 * 6 px of spacing + 6 = 168.
const SPIN_W: f32 = 168.0;

const NOT_IMPLEMENTED: &str = "Not implemented yet; the control is shown for parity with deskHPSDR and does nothing.";
const PEAKS_TX_HELP: &str = "Enable Peaks & Hold for the TX panadapter. Usable only as Peaks decay (fast attack, slow release at the Drop rate), Peaks hold is not available with TX. NOT usable if Duplex TX mode is active.";
const PAN_AUTO_HELP: &str = "deskHPSDR's algorithm: once a second the 60th percentile of the visible spectrum + 3 dB is measured and smoothed; every 5 s the panadapter Low is set to that noise floor rounded down to 10 dB, plus the Noisefloor Margin, minus 5 dB (limits -220..-95). Low only moves if it differs by more than 10 dB or is below the new value. When Automatic is switched on, a High of -50 or lower is set to -50 once (deskHPSDR does it every time; here your High then stays). Not applied while transmitting.";
const WF_AUTO_HELP: &str = "deskHPSDR's algorithm: Low = mean level of the waterfall row - 5 dB, High = Low + 55 dB, updated with every new row. Your own Waterfall High/Low stay as they are and are used again when Automatic is switched off.";
const FPS_HELP: &str = "Paces how often the screen is redrawn (receive and transmit). The analyzer itself runs at a fixed 10 frames per second.";
const SMOOTH_HELP: &str = "Off (default, like deskHPSDR): the panadapter trace is the raw per-pixel row joined by straight segments. On: the row is smoothed across neighbouring pixels and drawn as a spline.";
const AVG_HELP: &str = "Detector and averaging of the panadapter trace (WDSP). Av. Time sets the length of the averaging.";

// ======================================================================= automatic levels (deskHPSDR)

/// Runtime state of Panadapter Automatic (rx_panadapter.c, noise floor measurement + autoscale) and Waterfall Automatic
/// (waterfall.c).
#[derive(Default)]
pub struct AutoState {
    /// Smoothed noise floor (60th percentile + 3 dB, EMA alpha 0.25), None until measured.
    smoothed: Option<f64>,
    last_measure: Option<Instant>,
    last_calc: Option<Instant>,
    /// True once the first calculation after Automatic was switched on has run.
    calculated: bool,
    was_pan_auto: bool,
    /// False until the first tick: the state found at start-up (Automatic already on from the saved config) is not a switch-on.
    started: bool,
    /// The "High -50" rule of Panadapter Automatic is applied only once, at the first calculation after Automatic was
    /// switched on (deliberate deviation from deskHPSDR, which forces it on every calculation).
    high_rule_pending: bool,
    last_wf_rev: Option<u64>,
}

/// deskHPSDR autoscale_panadapter_with_offset(): the noise floor rounded down to the next 10 dB (C integer arithmetic)
/// plus the margin, limited to -220..-95.
pub(crate) fn autoscale_low(noise: f64, margin: i32) -> i32 {
    let n = noise as i32;
    let value = (n / 10 - if n % 10 != 0 { 1 } else { 0 }) * 10 + margin;
    value.clamp(-220, -95)
}

impl AutoState {
    /// deskHPSDR rx_panadapter_force_noisefloor_update(): forget the noise-floor state so the next tick measures and
    /// calculates at once (used when the Noisefloor Margin changes).
    pub fn force(&mut self) {
        self.smoothed = None;
        self.last_measure = None;
        self.last_calc = None;
        self.calculated = false;
    }

    /// `row` is the visible spectrum with the display correction already added (deskHPSDR: samples + soffset), `wf_row`
    /// the newest raw waterfall row and `corr` the same display correction (the waterfall levels are stored in the
    /// corrected domain, see main.rs wf_correction). `lv` = (panadapter low, high, waterfall low, high).
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        row: &[f32],
        wf_row: Option<&[f32]>,
        wf_rev: u64,
        corr: f32,
        pan_auto: bool,
        wf_auto: bool,
        margin: i32,
        lv: (&mut f32, &mut f32, &mut f32, &mut f32),
    ) {
        let (lo, hi, wlo, whi) = lv;
        if !self.started {
            // At start-up an already-on Automatic must not count as "switched on": the High rule (-50 once) would otherwise
            // reset a saved High like -60 at every launch.
            self.started = true;
            self.was_pan_auto = pan_auto;
        }
        if pan_auto {
            if !self.was_pan_auto {
                // Switched on: measure and apply at once (deskHPSDR's "first run").
                self.force();
                self.high_rule_pending = true;
            }
            let now = Instant::now();
            let due = self.smoothed.is_none() || self.last_measure.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1));
            let mut measured = false;
            if due && row.len() > 1 {
                let mut v: Vec<f32> = row.iter().copied().filter(|x| x.is_finite()).collect();
                if !v.is_empty() {
                    v.sort_by(|a, b| a.total_cmp(b));
                    let idx = ((0.6 * v.len() as f64) as usize).min(v.len() - 1);
                    let level = v[idx] as f64 + 3.0;
                    self.smoothed = Some(match self.smoothed {
                        None => level,
                        Some(s) => s + (level - s) * 0.25,
                    });
                    self.last_measure = Some(now);
                    measured = true;
                }
            }
            let calc_due = !self.calculated || self.last_calc.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(5));
            if measured && calc_due {
                if let Some(noise) = self.smoothed {
                    let adjusted = autoscale_low(noise, margin) - 5;
                    let current = lo.round() as i32;
                    // The first calculation (after switching on or a force) always applies.
                    if !self.calculated || (adjusted - current).abs() > 10 || current < adjusted {
                        if current != adjusted {
                            *lo = adjusted as f32;
                        }
                    }
                    if self.high_rule_pending {
                        self.high_rule_pending = false;
                        if *hi <= -50.0 {
                            *hi = -50.0;
                        }
                    }
                    self.last_calc = Some(now);
                    self.calculated = true;
                }
            }
        }
        self.was_pan_auto = pan_auto;

        if wf_auto {
            if let Some(w) = wf_row {
                // One update per new waterfall row, like deskHPSDR (it runs when a row is added).
                if self.last_wf_rev != Some(wf_rev) {
                    self.last_wf_rev = Some(wf_rev);
                    let mut sum = 0.0f64;
                    let mut n = 0usize;
                    for x in w.iter().filter(|x| x.is_finite()) {
                        sum += *x as f64;
                        n += 1;
                    }
                    if n > 0 {
                        let low = (sum / n as f64) as f32 + corr - 5.0;
                        *wlo = low;
                        *whi = low + 55.0;
                    }
                }
            }
        } else {
            self.last_wf_rev = None;
        }
    }
}

/// What the analyzer thread applies (spectrum.rs `set_display_avg`).
pub(crate) fn display_avg_of(c: &ConnectedState) -> DisplayAvg {
    DisplayAvg { detector: c.display_detector, mode: c.display_average_mode, time_ms: c.display_average_time_ms }
}

// ======================================================================= extra receivers

/// The values deskHPSDR applies to every receiver from this menu.
#[derive(Clone, Copy, PartialEq)]
struct Shared {
    db_low: f32,
    db_high: f32,
    wf_low: f32,
    wf_high: f32,
    palette: Palette,
    wf_enabled: bool,
    ratio: f32,
}

fn shared_of(c: &ConnectedState) -> Shared {
    Shared {
        db_low: c.db_low,
        db_high: c.db_high,
        wf_low: c.waterfall_db_low,
        wf_high: c.waterfall_db_high,
        palette: c.waterfall_palette,
        wf_enabled: c.waterfall_enabled,
        ratio: c.spectrum_waterfall_ratio,
    }
}

/// Writes the fields that differ between `before` and `after` into every extra receiver (fields they lack are ignored).
fn mirror_to_extra(c: &ConnectedState, before: Shared, after: Shared) {
    for rx in &c.extra_receivers {
        let mut rx = rx.lock().unwrap();
        if before.db_low != after.db_low {
            rx.db_low = after.db_low;
        }
        if before.db_high != after.db_high {
            rx.db_high = after.db_high;
        }
        if before.wf_low != after.wf_low {
            rx.waterfall_db_low = after.wf_low;
        }
        if before.wf_high != after.wf_high {
            rx.waterfall_db_high = after.wf_high;
        }
        if before.palette != after.palette {
            rx.waterfall_palette = after.palette;
        }
        if before.wf_enabled != after.wf_enabled {
            rx.waterfall_enabled = after.wf_enabled;
        }
        if before.ratio != after.ratio {
            rx.spectrum_waterfall_ratio = after.ratio;
        }
    }
}

// ======================================================================= small layout helpers

/// Like tx_window::child, optionally disabled (greyed, inert).
fn cell(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, dy: f32, enabled: bool) -> egui::Ui {
    let r = egui::Rect::from_min_max(egui::pos2(row.left() + x0, row.top() + dy), egui::pos2(row.left() + x1, row.bottom() + dy));
    let mut b = egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center));
    if !enabled {
        b = b.disabled();
    }
    ui.new_child(b)
}

/// Label painted left-aligned at x (pixels from the row's left edge), greyed when `enabled` is false.
fn label(ui: &mut egui::Ui, row: egui::Rect, x: f32, text: &str, enabled: bool) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if enabled { ui.visuals().text_color() } else { egui::Color32::from_gray(110) };
    ui.painter().text(egui::pos2(row.left() + x + 4.0, row.center().y), egui::Align2::LEFT_CENTER, text, font, color);
}

/// Label at `x0` and a spin cell at `sx` (SPIN_W wide). True when the user changed the value.
#[allow(clippy::too_many_arguments)]
fn spin_row(ui: &mut egui::Ui, row: egui::Rect, x0: f32, sx: f32, text: &str, id: &str, v: &mut f64, min: f64, max: f64, step: f64, dec: usize, enabled: bool) -> bool {
    label(ui, row, x0, text, enabled);
    let mut c = cell(ui, row, sx, sx + SPIN_W, -6.0, enabled);
    spin_buttons_full(&mut c, id, v, min, max, step, dec, None, SPIN_W - 98.0).changed()
}

/// Checkbox in x0..x1; true when toggled.
fn check_cell(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, v: &mut bool, text: &str) -> bool {
    let mut c = cell(ui, row, x0, x1, 0.0, true);
    std_checkbox(&mut c, v, text).changed()
}

/// A greyed (inactive) checkbox with a "!" that says so; no storage.
fn inactive_check(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, id: &str, text: &str) {
    let mut c = cell(ui, row, x0, x1, 0.0, true);
    c.spacing_mut().item_spacing.x = 12.0;
    let mut off = false;
    c.add_enabled_ui(false, |u| {
        std_checkbox(u, &mut off, text);
    });
    help_button(&mut c, id, NOT_IMPLEMENTED);
}

// ======================================================================= window

/// Returns (close, changed).
///
/// The menu covers the spectrum and the waterfall, which is exactly what the values below change. So the first tap on any - / + collapses it to
/// a bar at the bottom that shows only the row of that control (same code, scrolled to that row and clipped): the spectrum stays visible
/// while it is adjusted. That first tap does NOT change the value (so the first change is seen with the spectrum already visible); the bar
/// stays until "Show" (or Close / Esc).
/// A press that still being held from the switch is ignored too (the `spin_suppress` flag read by spin_buttons_full).
pub fn display_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let shared0 = shared_of(connected);
    let page_id = egui::Id::new("display_window_page");
    let bar_id = egui::Id::new("display_window_bar");
    let press_id = egui::Id::new("last_spin_press");
    let mut page: u8 = ui.ctx().data(|d| d.get_temp(page_id)).unwrap_or(0);
    // (row offset, the press that switched to the bar is still being held)
    let mut bar: Option<(f32, bool)> = ui.ctx().data(|d| d.get_temp(bar_id));
    let w = screen.width() - 58.0;
    // A press on a - / + this frame (spin_buttons_full records where): fresh when it happened in the last 0.25 s.
    let now_t = ui.input(|i| i.time);
    let press: Option<f32> = ui.ctx().data(|d| d.get_temp::<(f32, f64)>(press_id)).filter(|(_, t)| now_t - *t < 0.25).map(|(y, _)| y);

    if let Some((y_off, holding)) = bar {
        // ---- Bar mode.
        let mut show = false;
        let holding_now = holding && ui.input(|i| i.pointer.any_down());
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("spin_suppress"), holding_now));
        let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
        egui::Window::new("display_bar")
            .id(egui::Id::new("display_bar_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .frame(frame)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -(crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN + 10.0)))
            .show(ui.ctx(), |ui| {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close_now = true;
                }
                ui.set_width(w);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.horizontal(|ui| {
                    if kiosk_accent_button(ui, "Show").clicked() {
                        show = true;
                    }
                    ui.label(match page {
                        0 => "Display - General Settings",
                        1 => "Display - Peak Blobs & Hold",
                        _ => "Display - Peak Labels",
                    });
                });
                egui::ScrollArea::vertical()
                    .id_salt("display_bar_scroll")
                    .vertical_scroll_offset(y_off)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .scroll_source(egui::scroll_area::ScrollSource::NONE)
                    .max_height(60.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                        match page {
                            0 => general_page(ui, w, connected, &mut changed),
                            1 => blobs_page(ui, w, connected, &mut changed),
                            _ => peak_labels_page(ui, w, connected, &mut changed),
                        }
                    });
            });
        ui.ctx().data_mut(|d| d.remove::<bool>(egui::Id::new("spin_suppress")));
        bar = if show { None } else { Some((y_off, holding_now)) };
    } else {
        // ---- Full menu.
        let mut new_bar_off: Option<f32> = None;
        // The first tap on a - / + here only switches to the bar: it must not change the value.
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("spin_suppress"), true));
        let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
        egui::Window::new("Display")
            .id(egui::Id::new("display_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .frame(frame)
            .fixed_pos(screen.min)
            .constrain_to(screen)
            .fixed_size(screen.size() - egui::vec2(58.0, 10.0))
            .show(ui.ctx(), |ui| {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close_now = true;
                }
                ui.set_width(w);
                ui.set_min_height(screen.height() - 10.0);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
                ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - Display", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

                // Page selector (round radios) and CLOSE at the top right.
                let row = new_row(ui, w, 54.0);
                for (i, (x0, title)) in [(10.0, "General Settings"), (240.0, "Peak Blobs & Hold"), (480.0, "Peak Labels")].iter().enumerate() {
                    let mut c = child(ui, row, *x0, *x0 + 220.0, false, 0.0);
                    if touch_radio(&mut c, page == i as u8, title).clicked() {
                        page = i as u8;
                    }
                }
                egui::Area::new(egui::Id::new("display_window_close"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                    .show(ui.ctx(), |ui| {
                        if kiosk_accent_button(ui, "CLOSE").clicked() {
                            close_now = true;
                        }
                    });
                ui.add_space(4.0);

                let page_top = ui.cursor().top();
                match page {
                    0 => general_page(ui, w, connected, &mut changed),
                    1 => blobs_page(ui, w, connected, &mut changed),
                    _ => peak_labels_page(ui, w, connected, &mut changed),
                }
                // A - / + was pressed in this frame: remember where its row is (a little above the buttons, so the label shows too).
                if let Some(top) = press {
                    new_bar_off = Some((top - page_top - 12.0).max(0.0));
                }
            });
        ui.ctx().data_mut(|d| d.remove::<bool>(egui::Id::new("spin_suppress")));
        if let Some(off) = new_bar_off {
            bar = Some((off, true));
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(page_id, page));
    if let Some(b) = bar {
        ui.ctx().data_mut(|d| d.insert_temp(bar_id, b));
    } else {
        ui.ctx().data_mut(|d| d.remove::<(f32, bool)>(bar_id));
    }
    if close_now {
        ui.ctx().data_mut(|d| d.remove::<(f32, bool)>(bar_id));
    }

    let shared1 = shared_of(connected);
    if shared1 != shared0 {
        mirror_to_extra(connected, shared0, shared1);
        if shared1.db_low != shared0.db_low || shared1.db_high != shared0.db_high || shared1.wf_low != shared0.wf_low || shared1.wf_high != shared0.wf_high {
            let freq = connected.session.frequency_hz.load(std::sync::atomic::Ordering::Relaxed);
            let mode = connected.spectrum.mode();
            crate::remember_band_settings(&mut connected.band_memory, freq, connected.db_low, connected.db_high, connected.waterfall_db_low, connected.waterfall_db_high, mode);
        }
    }
    (close_now, changed)
}

// ======================================================================= General Settings

fn general_page(ui: &mut egui::Ui, w: f32, c: &mut ConnectedState, changed: &mut bool) {
    const LS: f32 = 290.0; // left column: spin cells start here (labels are left of it)
    const RS: f32 = R0 + 130.0; // right column: combos / spins start here
    let pan_auto = c.db_low_auto;
    let wf_auto = c.waterfall_db_low_auto;

    // ---- row 0: Frames Per Second | Detector
    let row = new_row(ui, w, GRID_H);
    let mut fps = c.spectrum_fps as f64;
    if spin_row(ui, row, L0, LS, "Frames Per Second:", "dsp_fps", &mut fps, 5.0, 60.0, 5.0, 0, true) {
        c.spectrum_fps = fps.round() as u32;
        c.tx_spectrum_fps = c.spectrum_fps;
        *changed = true;
    }
    {
        let mut hc = cell(ui, row, 200.0, 240.0, 0.0, true);
        help_button(&mut hc, "dsp_fps_help", FPS_HELP);
    }
    label(ui, row, R0, "Detector:", true);
    {
        let mut cc = cell(ui, row, RS, RS + 270.0, 0.0, true);
        cc.spacing_mut().item_spacing.x = 12.0;
        let items: [(&str, bool); 4] = [("Peak", true), ("Rosenfell", true), ("Average", true), ("Sample", true)];
        if let Some(i) = choice_combo_h(&mut cc, "dsp_detector", 200.0, c.display_detector as usize, &items, 34.0) {
            c.display_detector = i as u8;
            *changed = true;
        }
        help_button(&mut cc, "dsp_avg_help", AVG_HELP);
    }

    // ---- row 1: Relation | Averaging
    let row = new_row(ui, w, GRID_H);
    let mut pct = ((c.spectrum_waterfall_ratio * 100.0).round() as f64).clamp(30.0, 80.0);
    if spin_row(ui, row, L0, LS, "Relation Pan<->Waterfall (%):", "dsp_rel", &mut pct, 30.0, 80.0, 5.0, 0, true) {
        c.spectrum_waterfall_ratio = (pct / 100.0) as f32;
        *changed = true;
    }
    label(ui, row, R0, "Averaging:", true);
    {
        let mut cc = cell(ui, row, RS, RS + 230.0, 0.0, true);
        let items: [(&str, bool); 4] = [("None", true), ("Recursive", true), ("Time Window", true), ("Log Recursive", true)];
        if let Some(i) = choice_combo_h(&mut cc, "dsp_averaging", 200.0, c.display_average_mode as usize, &items, 34.0) {
            c.display_average_mode = i as u8;
            *changed = true;
        }
    }

    // ---- row 2: Panadapter High | Av. Time
    let row = new_row(ui, w, GRID_H);
    let mut v = c.db_high as f64;
    if spin_row(ui, row, L0, LS, "Panadapter High:", "dsp_pan_high", &mut v, -175.0, 50.0, 1.0, 0, true) {
        c.db_high = v.round() as f32;
        *changed = true;
    }
    let mut t = c.display_average_time_ms as f64;
    if spin_row(ui, row, R0, RS, "Av. Time (ms):", "dsp_av_time", &mut t, 1.0, 9999.0, 10.0, 0, true) {
        c.display_average_time_ms = t.round().clamp(1.0, 9999.0) as u32;
        *changed = true;
    }

    // ---- row 3: Panadapter Low | Display Panadapter + Display Waterfall
    let row = new_row(ui, w, GRID_H);
    let mut v = c.db_low as f64;
    if spin_row(ui, row, L0, LS, "Panadapter Low:", "dsp_pan_low", &mut v, -175.0, 50.0, 1.0, 0, !pan_auto) {
        c.db_low = v.round() as f32;
        *changed = true;
    }
    let mut dp = c.display_panadapter;
    if check_cell(ui, row, R0, R0 + 238.0, &mut dp, "Display Panadapter") {
        c.display_panadapter = dp;
        if !dp && !c.waterfall_enabled {
            c.waterfall_enabled = true;
        }
        *changed = true;
    }
    let mut dw = c.waterfall_enabled;
    if check_cell(ui, row, R0 + 238.0, R1, &mut dw, "Display Waterfall") {
        c.waterfall_enabled = dw;
        if !dw && !c.display_panadapter {
            c.display_panadapter = true;
        }
        *changed = true;
    }

    // ---- row 4: Panadapter Step | Fill + Gradient
    let row = new_row(ui, w, GRID_H);
    let mut v = c.panadapter_step_db as f64;
    if spin_row(ui, row, L0, LS, "Panadapter Step:", "dsp_pan_step", &mut v, 1.0, 20.0, 1.0, 0, true) {
        c.panadapter_step_db = v.round() as f32;
        *changed = true;
    }
    let mut f = c.spectrum_filled;
    if check_cell(ui, row, R0, R0 + 238.0, &mut f, "Fill Panadapter") {
        c.spectrum_filled = f;
        *changed = true;
    }
    let mut g = c.spectrum_gradient;
    if check_cell(ui, row, R0 + 238.0, R1, &mut g, "Gradient Panadapter") {
        c.spectrum_gradient = g;
        *changed = true;
    }

    // ---- row 5: Waterfall High | Show Worldmap
    let row = new_row(ui, w, GRID_H);
    // While Waterfall Automatic is on the greyed spin shows the live automatic value; the manual one is untouched.
    let mut v = if wf_auto { c.wf_auto_high } else { c.waterfall_db_high } as f64;
    if spin_row(ui, row, L0, LS, "Waterfall High:", "dsp_wf_high", &mut v, -175.0, 50.0, 1.0, 0, !wf_auto) && !wf_auto {
        c.waterfall_db_high = v.round() as f32;
        *changed = true;
    }
    inactive_check(ui, row, R0, R1, "dsp_worldmap", "Show Worldmap");

    // ---- row 6: Waterfall Low | 3D Waterfall History
    let row = new_row(ui, w, GRID_H);
    let mut v = if wf_auto { c.wf_auto_low } else { c.waterfall_db_low } as f64;
    if spin_row(ui, row, L0, LS, "Waterfall Low:", "dsp_wf_low", &mut v, -175.0, 50.0, 1.0, 0, !wf_auto) && !wf_auto {
        c.waterfall_db_low = v.round() as f32;
        *changed = true;
    }
    inactive_check(ui, row, R0, R1, "dsp_3d", "3D Waterfall History");

    // ---- row 7: Waterfall Automatic | Display Info Bar
    let row = new_row(ui, w, GRID_H);
    {
        let mut cc = cell(ui, row, L0, L1, 0.0, true);
        cc.spacing_mut().item_spacing.x = 12.0;
        let mut a = c.waterfall_db_low_auto;
        if std_checkbox(&mut cc, &mut a, "Waterfall Automatic").changed() {
            c.waterfall_db_low_auto = a;
            *changed = true;
        }
        help_button(&mut cc, "dsp_wf_auto_help", WF_AUTO_HELP);
    }
    inactive_check(ui, row, R0, R1, "dsp_infobar", "Display Info Bar");

    // ---- row 8: Panadapter Automatic | Show Solardata in Info Bar
    let row = new_row(ui, w, GRID_H);
    {
        let mut cc = cell(ui, row, L0, L1, 0.0, true);
        cc.spacing_mut().item_spacing.x = 12.0;
        let mut a = c.db_low_auto;
        if std_checkbox(&mut cc, &mut a, "Panadapter Automatic (Related to Noisefloor)").changed() {
            c.db_low_auto = a;
            *changed = true;
        }
        help_button(&mut cc, "dsp_pan_auto_help", PAN_AUTO_HELP);
    }
    inactive_check(ui, row, R0, R1, "dsp_solar", "Show Solardata in Info Bar");

    // ---- row 9: Noisefloor Margin | Show clock & UDP broadcast
    let row = new_row(ui, w, GRID_H);
    let mut v = c.panadapter_noise_margin as f64;
    if spin_row(ui, row, L0, LS, "Noisefloor Margin:", "dsp_margin", &mut v, -20.0, 10.0, 1.0, 0, true) {
        c.panadapter_noise_margin = v.round() as i32;
        c.display_auto.force();
        *changed = true;
    }
    inactive_check(ui, row, R0, R1, "dsp_clock", "Show clock & UDP broadcast");

    // ---- row 10: Waterfall palette chips (right column)
    let row = new_row(ui, w, GRID_H);
    {
        // Left column of this row was free: "Smooth trace" (off = deskHPSDR's raw per-pixel trace).
        let mut cc = cell(ui, row, L0, L1, 0.0, true);
        cc.spacing_mut().item_spacing.x = 12.0;
        let mut s = c.spectrum_smooth_trace;
        if std_checkbox(&mut cc, &mut s, "Smooth trace").changed() {
            c.spectrum_smooth_trace = s;
            *changed = true;
        }
        help_button(&mut cc, "dsp_smooth_help", SMOOTH_HELP);
    }
    label(ui, row, R0, "Palette:", true);
    {
        let mut cc = cell(ui, row, R0 + 72.0, R1, 0.0, true);
        cc.spacing_mut().item_spacing.x = 8.0;
        // Five chips must fit the 394 px right of "Palette:": each is as wide as its label needs (measured, so the
        // kiosk UI scale cannot make it overflow) plus the button padding.
        let font = egui::TextStyle::Button.resolve(cc.style());
        for p in ALL_PALETTES.iter() {
            let tw = cc.painter().layout_no_wrap(p.label().to_string(), font.clone(), egui::Color32::WHITE).size().x;
            let cw = (tw + 18.0).max(48.0);
            if cc.add(chip_button(p.label(), *p == c.waterfall_palette).min_size(egui::vec2(cw, 34.0))).clicked() {
                c.waterfall_palette = *p;
                *changed = true;
            }
        }
    }
}

// ======================================================================= Peak Blobs & Hold

/// "R"/"G"/"B" spin buttons (0..255, step 5) and a swatch for an RGBA colour (alpha is not editable). True when changed.
fn colour_row(ui: &mut egui::Ui, row: egui::Rect, text: &str, id: &str, col: &mut [f32; 4]) -> bool {
    label(ui, row, L0, text, true);
    let sw = egui::Rect::from_min_size(egui::pos2(row.left() + 250.0, row.center().y - 15.0), egui::vec2(60.0, 30.0));
    let shown = egui::Color32::from_rgb((col[0] * 255.0).round() as u8, (col[1] * 255.0).round() as u8, (col[2] * 255.0).round() as u8);
    ui.painter().rect(sw, 5.0, shown, egui::Stroke::new(1.0, egui::Color32::from_gray(150)), egui::StrokeKind::Inside);
    let mut changed = false;
    for (k, name) in ["R", "G", "B"].iter().enumerate() {
        let x0 = 330.0 + k as f32 * 196.0;
        label(ui, row, x0 - 4.0, name, true);
        let mut v = (col[k] * 255.0).round() as f64;
        let mut c = cell(ui, row, x0 + 16.0, x0 + 16.0 + SPIN_W, -6.0, true);
        if spin_buttons_full(&mut c, &format!("{id}_{name}"), &mut v, 0.0, 255.0, 5.0, 0, None, SPIN_W - 98.0).changed() {
            col[k] = (v.clamp(0.0, 255.0) / 255.0) as f32;
            changed = true;
        }
    }
    changed
}

fn blobs_page(ui: &mut egui::Ui, w: f32, c: &mut ConnectedState, changed: &mut bool) {
    let mut t = c.tx_ui;
    let t0 = t;
    const SX: f32 = 330.0;

    let row = new_row(ui, w, GRID_H);
    check_cell(ui, row, L0, 600.0, &mut t.peak_hold_on, "Enable PEAKS & HOLD");

    let row = new_row(ui, w, GRID_H);
    label(ui, row, L0, "Type of Peaks & Hold function:", true);
    {
        let mut cc = cell(ui, row, SX, SX + 270.0, 0.0, true);
        let items: [(&str, bool); 2] = [("Peaks hold", true), ("Peaks decay", true)];
        if let Some(i) = choice_combo_h(&mut cc, "dsp_pk_type", 200.0, if t.peak_hold_mode == 1 { 0 } else { 1 }, &items, 34.0) {
            t.peak_hold_mode = if i == 0 { 1 } else { 2 };
        }
    }

    let row = new_row(ui, w, GRID_H);
    let mut hold = t.peak_hold_sec as f64;
    if spin_row(ui, row, L0, SX, "Decay hold time (s):", "dsp_pk_time", &mut hold, 0.1, 5.0, 0.1, 1, true) {
        t.peak_hold_sec = (hold.clamp(0.1, 5.0) * 10.0).round() as f32 / 10.0;
    }

    let row = new_row(ui, w, GRID_H);
    let mut drop = t.peak_hold_drop_db as f64;
    if spin_row(ui, row, L0, SX, "Drop (dbm/s):", "dsp_pk_drop", &mut drop, 1.0, 10.0, 0.5, 1, true) {
        t.peak_hold_drop_db = (drop.clamp(1.0, 10.0) * 2.0).round() as f32 / 2.0;
    }

    let row = new_row(ui, w, GRID_H);
    {
        let mut cc = cell(ui, row, L0, 600.0, 0.0, true);
        cc.spacing_mut().item_spacing.x = 12.0;
        std_checkbox(&mut cc, &mut t.peak_hold_tx, "Enable PEAKS & HOLD for TX");
        help_button(&mut cc, "dsp_pk_tx_help", PEAKS_TX_HELP);
    }

    let row = new_row(ui, w, GRID_H);
    colour_row(ui, row, "PEAKS & HOLD line color", "dsp_pk_col", &mut t.peak_line_col);
    let row = new_row(ui, w, GRID_H);
    colour_row(ui, row, "TX pan line/fill color", "dsp_tx_col", &mut t.tx_pan_col);

    if t != t0 {
        c.tx_ui = t;
        *changed = true;
    }
}

// ======================================================================= Peak Labels

/// Two columns: RX (left, S-meter option as deskHPSDR's receive panadapter) and TX (right, its own parameter set, labels
/// always in dBm). Budget: 8 rows x 44 px = 352 px of the ~500 px below the page selector; each column is 470 / 476 px
/// wide inside the 966 px content (24 px side margins, 20 px between the columns), spin cell 168 px at x = 250.
fn peak_labels_page(ui: &mut egui::Ui, w: f32, c: &mut ConnectedState, changed: &mut bool) {
    let mut t = c.tx_ui;
    let t0 = t;
    const LX: f32 = 250.0; // spin cell start inside a column

    let row = new_row(ui, w, GRID_H);
    check_cell(ui, row, L0, 700.0, &mut t.peaks_on, "Enable Peak Labels on Panadapter");

    let row = new_row(ui, w, GRID_H);
    label(ui, row, L0, "RX", true);
    label(ui, row, R0, "TX", true);

    let row = new_row(ui, w, GRID_H);
    check_cell(ui, row, L0, L1, &mut t.peaks_as_smeter, "Peak Labels as S-Meter values");
    label(ui, row, R0, "(TX labels are always in dBm)", false);

    let row = new_row(ui, w, GRID_H);
    check_cell(ui, row, L0, L1, &mut t.peaks_in_passband, "Peak Labels in Passband Only");
    check_cell(ui, row, R0, R1, &mut t.peaks_tx_in_passband, "Peak Labels in Passband Only");

    let row = new_row(ui, w, GRID_H);
    check_cell(ui, row, L0, L1, &mut t.peaks_hide_noise, "Hide Peaks Below Noise Floor");
    check_cell(ui, row, R0, R1, &mut t.peaks_tx_hide_noise, "Hide Peaks Below Noise Floor");

    let row = new_row(ui, w, GRID_H);
    let mut v = t.peaks_num as f64;
    if spin_row(ui, row, L0, LX, "Number of Peaks:", "dsp_pk_num", &mut v, 1.0, 10.0, 1.0, 0, true) {
        t.peaks_num = v.round() as i32;
    }
    let mut v = t.peaks_tx_num as f64;
    if spin_row(ui, row, R0, R0 + LX, "Number of Peaks:", "dsp_pk_tx_num", &mut v, 1.0, 10.0, 1.0, 0, true) {
        t.peaks_tx_num = v.round() as i32;
    }

    let row = new_row(ui, w, GRID_H);
    let mut v = t.peaks_ignore_divider as f64;
    if spin_row(ui, row, L0, LX, "Ignore Adjacent Peaks:", "dsp_pk_ign", &mut v, 1.0, 150.0, 1.0, 0, true) {
        t.peaks_ignore_divider = v.round() as i32;
    }
    let mut v = t.peaks_tx_divider as f64;
    if spin_row(ui, row, R0, R0 + LX, "Ignore Adjacent Peaks:", "dsp_pk_tx_ign", &mut v, 1.0, 150.0, 1.0, 0, true) {
        t.peaks_tx_divider = v.round() as i32;
    }

    let row = new_row(ui, w, GRID_H);
    let mut v = t.peaks_noise_percentile as f64;
    if spin_row(ui, row, L0, LX, "Noise Floor Percentile:", "dsp_pk_pct", &mut v, 1.0, 100.0, 1.0, 0, true) {
        t.peaks_noise_percentile = v.round() as i32;
    }
    let mut v = t.peaks_tx_percentile as f64;
    if spin_row(ui, row, R0, R0 + LX, "Noise Floor Percentile:", "dsp_pk_tx_pct", &mut v, 1.0, 100.0, 1.0, 0, true) {
        t.peaks_tx_percentile = v.round() as i32;
    }

    if t != t0 {
        c.tx_ui = t;
        *changed = true;
    }
}
