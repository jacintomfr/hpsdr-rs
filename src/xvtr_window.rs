//! The "XVTR" window (deskHPSDR xvtr_menu.c): the transverter slots -- Title, Min / Max / LO frequency (MHz), LO error (Hz), Gain (dB), Disable PA and
//! a Reset per slot. Same layout family as the OC Output window: it stops above the toolbar (which stays visible), square corners, no shadow, boxes
//! with rounded corners; the slots scroll with the touch scroll bar. The long description that headed the old Settings page is behind the round "!".
//!
//! As in deskHPSDR the texts are applied by "Update" (and when the window is closed), not per field: the limits depend on the LO, so entering Min
//! before LO would be clamped. An empty Title marks an unused slot; Reset clears a slot (and leaves it if it is the active transverter). The edits
//! are the same `ConnectedState::xvtr_edit` rows the old Settings -> XVTR tab used. Main receiver only.

use crate::discovery_ui::touch_scroll;
use crate::tx_window::{child, new_row};
use crate::{apply_xvtr_edit, help_button, kiosk_accent_button, std_checkbox, xvtr_edit_row, ConnectedState, Xvtr};

const ROW_H: f32 = 44.0;
const EDIT_H: f32 = 36.0;
/// Column x ranges inside the scroll area: (x0, x1).
const COLS: [(f32, f32); 6] = [(0.0, 170.0), (178.0, 298.0), (306.0, 426.0), (434.0, 554.0), (562.0, 652.0), (660.0, 740.0)];
const PA_X: f32 = 758.0;
const RESET_X: (f32, f32) = (806.0, 906.0);
const HEADS: [&str; 6] = ["Title", "Min Frq(MHz)", "Max Frq(MHz)", "LO Frq(MHz)", "LO Err(Hz)", "Gain (dB)"];

const XVTR_HELP: &str = "Transverters convert this radio's real tunable range (its IF) to some other operating frequency (RF) via an external analog box, e.g. a 10m IF of 28-29.7MHz driving a 2m transverter to cover 144-145.7MHz.\n\nRF = IF + LO Frq + LO Err. Up to about 4.3GHz RF. Main receiver only.\n\nThe texts are applied by Update (and when this window is closed), not one by one: the limits depend on the LO. An empty Title marks an unused slot.";

/// Returns (close, changed).
pub fn xvtr_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let radio_min = connected.device.frequency_min;
    let radio_max = connected.device.frequency_max;
    if connected.xvtr_edit.is_none() {
        connected.xvtr_edit = Some(connected.xvtrs.iter().map(xvtr_edit_row).collect());
    }
    let mut edit = connected.xvtr_edit.take().unwrap_or_default();
    let mut apply_all = false;
    let mut reset_row: Option<usize> = None;
    let pa_column = matches!(connected.device.protocol, 1 | 2);

    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("XVTR")
        .id(egui::Id::new("xvtr_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .fixed_pos(screen.min)
        .constrain_to(screen)
        .fixed_size(egui::vec2(screen.width() - 58.0, win_h))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            let w = screen.width() - 58.0;
            ui.set_width(w);
            ui.set_min_height(win_h);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            // Rounded boxes: the text fields and the buttons.
            {
                let v = ui.visuals_mut();
                for s in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                    s.corner_radius = egui::CornerRadius::same(5);
                }
                // A thin outline so the dark fields stand out, like the other outlined kiosk controls.
                v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(95));
            }
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - XVTR", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            egui::Area::new(egui::Id::new("xvtr_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });

            // Header: the "!" and Update.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, 400.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                help_button(&mut c, "xvtr_help", XVTR_HELP);
                if c.add(egui::Button::new("Update").min_size(egui::vec2(120.0, 44.0))).clicked() {
                    apply_all = true;
                }
            }
            ui.add_space(4.0);

            // Column titles (fixed).
            let font = egui::TextStyle::Body.resolve(ui.style());
            let strong = egui::FontId::proportional(font.size);
            let color = ui.visuals().text_color();
            let r = new_row(ui, w, 26.0);
            for (i, head) in HEADS.iter().enumerate() {
                ui.painter().text(egui::pos2(r.left() + COLS[i].0 + 2.0, r.center().y), egui::Align2::LEFT_CENTER, *head, strong.clone(), color);
            }
            if pa_column {
                ui.painter().text(egui::pos2(r.left() + PA_X - 10.0, r.center().y), egui::Align2::LEFT_CENTER, "Disable PA", strong.clone(), color);
            }
            ui.painter().text(egui::pos2(r.left() + RESET_X.0 + 28.0, r.center().y), egui::Align2::LEFT_CENTER, "Reset", strong.clone(), color);

            let scroll_h = (ui.available_height() - 4.0).max(120.0);
            touch_scroll(ui, "xvtr_rows", Some(scroll_h), false, &mut |ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                let inner_w = ui.available_width();
                for (i, erow) in edit.iter_mut().enumerate() {
                    let row = new_row(ui, inner_w, ROW_H);
                    for (c_idx, (x0, x1)) in COLS.iter().enumerate() {
                        let mut c = child(ui, row, *x0, *x1, false, 0.0);
                        c.add_sized(
                            [x1 - x0, EDIT_H],
                            egui::TextEdit::singleline(&mut erow[c_idx])
                                .font(egui::TextStyle::Monospace)
                                .vertical_align(egui::Align::Center)
                                .char_limit(if c_idx == 0 { 15 } else { 12 }),
                        );
                    }
                    if pa_column {
                        let mut c = child(ui, row, PA_X, PA_X + 38.0, false, 0.0);
                        if let Some(x) = connected.xvtrs.get_mut(i) {
                            if std_checkbox(&mut c, &mut x.disable_pa, "").changed() {
                                changed = true;
                            }
                        }
                    }
                    let mut c = child(ui, row, RESET_X.0, RESET_X.1, false, 0.0);
                    if c.add(egui::Button::new("Reset").min_size(egui::vec2(RESET_X.1 - RESET_X.0, EDIT_H))).clicked() {
                        reset_row = Some(i);
                    }
                }
            });
        });

    if let Some(i) = reset_row {
        if let Some(x) = connected.xvtrs.get_mut(i) {
            if connected.active_xvtr.as_deref() == Some(x.name.as_str()) {
                connected.active_xvtr = None;
            }
            *x = Xvtr::default();
            if let Some(e) = edit.get_mut(i) {
                *e = xvtr_edit_row(x);
            }
            changed = true;
        }
    }
    // Update applies every slot; closing the window applies them too (deskHPSDR saves on Close).
    if apply_all || close_now {
        for (i, erow) in edit.iter_mut().enumerate() {
            if let Some(x) = connected.xvtrs.get_mut(i) {
                let was_active = connected.active_xvtr.as_deref() == Some(x.name.as_str());
                let old_name = x.name.clone();
                apply_xvtr_edit(x, erow, radio_min, radio_max);
                if was_active && x.name != old_name {
                    connected.active_xvtr = None;
                }
                *erow = xvtr_edit_row(x);
            }
        }
        changed = true;
    }
    connected.xvtr_edit = if close_now { None } else { Some(edit) };
    (close_now, changed)
}
