//! The "DSP" menu (deskHPSDR fft_menu.c, the DSP button of the Menu): same in-app window style as the AGC / BAND / MODE /
//! FILTER menus (compact, anchored above the toolbar, Close top left, stays open until Close).
//! Two pages, because the full deskHPSDR table is too tall for the 1024x600 screen and would hide the spectrum:
//! * **Filters** (one column per channel, RX1 and TX): WDSP FIR filter type (linear phase / low latency), FIR size NC
//!   (2048..16384) and Binaural (RX only).
//! * **IQ** (RX1): RX image measure, offset, manual IQ gain / phase and Reset in a short strip, so the spectrum and the
//!   image stay visible while adjusting; the measured IRR is also shown inside the window.
//! The settings live in `RxExtra` / `TxExtra` (already saved with the configuration) and are sent to WDSP by the DSP
//! threads when they change (spectrum.rs `last_fir`, tx.rs `last_fir`).

use crate::noise_window::choice_combo_h;
use crate::{help_button, spin_buttons_full, std_checkbox, touch_close_button, ConnectedState};

const LABEL_W: f32 = 228.0;
const COL_W: f32 = 172.0;
const ROW_H: f32 = 46.0;
const GAP: f32 = 6.0;
/// Width of the value cells of the IQ page.
const IQ_CELL_W: f32 = 176.0;
/// Spectrum / waterfall ratio while the IQ page is open (45 %).
const IQ_RATIO: f32 = 0.45;
const NC_VALUES: [i32; 4] = [2048, 4096, 8192, 16384];
const TYPE_HELP: &str = "TX Low Latency sets CESSB option to DISABLED.\n\nThese two functions cannot be used simultaneously. Setting Linear Phase is REQUIRED when using CESSB.\n\nNote: RX is not affected. Selection is unrestricted.";
const NC_HELP: &str = "Sets the number of coefficients (NC) used by the WDSP FIR filters.\n\nHigher values provide steeper filter skirts at the cost of increased CPU load.\n\nDefault setting: 2048 (change only for a specific reason).";
const BIN_HELP: &str = "Outputs I and Q on the Left and Right audio channels.\n\nIf the audio output device is mono or NNR is active, the Binaural option is not available or switched off.";
const MEASURE_HELP: &str = "RX Image Measure: enables the RX image rejection measurement in the panadapter. The signal at +offset from the centre of the display is compared with its mirror at -offset and shown as IRR (dB); the larger, the better.\n\nImage Offset Hz: offset in Hz of the measured signal (100 to 10000). Put a strong, narrow signal at that offset above the centre of the display.";
const IQ_HELP: &str = "RX IQ Gain: manual RX IQ gain correction in dB.\nRX IQ Phase: manual RX IQ phase correction in degrees.\nReset: sets both back to 0.00.\n\nAdjust them while watching the IRR value: the mirror image gets smaller when the correction is right.";

fn label_box_w(ui: &mut egui::Ui, text: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::hover());
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(40), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
}

/// A fixed-width cell (so the two IQ rows line up); `f` draws its content.
fn cell(ui: &mut egui::Ui, w: f32, f: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::hover());
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    f(&mut c);
}

fn label_box(ui: &mut egui::Ui, text: &str) {
    label_box_w(ui, text, LABEL_W);
}

/// Column header text, centred over a column.
fn header(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(COL_W, 28.0), egui::Sense::hover());
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(18.0), egui::Color32::from_gray(225));
}

