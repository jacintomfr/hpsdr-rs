//! The "Noise" window (deskHPSDR noise_menu.c): full-screen, four columns like deskHPSDR's grid: SNB / ANF / Noise Reduction,
//! NR/NR2/ANF position, Noise Blanker, MNF, then the four settings sections (NR, NB, NR4, NNR) chosen with round radio
//! buttons. NR3 (RNNoise) and NR4 (specbleach) are listed but inactive: their engines are not in this build yet.

use crate::spectrum::{self, NoiseBlanker, NoiseReduction};
use crate::{kiosk_accent_button, spin_buttons_full, std_checkbox, ConnectedState};

const ROW_H: f32 = 50.0;
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 160, 255);
const ORANGE: egui::Color32 = egui::Color32::from_rgb(232, 150, 46);

/// A round radio button (circle with a filled dot when selected), as the section selectors of deskHPSDR.
pub(crate) fn touch_radio(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let d = 30.0f32;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, ui.visuals().text_color());
    let total = egui::vec2(d + 10.0 + galley.size().x, d.max(galley.size().y));
    let (rect, resp) = ui.allocate_exact_size(total, if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let c = egui::pos2(rect.left() + d / 2.0, rect.center().y);
    let dim = if enabled { 1.0 } else { 0.4 };
    let line = egui::Color32::from_gray((95.0 + 130.0 * dim) as u8);
    ui.painter().circle(c, d / 2.0 - 1.0, egui::Color32::from_gray(40), egui::Stroke::new(1.5, line));
    if selected {
        ui.painter().circle_filled(c, d / 2.0 - 7.0, if enabled { ORANGE } else { egui::Color32::from_gray(110) });
    }
    let text_col = if enabled { ui.visuals().text_color() } else { egui::Color32::from_gray(110) };
    ui.painter().galley(egui::pos2(rect.left() + d + 10.0, rect.center().y - galley.size().y / 2.0), galley, text_col);
    resp
}

/// A drop-down with big touch items. `items` = (label, enabled). Returns the picked index.
pub(crate) fn choice_combo(ui: &mut egui::Ui, id: &str, width: f32, selected: usize, items: &[(&str, bool)]) -> Option<usize> {
    choice_combo_h(ui, id, width, selected, items, 40.0)
}

/// `choice_combo` with a chosen height of the closed box (34 in the dense TX grid).
pub(crate) fn choice_combo_h(ui: &mut egui::Ui, id: &str, width: f32, selected: usize, items: &[(&str, bool)], height: f32) -> Option<usize> {
    let mut picked = None;
    ui.spacing_mut().interact_size.y = height;
    ui.spacing_mut().button_padding = egui::vec2(12.0, 8.0);
    egui::ComboBox::from_id_salt(id)
        .width(width)
        // Tall enough for the whole list (the default 200 px cut the 6 ports of the Ant window after four).
        .height((items.len() as f32 * 48.0 + 16.0).clamp(200.0, 380.0))
        .selected_text(items.get(selected).map(|i| i.0).unwrap_or(""))
        .show_ui(ui, |ui| {
            for (i, (label, enabled)) in items.iter().enumerate() {
                let r = ui.add_enabled(*enabled, egui::Button::selectable(selected == i, *label).min_size(egui::vec2(width, 44.0)));
                if r.clicked() {
                    picked = Some(i);
                }
            }
        });
    picked
}

