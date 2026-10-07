//! The "PA" window (deskHPSDR pa_menu.c, "Calibrate" page): full-screen, MAX Power, Transmit out of band, Save / Import
//! (deskHPSDR .props files) and the per-band PA calibration (dB) as spin buttons. The values are the same
//! `ConnectedState::pa_calibration` entries the old Settings -> PA Calibration tab edits (by band name).

use crate::noise_window::choice_combo_h;
use crate::tx_window::{child, new_row, spin};
use crate::{help_button, kiosk_accent_button, touch_checkbox, ConnectedState};
use std::sync::atomic::Ordering;

const ROW_H: f32 = 44.0;
/// deskHPSDR band.c clamps every calibration to this range.
pub(crate) const PA_MIN: f32 = 38.8;
pub(crate) const PA_MAX: f32 = 70.0;
const MAX_POWER_LIST: [u32; 14] = [1, 5, 10, 15, 20, 25, 30, 50, 75, 100, 125, 200, 500, 1000];
/// deskHPSDR's band enum: index of the first transverter slot (BANDS) and of "Gen".
const XVTR_BASE: usize = 24;
const GEN_INDEX: usize = 23;
/// (name used as key in `pa_calibration`, deskHPSDR band index). The first 12 are what a Hermes Lite reaches (radio_max_band).
const BAND_TABLE: [(&str, usize); 14] = [
    ("136kHz", 0),
    ("472kHz", 1),
    ("160m", 2),
    ("80m", 3),
    ("60m", 4),
    ("40m", 5),
    ("30m", 6),
    ("20m", 7),
    ("17m", 8),
    ("15m", 9),
    ("12m", 10),
    ("10m", 11),
    ("8m", 12),
    ("6m", 13),
];

const HL2_HELP: &str = "Hermes Lite 2 or compatible SDR devices:\n1. Set all bands to a value of 38.8 and MAX Power to 5W for full 5W output!\n2. Set PA enable in SDR Device!";
const OOB_HELP: &str = "Off (default): TX is blocked outside the defined ham band allocations, e.g. on \"Gen\". Enable only for MARS/CAP or other explicitly authorized out-of-band operation -- this does not check any regulatory database, it only removes this app's own safety check.";

pub(crate) fn clamp_db(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(PA_MIN, PA_MAX)
    } else {
        PA_MIN
    }
}

/// Every stored calibration clamped to deskHPSDR's range (used when the config is loaded).
pub(crate) fn clamped(map: &std::collections::HashMap<String, f32>) -> std::collections::HashMap<String, f32> {
    map.iter().map(|(k, v)| (k.clone(), clamp_db(*v))).collect()
}

fn is_hl2(connected: &ConnectedState) -> bool {
    matches!(connected.device.board, crate::Boards::HermesLite | crate::Boards::HermesLite2)
}

/// The bands shown, in deskHPSDR order: (key, title, deskHPSDR index).
fn visible_bands(connected: &ConnectedState, with_gen: bool) -> Vec<(String, String, usize)> {
    let max = if is_hl2(connected) { 12 } else { BAND_TABLE.len() };
    let mut v: Vec<(String, String, usize)> = Vec::new();
    if with_gen {
        v.push(("Gen".to_string(), "Gen".to_string(), GEN_INDEX));
    }
    for (name, idx) in BAND_TABLE.iter().take(max) {
        v.push((name.to_string(), name.to_string(), *idx));
    }
    for (i, x) in connected.xvtrs.iter().enumerate() {
        if !x.name.is_empty() {
            v.push((x.name.clone(), x.name.clone(), XVTR_BASE + i));
        }
    }
    v
}

/// Every band that has a title (what deskHPSDR saves/loads): the table, Gen and the named XVTR slots.
fn all_bands(connected: &ConnectedState) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = BAND_TABLE.iter().map(|(n, i)| (n.to_string(), *i)).collect();
    v.push(("Gen".to_string(), GEN_INDEX));
    for (i, x) in connected.xvtrs.iter().enumerate() {
        if !x.name.is_empty() {
            v.push((x.name.clone(), XVTR_BASE + i));
        }
    }
    v
}

fn value_of(connected: &ConnectedState, key: &str) -> f32 {
    clamp_db(connected.pa_calibration.get(key).copied().unwrap_or(crate::radio::DEFAULT_PA_GAIN_DB))
}

fn props_name(connected: &ConnectedState) -> String {
    let name: String = connected.device.board_label().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!("pa_calibration_{name}.props")
}

fn save(connected: &ConnectedState) -> bool {
    let Some(dir) = crate::config::settings_dir() else { return false };
    let mut text = String::new();
    for (key, idx) in all_bands(connected) {
        text.push_str(&format!("band.{}.pa_calibration={:.1}\n", idx, value_of(connected, &key)));
    }
    std::fs::write(dir.join(props_name(connected)), text).is_ok()
}

/// Reads a deskHPSDR pa_calibration .props file; unknown keys are ignored, values clamped. True when the file could be read.
fn import(connected: &mut ConnectedState, path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let bands = all_bands(connected);
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let Some(idx) = k.trim().strip_prefix("band.").and_then(|r| r.strip_suffix(".pa_calibration")).and_then(|n| n.parse::<usize>().ok()) else {
            continue;
        };
        let Ok(val) = v.trim().parse::<f32>() else { continue };
        if let Some((key, _)) = bands.iter().find(|(_, i)| *i == idx) {
            connected.pa_calibration.insert(key.clone(), clamp_db(val));
        }
    }
    true
}