/// Returns (close, changed).
pub fn dsp_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mut close_now = false;
    let mut changed = false;
    let can_tx = connected.tx_handle.is_some();
    let mut rx = connected.spectrum.rx_extra();
    let rx0 = rx;
    let mut tx = connected.tx_handle.as_ref().map(|t| t.tx_extra());
    let tx0 = tx;
    let mut binaural = connected.spectrum.binaural();
    let binaural0 = binaural;
    let mut image_measure = connected.image_measure;
    let image_measure0 = image_measure;
    let irr = connected.image_irr;
    let status = connected.iq_status.clone();
    let running = connected.iq_auto.is_some();
    let mut auto_clicked = false;
    let mut reset_clicked = false;
    let tab_id = egui::Id::new("dsp_window_tab");
    let mut tab: u8 = ui.ctx().data(|d| d.get_temp(tab_id)).unwrap_or(0);

    egui::Window::new("dsp_menu")
        .id(egui::Id::new("dsp_menu_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // Same placement rule as the AGC / filter menus: centred, anchored above the toolbar with a gap.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -58.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if touch_close_button(ui, ROW_H).clicked() {
                    close_now = true;
                }
                ui.label("DSP");
                ui.add_space(12.0);
                // Page tabs: the active one is blue, like the other pages of the kiosk menus.
                for (i, label) in ["Filters", "IQ"].iter().enumerate() {
                    let active = tab == i as u8;
                    let btn = egui::Button::new(egui::RichText::new(*label).color(if active { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if active { egui::Color32::from_rgb(70, 150, 245) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(100.0, ROW_H))
                        .corner_radius(5.0);
                    if ui.add(btn).clicked() {
                        tab = i as u8;
                    }
                }
                if tab == 1 {
                    ui.add_space(12.0);
                    // Auto RX IQ and its status live in the header row, so the two value rows stay short.
                    if ui.add_enabled(!running, egui::Button::new("Auto RX IQ").min_size(egui::vec2(130.0, ROW_H))).clicked() {
                        auto_clicked = true;
                    }
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(210.0, ROW_H), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 5.0, egui::Color32::from_gray(20));
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, &status, egui::FontId::proportional(16.0), egui::Color32::from_gray(220));
                }
            });

            if tab == 0 {
                // Column headers.
                ui.horizontal(|ui| {
                    ui.add_space(LABEL_W + GAP);
                    header(ui, "RX1");
                    if can_tx {
                        header(ui, "TX");
                    }
                });

                let items = [("Linear Phase", true), ("Low Latency", true)];
                let labels: Vec<String> = NC_VALUES.iter().map(|v| v.to_string()).collect();
                let nc_items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
                let pos = |nc: i32| NC_VALUES.iter().position(|v| *v == nc).unwrap_or(0);

                // FIR filter type.
                ui.horizontal(|ui| {
                    label_box(ui, "WDSP FIR Filter Type");
                    if let Some(i) = choice_combo_h(ui, "dsp_rx_type", COL_W, rx.fir_low_latency as usize, &items, 34.0) {
                        rx.fir_low_latency = i == 1;
                    }
                    if let Some(t) = tx.as_mut() {
                        if let Some(i) = choice_combo_h(ui, "dsp_tx_type", COL_W, t.fir_low_latency as usize, &items, 34.0) {
                            t.fir_low_latency = i == 1;
                            if t.fir_low_latency {
                                // deskHPSDR tx_set_latency(): low latency and CESSB exclude each other.
                                t.cessb_enable = false;
                            }
                        }
                    }
                    help_button(ui, "dsp_help_type", TYPE_HELP);
                });
                // FIR size.
                ui.horizontal(|ui| {
                    label_box(ui, "WDSP FIR Filter NC");
                    if let Some(i) = choice_combo_h(ui, "dsp_rx_nc", COL_W, pos(rx.fir_nc), &nc_items, 34.0) {
                        rx.fir_nc = NC_VALUES[i];
                    }
                    if let Some(t) = tx.as_mut() {
                        if let Some(i) = choice_combo_h(ui, "dsp_tx_nc", COL_W, pos(t.fir_nc), &nc_items, 34.0) {
                            t.fir_nc = NC_VALUES[i];
                        }
                    }
                    help_button(ui, "dsp_help_nc", NC_HELP);
                });
                // Binaural (RX only).
                ui.horizontal(|ui| {
                    label_box(ui, "Binaural");
                    // Both cells reserve their full width so the "!" lines up with the other rows.
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(COL_W, ROW_H), egui::Sense::hover());
                    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    std_checkbox(&mut c, &mut binaural, "");
                    if can_tx {
                        ui.allocate_exact_size(egui::vec2(COL_W, ROW_H), egui::Sense::hover());
                    }
                    help_button(ui, "dsp_help_bin", BIN_HELP);
                });
            } else {
                // IQ page: two short rows, so the spectrum above stays visible.
                ui.horizontal(|ui| {
                    label_box_w(ui, "RX Image Measure", 160.0);
                    cell(ui, IQ_CELL_W, |ui| {
                        std_checkbox(ui, &mut image_measure, "");
                    });
                    label_box_w(ui, "Image Offset Hz", 130.0);
                    cell(ui, IQ_CELL_W, |ui| {
                        let mut v = rx.image_measure_hz as f64;
                        if spin_buttons_full(ui, "dsp_image_offset", &mut v, 100.0, 10000.0, 10.0, 0, None, 70.0).changed() {
                            rx.image_measure_hz = v.round() as i32;
                        }
                    });
                    // The measured value, readable here even when the spectrum label is hidden behind the window.
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, ROW_H), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 5.0, egui::Color32::from_gray(20));
                    let (text, col) = match irr {
                        Some((_, rej)) => (format!("IRR {:.1} dB", rej.abs()), egui::Color32::WHITE),
                        None => ("IRR --".to_string(), egui::Color32::from_gray(120)),
                    };
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(20.0), col);
                    help_button(ui, "dsp_help_measure", MEASURE_HELP);
                });
                ui.horizontal(|ui| {
                    label_box_w(ui, "RX IQ Gain", 160.0);
                    cell(ui, IQ_CELL_W, |ui| {
                        let mut v = rx.iq_gain_db as f64;
                        if spin_buttons_full(ui, "dsp_iq_gain", &mut v, -5.0, 5.0, 0.01, 2, None, 70.0).changed() {
                            rx.iq_gain_db = ((v * 100.0).round() / 100.0) as f32;
                        }
                    });
                    label_box_w(ui, "RX IQ Phase", 130.0);
                    cell(ui, IQ_CELL_W, |ui| {
                        let mut v = rx.iq_phase_deg as f64;
                        if spin_buttons_full(ui, "dsp_iq_phase", &mut v, -20.0, 20.0, 0.01, 2, None, 70.0).changed() {
                            rx.iq_phase_deg = ((v * 100.0).round() / 100.0) as f32;
                        }
                    });
                    cell(ui, 150.0, |ui| {
                        if ui.add(egui::Button::new("Reset").min_size(egui::vec2(150.0, 38.0))).clicked() {
                            rx.iq_gain_db = 0.0;
                            rx.iq_phase_deg = 0.0;
                            reset_clicked = true;
                        }
                    });
                    help_button(ui, "dsp_help_iq", IQ_HELP);
                });
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    if reset_clicked {
        // deskHPSDR rx_iq_reset_cb: cancels a running calibration and sets the status back to Idle.
        iq_auto_cancel(connected, "Idle");
    }
    // IQ page: spectrum/waterfall ratio down to  so the IRR label and the image stay above the window; the old value
    // comes back when the page is left (or, in main.rs, when the window closes).
    if tab == 1 {
        if connected.dsp_saved_ratio.is_none() {
            connected.dsp_saved_ratio = Some(connected.spectrum_waterfall_ratio);
            connected.spectrum_waterfall_ratio = IQ_RATIO;
        }
    } else if let Some(r) = connected.dsp_saved_ratio.take() {
        connected.spectrum_waterfall_ratio = r;
    }

    if rx != rx0 {
        connected.spectrum.set_rx_extra(rx);
        changed = true;
    }
    if tx != tx0 {
        if let (Some(t), Some(h)) = (tx, connected.tx_handle.as_ref()) {
            h.set_tx_extra(t);
            changed = true;
        }
    }
    if image_measure != image_measure0 {
        connected.image_measure = image_measure;
    }
    if auto_clicked {
        iq_auto_start(connected);
    }
    if binaural != binaural0 {
        connected.spectrum.set_binaural(binaural);
        changed = true;
    }
    (close_now, changed)
}

