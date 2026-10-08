//! The "CW" menu (deskHPSDR cw_menu.c, the CW button of the Menu): what used to be Settings -> CW, in the layout of the new menus (a compact
//! overlay anchored above the toolbar, Close top left, a label box in front of every control, -/+ spin buttons).
//! Two pages: **Keyer** (paddle mode, speed, weight, sidetone level and frequency, break-in delay, CW pitch, PC sidetone) and
//! **Messages** (the 5 saved CW texts). Every value is stored where the Settings tab stored it, so nothing changes in the saved configuration.
//!
//! deskHPSDR's menu also has "CW handled in Radio", "CW Break-In", "CW Zero Beat Freq. Corr", "Keys reversed" and "Enforce letter spacing";
//! this program has no state behind them yet, so they are not shown.

use crate::noise_window::choice_combo_h;
use crate::radio::{CW_KEYER_MODE_IAMBIC_A, CW_KEYER_MODE_IAMBIC_B, CW_KEYER_MODE_STRAIGHT};
use crate::{help_button, spin_buttons_full, std_checkbox, touch_close_button, ConnectedState};
use std::sync::atomic::Ordering;

const LABEL_W: f32 = 228.0;
const ROW_H: f32 = 46.0;
const GAP: f32 = 6.0;
/// Distance from the bottom of the screen to the bottom of the window: the same as the FILTER / MODE / BAND / AGC / DSP windows.
const BOTTOM_OFFSET: f32 = 58.0;

const PITCH_HELP: &str = "Audio pitch (Hz) that CWL/CWU centre on: it affects the RX filter, click-to-tune centring and the TX Tune tone.";
const SIDETONE_HELP: &str = "What you hear in your own headphones while sending, independent of the CW Pitch above (which is the RX side).";
const BREAKIN_HELP: &str = "How long the radio holds TX after the last element before dropping back to RX.";
const WEIGHT_HELP: &str = "Dot/dash timing ratio: 50 is the standard 1:3 ratio; higher lengthens dashes and shortens dots, lower the reverse.";
const KEYER_HELP: &str = "These settings are for the radio's own built-in keyer (a paddle wired directly into the radio), not for a paddle connected to this PC. Speed and Weight also set the CW text messages.";
const PC_SIDETONE_HELP: &str = "Also play the sidetone through this PC's own audio output (with the Sidetone Level and Frequency above), in addition to what the radio's own keyer does on its local output. Useful when the radio has no local audio output, or for remote operation.";
const MESSAGES_HELP: &str = "Up to 5 saved messages, sent as real CW from the main window's Send control, at the Speed and Weight set on the Keyer page, through the radio's own transmitter (this program generates the CW carrier directly).";

fn label_box(ui: &mut egui::Ui, text: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::hover());
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(40), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
}

/// One row: label box, a spin button (value box + - / +) and a "!" help.
#[allow(clippy::too_many_arguments)]
fn spin_row(ui: &mut egui::Ui, label: &str, id: &str, help: Option<&str>, v: &mut f64, min: f64, max: f64, step: f64) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_box(ui, label, LABEL_W);
        if spin_buttons_full(ui, id, v, min, max, step, 0, None, 78.0).changed() {
            changed = true;
        }
        if let Some(h) = help {
            help_button(ui, &format!("{id}_help"), h);
        }
    });
    changed
}

