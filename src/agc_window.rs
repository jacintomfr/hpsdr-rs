//! The AGC menu (deskHPSDR agc_menu.c): same in-app window style as the BAND / MODE / FILTER menus (Close top left, compact,
//! stays open until Close): AGC mode, AGC Automatic ("On"), AGC Auto Offset and the Hang Threshold of the Long/Slow modes.

use crate::spectrum::Agc;
use crate::{spin_buttons_full, std_checkbox, touch_close_button, ConnectedState};

const LABEL_W: f32 = 228.0;
const ROW_H: f32 = 46.0;
const GAP: f32 = 6.0;

fn label_box(ui: &mut egui::Ui, text: &str, enabled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(LABEL_W, ROW_H), egui::Sense::hover());
    let col = if enabled { egui::Color32::from_gray(225) } else { egui::Color32::from_gray(110) };
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(40), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), col);
}

/// Returns (close, changed).
pub fn agc_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let params = connected.spectrum.agc_params();
    let agc = params.agc;
    let hang_ok = matches!(agc, Agc::Long | Agc::Slow);
    egui::Window::new("agc_menu")
        .id(egui::Id::new("agc_menu_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // Same placement rule as the filter menu: centred, anchored above the toolbar with a gap.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -58.0))
        .show(ui.ctx(), |ui| {
            let _ = screen;
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if touch_close_button(ui, ROW_H).clicked() {
                    close_now = true;
                }
                ui.label("AGC");
            });
            // Mode.
            ui.horizontal(|ui| {
                label_box(ui, "AGC", true);
                ui.spacing_mut().interact_size.y = ROW_H;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 9.0);
                let items = [("Off", Agc::Off), ("Long", Agc::Long), ("Slow", Agc::Slow), ("Medium", Agc::Medium), ("Fast", Agc::Fast)];
                egui::ComboBox::from_id_salt("agc_mode_combo")
                    .width(170.0)
                    .selected_text(items.iter().find(|(_, a)| *a == agc).map(|(n, _)| *n).unwrap_or("Medium"))
                    .show_ui(ui, |ui| {
                        for (name, a) in items {
                            if ui.add(egui::Button::selectable(agc == a, name).min_size(egui::vec2(170.0, 44.0))).clicked() {
                                connected.spectrum.set_agc(a);
                                changed = true;
                            }
                        }
                    });
            });
            // AGC Automatic.
            ui.horizontal(|ui| {
                label_box(ui, "AGC Automatic RX1", true);
                let mut on = connected.agc_auto;
                if std_checkbox(ui, &mut on, "On").changed() {
                    connected.agc_auto = on;
                    changed = true;
                }
            });
            // AGC Auto Offset.
            ui.horizontal(|ui| {
                label_box(ui, "AGC Auto Offset RX1", true);
                let mut v = connected.agc_auto_offset_db as f64;
                if spin_buttons_full(ui, "agc_win_offset", &mut v, -35.0, -15.0, 1.0, 0, None, 70.0).changed() {
                    connected.agc_auto_offset_db = v.round() as f32;
                    changed = true;
                }
            });
            // Hang Threshold (Long and Slow only).
            ui.horizontal(|ui| {
                label_box(ui, "Hang Threshold", hang_ok);
                ui.add_enabled_ui(hang_ok, |ui| {
                    let mut v = params.agc_hang_threshold as f64;
                    if spin_buttons_full(ui, "agc_win_hang", &mut v, 0.0, 100.0, 1.0, 0, None, 70.0).changed() {
                        connected.spectrum.set_agc_hang_threshold(v.round() as i32);
                        changed = true;
                    }
                });
            });
        });
    (close_now, changed)
}