/// One cell of the 4-column grid of a row.
fn cell(ui: &mut egui::Ui, row: egui::Rect, col: usize, span: usize, align: egui::Align) -> egui::Ui {
    let cw = row.width() / 4.0;
    let r = egui::Rect::from_min_size(egui::pos2(row.left() + cw * col as f32, row.top()), egui::vec2(cw * span as f32 - 6.0, row.height()));
    let layout = if align == egui::Align::Max {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.new_child(egui::UiBuilder::new().max_rect(r).layout(layout))
}

fn label_cell(ui: &mut egui::Ui, row: egui::Rect, col: usize, span: usize, text: &str) {
    // Painted at the vertical centre of the row so it lines up with the controls in the other cells.
    let cw = row.width() / 4.0;
    let right = row.left() + cw * (col + span) as f32 - 12.0;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    ui.painter().text(egui::pos2(right, row.center().y), egui::Align2::RIGHT_CENTER, text, font, color);
}

/// Spin cell: -  value  + with the box filling the cell.
fn spin_cell(ui: &mut egui::Ui, row: egui::Rect, col: usize, id: &str, value: &mut f64, min: f64, max: f64, step: f64, decimals: usize) -> bool {
    // The spin's own row sits a little low in its cell: lift it so it lines up with the label and the other cells.
    let mut c = cell(ui, row.translate(egui::vec2(0.0, -6.0)), col, 1, egui::Align::Min);
    let box_w = (row.width() / 4.0 - 6.0 - 98.0).max(50.0);
    spin_buttons_full(&mut c, id, value, min, max, step, decimals, None, box_w).changed()
}

/// Returns (close, changed).
pub fn noise_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let section_id = egui::Id::new("noise_window_section");
    let mut section: u8 = ui.ctx().data(|d| d.get_temp(section_id)).unwrap_or(0);

    let mode = connected.spectrum.mode();
    let digital = matches!(mode, spectrum::Mode::Digl | spectrum::Mode::Digu);
    let params = connected.spectrum.agc_params();
    let mut ex = params.noise_extra;
    let mut ex_dirty = false;
    let mut nr = connected.spectrum.noise_reduction();
    let mut nb = connected.spectrum.noise_blanker();
    let mut snb = connected.spectrum.snb();
    let mut anf = connected.spectrum.anf();

    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
    egui::Window::new("Noise")
        .id(egui::Id::new("noise_window"))
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
            let avail_w = screen.width() - 58.0;
            ui.set_width(avail_w);
            ui.set_min_height(screen.height() - 10.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let title = format!("hpsdr-rs - Noise [{}]", match nr {
                NoiseReduction::Off => "NONE",
                NoiseReduction::Nr => "NR",
                NoiseReduction::Nr2 => "NR2",
                NoiseReduction::Nr3 => "NNR",
            });
            let (tr, _) = ui.allocate_exact_size(egui::vec2(avail_w, 24.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            let new_row = |ui: &mut egui::Ui, h: f32| ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::hover()).0;

            // Row 0: SNB, ANF, Noise Reduction.
            let row = new_row(ui, ROW_H);
            {
                let mut c = cell(ui, row, 0, 1, egui::Align::Min);
                c.add_space(10.0);
                c.add_enabled_ui(!digital, |ui| {
                    if std_checkbox(ui, &mut snb, "SNB").on_hover_text("Spectral Noise Blanker").changed() {
                        connected.spectrum.set_snb(snb);
                        changed = true;
                    }
                });
                let mut c = cell(ui, row, 1, 1, egui::Align::Min);
                c.add_space(10.0);
                let anf_allowed = !matches!(mode, spectrum::Mode::Cwl | spectrum::Mode::Cwu | spectrum::Mode::Digl | spectrum::Mode::Digu);
                c.add_enabled_ui(anf_allowed, |ui| {
                    if std_checkbox(ui, &mut anf, "ANF").on_hover_text("Auto Notch Filter").changed() {
                        connected.spectrum.set_anf(anf);
                        changed = true;
                    }
                });
                label_cell(ui, row, 2, 1, "Noise Reduction");
                let mut c = cell(ui, row, 3, 1, egui::Align::Min);
                c.add_enabled_ui(!digital, |ui| {
                    let items = [("NONE", true), ("NR", true), ("NR2", true), ("NR3", false), ("NR4", false), ("NNR", true)];
                    let sel = match nr {
                        NoiseReduction::Off => 0,
                        NoiseReduction::Nr => 1,
                        NoiseReduction::Nr2 => 2,
                        NoiseReduction::Nr3 => 5,
                    };
                    if let Some(i) = choice_combo(ui, "noise_nr", row.width() / 4.0 - 20.0, sel, &items) {
                        nr = match i {
                            1 => NoiseReduction::Nr,
                            2 => NoiseReduction::Nr2,
                            5 => NoiseReduction::Nr3,
                            _ => NoiseReduction::Off,
                        };
                        connected.spectrum.set_noise_reduction(nr);
                        changed = true;
                    }
                });
            }
            // Row 1: position, noise blanker.
            let row = new_row(ui, ROW_H);
            {
                label_cell(ui, row, 0, 1, "NR/NR2/ANF Position");
                let mut c = cell(ui, row, 1, 1, egui::Align::Min);
                c.add_enabled_ui(!digital, |ui| {
                    let items = [("Pre AGC", true), ("Post AGC", true)];
                    if let Some(i) = choice_combo(ui, "noise_pos", row.width() / 4.0 - 20.0, ex.nr_pos_post as usize, &items) {
                        ex.nr_pos_post = i == 1;
                        ex_dirty = true;
                    }
                });
                label_cell(ui, row, 2, 1, "Noise Blanker");
                let mut c = cell(ui, row, 3, 1, egui::Align::Min);
                let items = [("NONE", true), ("NB", true), ("NB2", true)];
                let sel = match nb {
                    NoiseBlanker::Off => 0,
                    NoiseBlanker::Nb => 1,
                    NoiseBlanker::Nb2 => 2,
                };
                if let Some(i) = choice_combo(&mut c, "noise_nb", row.width() / 4.0 - 20.0, sel, &items) {
                    nb = match i {
                        1 => NoiseBlanker::Nb,
                        2 => NoiseBlanker::Nb2,
                        _ => NoiseBlanker::Off,
                    };
                    connected.spectrum.set_noise_blanker(nb);
                    changed = true;
                }
            }
            // Row 2: MNF and its bandwidth.
            let row = new_row(ui, ROW_H);
            {
                let mut c = cell(ui, row, 0, 1, egui::Align::Min);
                c.add_space(10.0);
                if std_checkbox(&mut c, &mut ex.mnf_enabled, "MNF").on_hover_text("Enable Manual Notch Filter").changed() {
                    if ex.mnf_enabled && ex.mnf_cfreq_hz <= 0.0 {
                        // Like deskHPSDR's MNF action: the notch starts 1 kHz above the VFO.
                        ex.mnf_cfreq_hz = (if connected.ctun { connected.ctun_frequency_hz } else { connected.session.frequency_hz.load(std::sync::atomic::Ordering::Relaxed) }) as f64 + 1000.0;
                    }
                    ex_dirty = true;
                }
                let mut w = ex.mnf_fbw_hz;
                if spin_cell(ui, row, 1, "noise_mnf_fbw", &mut w, 10.0, 15000.0, 10.0, 0) {
                    ex.mnf_fbw_hz = w;
                    ex_dirty = true;
                }
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);

            // Section selector: four round radio buttons.
            let row = new_row(ui, ROW_H - 6.0);
            if digital && section != 1 {
                section = 1;
            }
            for (i, label) in ["NR Settings", "NB Settings", "NR4 Settings", "NNR Settings"].iter().enumerate() {
                let mut c = cell(ui, row, i, 1, egui::Align::Min);
                c.add_space(10.0);
                let usable = !digital || i == 1;
                c.add_enabled_ui(usable, |ui| {
                    if touch_radio(ui, section == i as u8, label).clicked() {
                        section = i as u8;
                    }
                });
            }
            ui.add_space(6.0);

            match section {
                0 => {
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NR2 Gain Method");
                    let mut c = cell(ui, row, 1, 1, egui::Align::Min);
                    let items = [("Linear", true), ("Log", true), ("Gamma", true), ("Trained", true)];
                    if let Some(i) = choice_combo(&mut c, "noise_gain", row.width() / 4.0 - 20.0, ex.nr2_gain_method as usize, &items) {
                        ex.nr2_gain_method = i as u8;
                        ex_dirty = true;
                    }
                    label_cell(ui, row, 2, 1, "NR2 NPE Method");
                    let mut c = cell(ui, row, 3, 1, egui::Align::Min);
                    let items = [("OSMS", true), ("MMSE", true), ("NSTAT", true)];
                    if let Some(i) = choice_combo(&mut c, "noise_npe", row.width() / 4.0 - 20.0, ex.nr2_npe_method as usize, &items) {
                        ex.nr2_npe_method = i as u8;
                        ex_dirty = true;
                    }
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NR2 Trained Thresh");
                    let mut v = ex.nr2_trained_threshold;
                    if spin_cell(ui, row, 1, "noise_tthr", &mut v, -5.0, 5.0, 0.1, 1) {
                        ex.nr2_trained_threshold = (v * 10.0).round() / 10.0;
                        ex_dirty = true;
                    }
                    label_cell(ui, row, 2, 1, "NR2 Trained T2");
                    let mut v = ex.nr2_trained_t2;
                    if spin_cell(ui, row, 3, "noise_tt2", &mut v, 0.02, 0.3, 0.01, 2) {
                        ex.nr2_trained_t2 = (v * 100.0).round() / 100.0;
                        ex_dirty = true;
                    }
                    let row = new_row(ui, ROW_H);
                    {
                        let mut c = cell(ui, row, 0, 2, egui::Align::Min);
                        c.add_space(10.0);
                        if std_checkbox(&mut c, &mut ex.nr2_post, "NR2 Post-Processing").changed() {
                            ex_dirty = true;
                        }
                        let mut c = cell(ui, row, 2, 2, egui::Align::Min);
                        c.add_space(10.0);
                        if std_checkbox(&mut c, &mut ex.nr2_ae, "NR2 Artifact Elimination").changed() {
                            ex_dirty = true;
                        }
                    }
                    let mut int_row = |ui: &mut egui::Ui, l0: &str, id0: &str, v0: &mut i32, l1: &str, id1: &str, v1: &mut i32| {
                        let row = new_row(ui, ROW_H);
                        label_cell(ui, row, 0, 1, l0);
                        let mut f = *v0 as f64;
                        if spin_cell(ui, row, 1, id0, &mut f, 0.0, 100.0, 1.0, 0) {
                            *v0 = f.round() as i32;
                            ex_dirty = true;
                        }
                        label_cell(ui, row, 2, 1, l1);
                        let mut f = *v1 as f64;
                        if spin_cell(ui, row, 3, id1, &mut f, 0.0, 100.0, 1.0, 0) {
                            *v1 = f.round() as i32;
                            ex_dirty = true;
                        }
                    };
                    let (mut pl, mut pf, mut pr, mut pt) = (ex.nr2_post_nlevel, ex.nr2_post_factor, ex.nr2_post_rate, ex.nr2_post_taper);
                    int_row(ui, "NR2 Post Level", "noise_pl", &mut pl, "NR2 Post Factor", "noise_pf", &mut pf);
                    int_row(ui, "NR2 Post Rate", "noise_pr", &mut pr, "NR2 Post Taper", "noise_pt", &mut pt);
                    ex.nr2_post_nlevel = pl;
                    ex.nr2_post_factor = pf;
                    ex.nr2_post_rate = pr;
                    ex.nr2_post_taper = pt;
                }
                1 => {
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NB2 mode");
                    let mut c = cell(ui, row, 1, 1, egui::Align::Min);
                    let items = [("Zero", true), ("Sample&Hold", true), ("Mean Hold", true), ("Hold Sample", true), ("Interpolate", true)];
                    if let Some(i) = choice_combo(&mut c, "noise_nb2mode", row.width() / 4.0 - 20.0, ex.nb2_mode as usize, &items) {
                        ex.nb2_mode = i as u8;
                        ex_dirty = true;
                    }
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NB Slew time (ms)");
                    let mut v = ex.nb_tau_ms;
                    if spin_cell(ui, row, 1, "noise_slew", &mut v, 0.0, 0.1, 0.001, 3) {
                        ex.nb_tau_ms = v;
                        ex_dirty = true;
                    }
                    label_cell(ui, row, 2, 1, "NB Lead time (ms)");
                    let mut v = ex.nb_advtime_ms;
                    if spin_cell(ui, row, 3, "noise_lead", &mut v, 0.0, 0.1, 0.001, 3) {
                        ex.nb_advtime_ms = v;
                        ex_dirty = true;
                    }
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NB Lag time (ms)");
                    let mut v = ex.nb_hang_ms;
                    if spin_cell(ui, row, 1, "noise_lag", &mut v, 0.0, 0.1, 0.001, 3) {
                        ex.nb_hang_ms = v;
                        ex_dirty = true;
                    }
                    label_cell(ui, row, 2, 1, "NB Threshold");
                    // deskHPSDR shows the threshold as value / 0.165 (its internal value is 0.165 x the shown one).
                    let mut v = (params.nb_threshold / 0.165).round();
                    if spin_cell(ui, row, 3, "noise_nbthr", &mut v, 15.0, 500.0, 1.0, 0) {
                        connected.spectrum.set_nb_threshold(v * 0.165);
                        changed = true;
                    }
                }
                2 => {
                    // NR4: the engine (libspecbleach) is not in this build yet: the section is shown for the future, inactive.
                    let row = new_row(ui, ROW_H);
                    c_note(ui, row);
                    ui.add_enabled_ui(false, |ui| {
                        let row = new_row(ui, ROW_H);
                        label_cell(ui, row, 0, 1, "NR4 Reduction (dB)");
                        let mut v = ex.nr4_reduction;
                        spin_cell(ui, row, 1, "noise_n4r", &mut v, 0.0, 20.0, 1.0, 0);
                        label_cell(ui, row, 2, 1, "NR4 Smoothing (%)");
                        let mut v = ex.nr4_smoothing;
                        spin_cell(ui, row, 3, "noise_n4s", &mut v, 0.0, 100.0, 1.0, 0);
                        let row = new_row(ui, ROW_H);
                        label_cell(ui, row, 0, 1, "NR4 Whitening (%)");
                        let mut v = ex.nr4_whitening;
                        spin_cell(ui, row, 1, "noise_n4w", &mut v, 0.0, 100.0, 1.0, 0);
                        label_cell(ui, row, 2, 1, "NR4 rescale (dB)");
                        let mut v = ex.nr4_rescale;
                        spin_cell(ui, row, 3, "noise_n4c", &mut v, 0.0, 12.0, 0.1, 1);
                        let row = new_row(ui, ROW_H);
                        label_cell(ui, row, 0, 3, "NR4 post filter threshold (dB)");
                        let mut v = ex.nr4_post_threshold;
                        spin_cell(ui, row, 3, "noise_n4t", &mut v, -10.0, 10.0, 0.1, 1);
                    });
                }
                _ => {
                    let row = new_row(ui, ROW_H);
                    label_cell(ui, row, 0, 1, "NNR Model");
                    let mut c = cell(ui, row, 1, 1, egui::Align::Min);
                    let items = [("Standard", true), ("Premium", true)];
                    if let Some(i) = choice_combo(&mut c, "noise_nnrmodel", row.width() / 4.0 - 20.0, params.nnr_premium as usize, &items) {
                        connected.spectrum.set_nnr_premium(i == 1);
                        changed = true;
                    }
                    label_cell(ui, row, 2, 1, "NNR Mask Floor (dB)");
                    let mut v = params.nnr_mask_floor_db;
                    if spin_cell(ui, row, 3, "noise_nnrfloor", &mut v, -50.0, -10.0, 1.0, 0) {
                        connected.spectrum.set_nnr_mask_floor_db(v);
                        changed = true;
                    }
                    let row = new_row(ui, ROW_H);
                    let mut c = cell(ui, row, 3, 1, egui::Align::Min);
                    let size = egui::vec2(row.width() / 4.0 - 20.0, 40.0);
                    if c.add(egui::Button::new("Defaults").min_size(size).corner_radius(5.0)).clicked() {
                        connected.spectrum.set_nnr_premium(false);
                        connected.spectrum.set_nnr_mask_floor_db(-25.0);
                        changed = true;
                    }
                }
            }

            egui::Area::new(egui::Id::new("noise_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
        });
    ui.ctx().data_mut(|d| d.insert_temp(section_id, section));
    if ex_dirty {
        connected.spectrum.set_noise_extra(ex);
        changed = true;
    }
    (close_now, changed)
}

fn c_note(ui: &mut egui::Ui, row: egui::Rect) {
    let mut c = cell(ui, row, 0, 4, egui::Align::Min);
    c.add_space(10.0);
    c.colored_label(BLUE, "NR4 (specbleach) is not available in this build yet; these settings are shown for later.");
}