/// Returns (close, changed).
pub fn cw_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mut close_now = false;
    let mut changed = false;
    let tab_id = egui::Id::new("cw_window_tab");
    let mut tab: u8 = ui.ctx().data(|d| d.get_temp(tab_id)).unwrap_or(0);

    egui::Window::new("cw_menu")
        .id(egui::Id::new("cw_menu_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // The normal window frame (rounded corners and the theme's shadow), like the FILTER / MODE / BAND windows.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -BOTTOM_OFFSET))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if touch_close_button(ui, ROW_H).clicked() {
                    close_now = true;
                }
                ui.label("CW");
                ui.add_space(12.0);
                for (i, label) in ["Keyer", "Messages"].iter().enumerate() {
                    let active = tab == i as u8;
                    let btn = egui::Button::new(egui::RichText::new(*label).color(if active { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if active { egui::Color32::from_rgb(70, 150, 245) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(110.0, ROW_H))
                        .corner_radius(5.0);
                    if ui.add(btn).clicked() {
                        tab = i as u8;
                    }
                }
            });

            if tab == 0 {
                // Paddle mode.
                ui.horizontal(|ui| {
                    label_box(ui, "Paddle Mode", LABEL_W);
                    let mode = connected.session.cw_keyer.mode.load(Ordering::Relaxed);
                    let items = [("Straight Key", true), ("Iambic Mode A", true), ("Iambic Mode B", true)];
                    let sel = match mode {
                        CW_KEYER_MODE_STRAIGHT => 0,
                        CW_KEYER_MODE_IAMBIC_B => 2,
                        _ => 1,
                    };
                    if let Some(i) = choice_combo_h(ui, "cw_paddle_mode", 190.0, sel, &items, 34.0) {
                        let v = match i {
                            0 => CW_KEYER_MODE_STRAIGHT,
                            2 => CW_KEYER_MODE_IAMBIC_B,
                            _ => CW_KEYER_MODE_IAMBIC_A,
                        };
                        if v != mode {
                            connected.session.cw_keyer.mode.store(v, Ordering::Relaxed);
                            changed = true;
                        }
                    }
                    help_button(ui, "cw_keyer_help", KEYER_HELP);
                });
                // Speed.
                let mut speed = connected.session.cw_keyer.speed_wpm.load(Ordering::Relaxed) as f64;
                if spin_row(ui, "CW Speed (WPM)", "cw_speed", None, &mut speed, 1.0, 60.0, 1.0) {
                    connected.session.cw_keyer.speed_wpm.store(speed.round() as u32, Ordering::Relaxed);
                    changed = true;
                }
                // Weight.
                let mut weight = connected.session.cw_keyer.weight.load(Ordering::Relaxed) as f64;
                if spin_row(ui, "Weight", "cw_weight", Some(WEIGHT_HELP), &mut weight, 0.0, 100.0, 1.0) {
                    connected.session.cw_keyer.weight.store(weight.round() as u32, Ordering::Relaxed);
                    changed = true;
                }
                // Sidetone level (0..127 on both protocols, like deskHPSDR).
                let mut level = connected.session.cw_keyer.sidetone_volume.load(Ordering::Relaxed) as f64;
                if spin_row(ui, "Sidetone Level", "cw_sidetone_level", None, &mut level, 0.0, 127.0, 1.0) {
                    connected.session.cw_keyer.sidetone_volume.store(level.round() as u32, Ordering::Relaxed);
                    changed = true;
                }
                // Sidetone frequency.
                let mut freq = connected.session.cw_keyer.sidetone_freq_hz.load(Ordering::Relaxed) as f64;
                if spin_row(ui, "Sidetone Freq (Hz)", "cw_sidetone_freq", Some(SIDETONE_HELP), &mut freq, 100.0, 1000.0, 10.0) {
                    connected.session.cw_keyer.sidetone_freq_hz.store(freq.round() as u32, Ordering::Relaxed);
                    changed = true;
                }
                // Break-in delay.
                let mut hang = connected.session.cw_keyer.hang_time_ms.load(Ordering::Relaxed) as f64;
                if spin_row(ui, "Break-in Delay (ms)", "cw_hang", Some(BREAKIN_HELP), &mut hang, 0.0, 1000.0, 10.0) {
                    connected.session.cw_keyer.hang_time_ms.store(hang.round() as u32, Ordering::Relaxed);
                    changed = true;
                }
                // CW pitch (the RX side).
                let mut pitch = crate::spectrum::cw_pitch_hz();
                if spin_row(ui, "CW Pitch (Hz)", "cw_pitch", Some(PITCH_HELP), &mut pitch, 300.0, 1000.0, 10.0) {
                    crate::spectrum::set_cw_pitch_hz(pitch.round());
                    changed = true;
                }
                // PC sidetone.
                ui.horizontal(|ui| {
                    label_box(ui, "PC Sidetone", LABEL_W);
                    let mut on = connected.cw_sidetone.enabled.load(Ordering::Relaxed);
                    if std_checkbox(ui, &mut on, "On").changed() {
                        connected.cw_sidetone.enabled.store(on, Ordering::Relaxed);
                        changed = true;
                    }
                    help_button(ui, "cw_pc_sidetone_help", PC_SIDETONE_HELP);
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("CW Text Messages");
                    help_button(ui, "cw_messages_help", MESSAGES_HELP);
                });
                for (i, message) in connected.cw_text_messages.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        label_box(ui, &format!("Message {}", i + 1), 120.0);
                        if ui.add(egui::TextEdit::singleline(message).desired_width(560.0).min_size(egui::vec2(560.0, 36.0))).changed() {
                            changed = true;
                        }
                    });
                }
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    (close_now, changed)
}