fn list_props() -> Vec<std::path::PathBuf> {
    let Some(dir) = crate::config::settings_dir() else { return Vec::new() };
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map(|e| e == "props").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Returns (close, changed).
pub fn pa_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let popup_id = egui::Id::new("pa_window_popup");
    let msg_id = egui::Id::new("pa_window_msg");

    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
    egui::Window::new("PA Calibration")
        .id(egui::Id::new("pa_window"))
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
            let w = screen.width() - 58.0;
            ui.set_width(w);
            ui.set_min_height(screen.height() - 10.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - PA Calibration", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

            // Header row; CLOSE (96 px wide, 24 px from the screen edge) takes the right end, so the controls stop at 866.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                c.label("MAX Power");
                let sel = MAX_POWER_LIST.iter().position(|v| *v == connected.max_tx_power_watts);
                let mut labels: Vec<String> = MAX_POWER_LIST.iter().map(|v| if *v == 1000 { "1KW".to_string() } else { format!("{v}W") }).collect();
                if sel.is_none() {
                    // A value from the old slider that is not in the list: shown as it is, until another entry is picked.
                    labels.push(format!("{}W", connected.max_tx_power_watts));
                }
                let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
                if let Some(i) = choice_combo_h(&mut c, "pa_max_power", 90.0, sel.unwrap_or(MAX_POWER_LIST.len()), &items, 40.0) {
                    if let Some(v) = MAX_POWER_LIST.get(i) {
                        connected.max_tx_power_watts = *v;
                        let capped = connected.session.tx_power_watts.load(Ordering::Relaxed).min(*v);
                        connected.session.tx_power_watts.store(capped, Ordering::Relaxed);
                        changed = true;
                    }
                }
                let mut oob = connected.allow_out_of_band_tx.load(Ordering::Relaxed);
                if touch_checkbox(&mut c, &mut oob, "Transmit out of band").changed() {
                    connected.allow_out_of_band_tx.store(oob, Ordering::Relaxed);
                    changed = true;
                }
                help_button(&mut c, "pa_oob", OOB_HELP);
                if c.add(egui::Button::new(egui::RichText::new("Save").strong()).min_size(egui::vec2(90.0, 40.0))).clicked() {
                    let ok = save(connected);
                    c.ctx().data_mut(|d| d.insert_temp(msg_id, if ok { "PA calibration saved" } else { "ERROR: PA calibration not saved" }.to_string()));
                }
                if c.add(egui::Button::new(egui::RichText::new("Import").strong()).min_size(egui::vec2(110.0, 40.0))).clicked() {
                    c.ctx().data_mut(|d| d.insert_temp(popup_id, true));
                }
            }
            egui::Area::new(egui::Id::new("pa_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(4.0);

            // Page title ("Calibrate") with the Hermes Lite hint behind a "!" button.
            let row = new_row(ui, w, ROW_H);
            {
                let mut c = child(ui, row, 0.0, 400.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                c.label(egui::RichText::new("Calibrate").strong());
                if is_hl2(connected) && !connected.device.is_radioberry {
                    help_button(&mut c, "pa_hl2", HL2_HELP);
                }
            }

            // The bands: column pairs (label + spin), filled top to bottom like deskHPSDR.
            let with_gen = connected.allow_out_of_band_tx.load(Ordering::Relaxed);
            let bands = visible_bands(connected, with_gen);
            let n = bands.len();
            let cols = if n <= 12 { 2 } else { 3 };
            let rows = if n <= 12 { 6 } else { n.div_ceil(cols) };
            let pitch = w / cols as f32;
            for r in 0..rows {
                let row = new_row(ui, w, ROW_H);
                for col in 0..cols {
                    let Some((key, title, _)) = bands.get(col * rows + r) else { continue };
                    let x0 = pitch * col as f32;
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let color = ui.visuals().text_color();
                    ui.painter().text(egui::pos2(row.left() + x0 + 4.0, row.center().y), egui::Align2::LEFT_CENTER, title, font, color);
                    let mut v = value_of(connected, key) as f64;
                    let sx = x0 + 96.0;
                    if spin(ui, row, sx, sx + 98.0 + 110.0, &format!("pa_cal_{key}"), &mut v, PA_MIN as f64, PA_MAX as f64, 0.1, 1) {
                        connected.pa_calibration.insert(key.clone(), clamp_db(((v * 10.0).round() / 10.0) as f32));
                        changed = true;
                    }
                }
            }
        });

    // Import: in-app file list (no native dialog on the touch panel).
    if ui.ctx().data(|d| d.get_temp::<bool>(popup_id)).unwrap_or(false) {
        let mut close = false;
        egui::Window::new("pa_import_files")
            .id(egui::Id::new("pa_import_files"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(520.0);
                ui.label(egui::RichText::new("Import deskHPSDR PA Calibration").strong());
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for path in list_props() {
                        let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::Button::new(&fname).min_size(egui::vec2(500.0, 40.0))).clicked() {
                            let ok = import(connected, &path);
                            if ok {
                                changed = true;
                            }
                            ui.ctx().data_mut(|d| d.insert_temp(msg_id, if ok { "PA calibration loaded" } else { "ERROR: PA calibration not loaded" }.to_string()));
                            close = true;
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(110.0, 40.0))).clicked() {
                    close = true;
                }
            });
        if close {
            ui.ctx().data_mut(|d| d.insert_temp(popup_id, false));
        }
    }

    // Confirmation message.
    if let Some(msg) = ui.ctx().data(|d| d.get_temp::<String>(msg_id)) {
        let mut ok = false;
        egui::Window::new("pa_message")
            .id(egui::Id::new("pa_message"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(300.0);
                ui.label(&msg);
                ui.add_space(8.0);
                if ui.add(egui::Button::new("OK").min_size(egui::vec2(110.0, 40.0))).clicked() {
                    ok = true;
                }
            });
        if ok {
            ui.ctx().data_mut(|d| d.remove::<String>(msg_id));
        }
    }
    (close_now, changed)
}
