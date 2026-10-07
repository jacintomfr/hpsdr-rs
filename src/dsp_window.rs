//! The "DSP" menu (deskHPSDR fft_menu.c, the DSP button of the Menu): same in-app window style as the AGC / BAND / MODE /
//! FILTER menus (compact, anchored above the toolbar, Close top left, stays open until Close), one column per channel (RX1, TX).
//! Phase 1: WDSP FIR filter type (linear phase / low latency), FIR size NC (2048..16384) and Binaural (RX only).
//! The settings live in `RxExtra` / `TxExtra` (already saved with the configuration) and are sent to WDSP by the DSP
//! threads when they change (spectrum.rs `last_fir`, tx.rs `last_fir`).

use crate::noise_window::choice_combo_h;
use crate::{help_button, std_checkbox, touch_close_button, ConnectedState};

const LABEL_W: f32 = 228.0;
const COL_W: f32 = 172.0;
const ROW_H: f32 = 46.0;
const GAP: f32 = 6.0;
const NC_VALUES: [i32; 4] = [2048, 4096, 8192, 16384];
const TYPE_HELP: &str = "TX Low Latency sets CESSB option to DISABLED.\n\nThese two functions cannot be used simultaneously. Setting Linear Phase is REQUIRED when using CESSB.\n\nNote: RX is not affected. Selection is unrestricted.";
const NC_HELP: &str = "Sets the number of coefficients (NC) used by the WDSP FIR filters.\n\nHigher values provide steeper filter skirts at the cost of increased CPU load.\n\nDefault setting: 2048 (change only for a specific reason).";
const BIN_HELP: &str = "Outputs I and Q on the Left and Right audio channels.\n\nIf the audio output device is mono or NNR is active, the Binaural option is not available or switched off.";

fn label_box(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(LABEL_W, ROW_H), egui::Sense::hover());
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(40), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
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
            });
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
        });

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
    if binaural != binaural0 {
        connected.spectrum.set_binaural(binaural);
        changed = true;
    }
    (close_now, changed)
}