// ---- Auto RX IQ (deskHPSDR fft_menu.c rx_iq_auto_thread), run as a state machine one step per UI frame ----

/// Longest a run may take (deskHPSDR: 10 s).
const AUTO_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
/// A measurement: wait 250 ms, then 5 samples of the IRR 100 ms apart (deskHPSDR rx_iq_auto_measure).
const MEASURE_WAIT_MS: u128 = 250;
const MEASURE_SAMPLES: u8 = 5;
const MEASURE_STEP_MS: u128 = 100;
/// Sweeps after the zero candidate: (centre follows the best value so far, range, step); gain, phase, gain, phase.
const SWEEPS: [(f64, f64); 4] = [(0.05, 0.005), (0.50, 0.05), (0.01, 0.001), (0.10, 0.01)];

struct Measure {
    t0: std::time::Instant,
    sum: f64,
    valid: u32,
    taken: u8,
}

/// A running Auto RX IQ calibration.
pub(crate) struct IqAuto {
    /// Gain, phase and |IRR| when the run started.
    start: (f32, f32, f64),
    /// Best gain, phase and |IRR| found so far.
    best: (f64, f64, f64),
    /// 0 = the zero candidate, 1..=4 = the sweeps of SWEEPS (odd = gain, even = phase).
    stage: usize,
    cands: Vec<f64>,
    idx: usize,
    meas: Option<Measure>,
    deadline: std::time::Instant,
}

impl IqAuto {
    /// (gain, phase) of the candidate being measured.
    fn current(&self) -> (f64, f64) {
        if self.stage == 0 {
            (0.0, 0.0)
        } else if self.stage % 2 == 1 {
            (self.cands[self.idx], self.best.1)
        } else {
            (self.best.0, self.cands[self.idx])
        }
    }

