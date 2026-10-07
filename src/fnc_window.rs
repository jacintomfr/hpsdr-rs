//! "FNC's": quick jump to any toolbar layer. A compact window (same style and placement as the AGC / Band / Filter popups)
//! lists the eight layers with the functions of their eight boxes; while it is open the bottom toolbar shows
//! FNC(0)..FNC(7) and pressing box k (screen or MIDI) selects layer k and closes the list.
//!
//! Size budget (1024x600): layer column 78 px + 8 cells x 108 px = 942 px of rows (+ window frame ~ 16 px = ~958 px, inside 1024 - 2 x 24 = 976);
//! height CLOSE row 46 + 8 rows x 30 px + 8 gaps x 10 px = ~370 px, anchored 72 px above the bottom edge.

use crate::{toolbar, touch_close_button, ConnectedState};

const ROW_H: f32 = 30.0;
const GAP: f32 = 10.0;
const LAYER_W: f32 = 78.0;
const CELL_W: f32 = 108.0;
const FONT: f32 = 15.0;

/// FNC's pressed: open or close the list. Opening closes the other compact popups that share its place.
pub(crate) fn toggle(c: &mut ConnectedState) {
    if c.fnc_list_open {
        c.fnc_list_open = false;
    } else {
        c.band_window_open = false;
        c.mode_window_open = false;
        c.filter_window_open = false;
        c.agc_window_open = false;
        c.fnc_list_open = !covered(c);
    }
}

/// Box `slot` (0..=7) pressed while the list is open: that layer becomes current and the list closes.
pub(crate) fn pick(c: &mut ConnectedState, slot: usize) {
    if slot < toolbar::LAYERS {
        c.toolbar_layer = slot;
        c.settings_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    c.fnc_list_open = false;
}

/// True while a full-screen window or another popup in the same place is up, so the list must not stay open.
pub(crate) fn covered(c: &ConnectedState) -> bool {
    c.menu_window_open
        || c.rx_window_open
        || c.tx_window_open
        || c.pa_window_open
        || c.toolbar_window_open
        || c.display_window_open
        || c.noise_window_open
        || c.sdr_window_open
        || c.eq_window_open
        || c.vox_window_open
        || c.band_window_open
        || c.mode_window_open
        || c.filter_window_open
        || c.agc_window_open
        || c.meter_window_open
        || c.show_settings_window
        || c.frequency_entry.is_some()
        || c.toolbar_choose.is_some()
}

/// Draws the list; returns true when it should close (CLOSE or Escape).
pub fn fnc_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> bool {
    let mut close_now = false;
    let mut picked = None;
    egui::Window::new("fnc_list")
        .id(egui::Id::new("fnc_list_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -72.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if touch_close_button(ui, 46.0).clicked() {
                    close_now = true;
                }
                ui.label("FNC's: jump to a layer");
            });
            for layer in 0..toolbar::LAYERS {
                let current = layer == connected.toolbar_layer;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(LAYER_W + CELL_W * toolbar::BUTTONS as f32, ROW_H), egui::Sense::click());
                let fill = if current { egui::Color32::from_gray(112) } else { egui::Color32::from_gray(48) };
                let fg = if current { egui::Color32::WHITE } else { egui::Color32::from_gray(190) };
                let p = ui.painter();
                p.rect(rect, 5.0, fill, egui::Stroke::new(1.0, egui::Color32::from_gray(110)), egui::StrokeKind::Inside);
                p.text(
                    egui::pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("FNC({layer})"),
                    egui::FontId::proportional(FONT),
                    egui::Color32::from_rgb(235, 195, 40),
                );
                for b in 0..toolbar::BUTTONS {
                    let f = connected.toolbar_layers[layer][b];
                    let text = if f == toolbar::ToolbarFn::None { "-" } else { f.short_label() };
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + LAYER_W + CELL_W * b as f32, rect.top()),
                        egui::vec2(CELL_W, ROW_H),
                    );
                    let mut size = FONT;
                    let mut galley = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(size), fg);
                    if galley.size().x > CELL_W - 10.0 {
                        size *= (CELL_W - 10.0) / galley.size().x;
                        galley = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(size), fg);
                    }
                    p.with_clip_rect(cell).galley(cell.center() - galley.size() / 2.0, galley, fg);
                }
                if resp.clicked() {
                    picked = Some(layer);
                }
            }
        });
    if let Some(l) = picked {
        pick(connected, l);
    }
    close_now
}