    /// Candidate values of sweep `stage` (1..=4) around the best value so far.
    fn build(&mut self) {
        let (range, step) = SWEEPS[self.stage - 1];
        let center = if self.stage % 2 == 1 { self.best.0 } else { self.best.1 };
        let limit = center + range + step * 0.5;
        self.cands.clear();
        let mut v = center - range;
        while v <= limit {
            self.cands.push(v);
            v += step;
        }
        self.idx = 0;
    }
}

fn set_correction(c: &mut ConnectedState, gain: f64, phase: f64) {
    let mut ex = c.spectrum.rx_extra();
    ex.iq_gain_db = gain as f32;
    ex.iq_phase_deg = phase as f32;
    c.spectrum.set_rx_extra(ex);
}

/// Starts the calibration (button "Auto RX IQ"): needs the image measurement on and a calibration signal.
pub(crate) fn iq_auto_start(c: &mut ConnectedState) {
    if c.iq_auto.is_some() {
        return;
    }
    let Some((signal_db, rej)) = c.image_irr.filter(|_| c.image_measure) else {
        c.iq_status = "No calibration signal".to_string();
        return;
    };
    if signal_db < -100.0 {
        c.iq_status = "No calibration signal".to_string();
        return;
    }
    let ex = c.spectrum.rx_extra();
    let irr = rej.abs() as f64;
    c.iq_auto = Some(IqAuto {
        start: (ex.iq_gain_db, ex.iq_phase_deg, irr),
        best: (ex.iq_gain_db as f64, ex.iq_phase_deg as f64, irr),
        stage: 0,
        cands: Vec::new(),
        idx: 0,
        meas: None,
        deadline: std::time::Instant::now() + AUTO_DEADLINE,
    });
    c.iq_status = "Auto calibrating...".to_string();
}

/// Stops a running calibration and puts the old correction back (Reset, or the measurement was switched off).
pub(crate) fn iq_auto_cancel(c: &mut ConnectedState, status: &str) {
    if let Some(a) = c.iq_auto.take() {
        set_correction(c, a.start.0 as f64, a.start.1 as f64);
    }
    c.iq_status = status.to_string();
}

fn iq_auto_finish(c: &mut ConnectedState, a: IqAuto) {
    if a.best.2 >= a.start.2 + 0.5 {
        set_correction(c, a.best.0, a.best.1);
        c.iq_status = format!("Complete ({:.1} dB)", a.best.2);
    } else {
        set_correction(c, a.start.0 as f64, a.start.1 as f64);
        c.iq_status = "No improvement".to_string();
    }
    c.settings_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// One step of the calibration, called every frame while the radio screen is shown.
pub(crate) fn iq_auto_tick(c: &mut ConnectedState, ctx: &egui::Context) {
    let Some(mut a) = c.iq_auto.take() else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(30));
    if !c.image_measure {
        c.iq_auto = Some(a);
        iq_auto_cancel(c, "Cancelled");
        return;
    }
    if std::time::Instant::now() >= a.deadline {
        iq_auto_finish(c, a);
        return;
    }
    if a.meas.is_none() {
        let (g, p) = a.current();
        set_correction(c, g, p);
        a.meas = Some(Measure { t0: std::time::Instant::now(), sum: 0.0, valid: 0, taken: 0 });
        c.iq_auto = Some(a);
        return;
    }
    let (gain, phase) = a.current();
    let irr_now = c.image_irr.map(|(_, rej)| rej.abs() as f64);
    let m = a.meas.as_mut().unwrap();
    let elapsed = m.t0.elapsed().as_millis();
    while m.taken < MEASURE_SAMPLES && elapsed >= MEASURE_WAIT_MS + m.taken as u128 * MEASURE_STEP_MS {
        if let Some(v) = irr_now {
            m.sum += v;
            m.valid += 1;
        }
        m.taken += 1;
    }
    if m.taken >= MEASURE_SAMPLES && elapsed >= MEASURE_WAIT_MS + MEASURE_SAMPLES as u128 * MEASURE_STEP_MS {
        let result = if m.valid > 0 { Some(m.sum / m.valid as f64) } else { None };
        a.meas = None;
        if let Some(irr) = result {
            if irr > a.best.2 {
                a.best = (gain, phase, irr);
            }
        }
        a.idx += 1;
        if a.stage == 0 || a.idx >= a.cands.len() {
            // Next sweep around what is best so far (that value is already in force for the next measurement).
            a.stage += 1;
            if a.stage > SWEEPS.len() {
                iq_auto_finish(c, a);
                return;
            }
            a.build();
        }
    }
    c.iq_auto = Some(a);
}
