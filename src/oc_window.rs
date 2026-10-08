//! The "OC Output" window (deskHPSDR oc_menu.c): the per-band Open Collector Rx/Tx outputs (OC1-OC7), the Tune row (ORed into TX while TUNE is
//! on) and deskHPSDR's Full Tune / Memory Tune times. Same layout family as the PA window: it stops above the toolbar (which stays visible),
//! square corners, no shadow. The band rows scroll (touch scroll bar); the Tune column stays in view. The long description that used to head
//! the old Settings page is behind the round "!".
//!
//! Values are the same `ConnectedState::oc_settings` / `oc_tune` the old Settings -> Open Collector page edited. New (deskHPSDR / piHPSDR
//! radio.c, old_protocol.c, new_protocol.c): `OCfull_tune_time` / `OCmemory_tune_time` -- how long the Tune outputs stay on after a TUNE that was
//! armed as "Full" or "Memory" (an ATU driven by OC lines). 0 / 0 (the default here) keeps the previous behaviour: Tune outputs on for the whole
//! TUNE.

use crate::discovery_ui::touch_scroll;
use crate::tx_window::{child, new_row, spin};
use crate::{help_button, kiosk_accent_button, touch_checkbox_sized, ConnectedState, OcMask};
use std::sync::atomic::Ordering;

const BOX: f32 = 28.0;
/// One checkbox cell: the box plus the 10 px `touch_checkbox_sized` leaves after it.
const PITCH: f32 = 38.0;
const ROW_H: f32 = 38.0;
const LABEL_W: f32 = 72.0;
const RX_X: f32 = LABEL_W + 4.0;
const TX_X: f32 = RX_X + 7.0 * PITCH + 16.0;
const GRID_W: f32 = TX_X + 7.0 * PITCH;
/// Width of the left block: grid + the 30 px scroll bar and its gap.
const LEFT_W: f32 = GRID_W + 36.0 + 4.0;

const OC_HELP: &str = "Open Collector outputs (OC1-OC7) are general-purpose relay driver lines, configured per band -- e.g. for external antenna switching, bandpass filter selection, or amp keying.\n\nRx is active while receiving on that band, Tx while transmitting. Driven by the primary front end's band, shared across every receiver -- not a per-extra-receiver setting.\n\nThe Tune column (its own set of outputs, not tied to any band) is ORed into the current band's Tx outputs while the Tune button is engaged.";
const TUNE_HELP: &str = "Full Tune / Memory Tune (deskHPSDR, for an antenna tuner driven by the OC lines): how long, in ms, the Tune outputs stay on after a TUNE that was armed as Full (first tune on a band) or Memory (re-tune from the tuner's memory). Arm one of them below, or with the Tune Full / Tune Mem toolbar or MIDI actions; arming one disarms the other.\n\nAs in deskHPSDR, with Memory Tune not 0 the Tune outputs are only on during an armed TUNE and only for its time. 0 / 0 (default): the Tune outputs stay on for the whole TUNE.";

/// The rows: reachable bands, then "Gen", then the configured XVTRs (the same list as the old Settings page).
fn band_names(connected: &ConnectedState) -> Vec<String> {
    crate::BANDS
        .iter()
        .filter(|band| (band.low() as u64) >= connected.device.frequency_min && (band.high() as u64) <= connected.device.frequency_max)
        .map(|band| band.name.to_string())
        .chain(std::iter::once("Gen".to_string()))
        .chain(connected.xvtrs.iter().filter(|x| !x.name.is_empty()).map(|x| x.name.clone()))
        .collect()
}

/// Returns (close, changed).
pub fn oc_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;

    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("OC Output")
        .id(egui::Id::new("oc_window"))
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
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - Open Collector Outputs", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            egui::Area::new(egui::Id::new("oc_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });

            // Header: the "!" (what these outputs are) and the outputs being sent right now (deskHPSDR's "OC monitor").
            let row = new_row(ui, w, 46.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                help_button(&mut c, "oc_help", OC_HELP);
                let transmitting = connected.session.mox_active();
                let bits = if transmitting { connected.session.oc_tx.load(Ordering::Relaxed) } else { connected.session.oc_rx.load(Ordering::Relaxed) };
                c.label(if transmitting { "Sending now (Tx):" } else { "Sending now (Rx):" });
                for i in 0..7u8 {
                    let on = bits & (1 << i) != 0;
                    let (r, _) = c.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    let fill = if on { egui::Color32::from_rgb(232, 150, 46) } else { egui::Color32::from_gray(40) };
                    c.painter().circle(r.center(), 13.0, fill, egui::Stroke::new(1.5, egui::Color32::from_gray(150)));
                    let txt = if on { egui::Color32::BLACK } else { egui::Color32::from_gray(190) };
                    c.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(14.0), txt);
                }
            }
            ui.add_space(4.0);

            let avail = ui.available_height();
            let (body, _) = ui.allocate_exact_size(egui::vec2(w, avail), egui::Sense::hover());

            // ---- Left: column titles (fixed) + the band rows (scroll).
            let left_rect = egui::Rect::from_min_size(body.min, egui::vec2(LEFT_W, avail));
            let mut left = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            left.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let font = egui::TextStyle::Body.resolve(left.style());
            let strong = egui::FontId::proportional(font.size);
            let color = left.visuals().text_color();
            let r1 = new_row(&mut left, LEFT_W, 22.0);
            left.painter().text(egui::pos2(r1.left() + 4.0, r1.center().y), egui::Align2::LEFT_CENTER, "Band", strong.clone(), color);
            left.painter().text(egui::pos2(r1.left() + RX_X + 3.5 * PITCH - 5.0, r1.center().y), egui::Align2::CENTER_CENTER, "Rx", strong.clone(), color);
            left.painter().text(egui::pos2(r1.left() + TX_X + 3.5 * PITCH - 5.0, r1.center().y), egui::Align2::CENTER_CENTER, "Tx", strong.clone(), color);
            let r2 = new_row(&mut left, LEFT_W, 22.0);
            for i in 0..7 {
                for x0 in [RX_X, TX_X] {
                    left.painter().text(egui::pos2(r2.left() + x0 + i as f32 * PITCH + BOX / 2.0, r2.center().y), egui::Align2::CENTER_CENTER, format!("{}", i + 1), strong.clone(), color);
                }
            }
            let names = band_names(connected);
            let scroll_h = (avail - 48.0).max(120.0);
            touch_scroll(&mut left, "oc_rows", Some(scroll_h), false, &mut |ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                let inner_w = ui.available_width();
                for name in &names {
                    let row = new_row(ui, inner_w, ROW_H);
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let color = ui.visuals().text_color();
                    ui.painter().text(egui::pos2(row.left() + 4.0, row.center().y), egui::Align2::LEFT_CENTER, name, font, color);
                    let mut oc: OcMask = connected.oc_settings.get(name.as_str()).copied().unwrap_or_default();
                    let mut edited = false;
                    for i in 0..7u8 {
                        let mask = 1u8 << i;
                        let mut on = oc.rx & mask != 0;
                        let x0 = RX_X + i as f32 * PITCH;
                        let mut c = child(ui, row, x0, x0 + PITCH, false, 0.0);
                        if touch_checkbox_sized(&mut c, &mut on, "", BOX).changed() {
                            if on { oc.rx |= mask } else { oc.rx &= !mask }
                            edited = true;
                        }
                        let mut on = oc.tx & mask != 0;
                        let x0 = TX_X + i as f32 * PITCH;
                        let mut c = child(ui, row, x0, x0 + PITCH, false, 0.0);
                        if touch_checkbox_sized(&mut c, &mut on, "", BOX).changed() {
                            if on { oc.tx |= mask } else { oc.tx &= !mask }
                            edited = true;
                        }
                    }
                    if edited {
                        connected.oc_settings.insert(name.clone(), oc);
                        changed = true;
                    }
                }
            });

            // ---- Right: the Tune column.
            let pw = w - LEFT_W - 16.0;
            let right_rect = egui::Rect::from_min_size(egui::pos2(body.left() + LEFT_W + 16.0, body.top()), egui::vec2(pw, avail));
            let mut right = ui.new_child(egui::UiBuilder::new().max_rect(right_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            right.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let font = egui::TextStyle::Body.resolve(right.style());
            let strong = egui::FontId::proportional(font.size);
            let color = right.visuals().text_color();
            let r = new_row(&mut right, pw, 40.0);
            {
                let mut c = child(&mut right, r, 0.0, pw, false, 0.0);
                c.spacing_mut().item_spacing.x = 8.0;
                c.label(egui::RichText::new("Tune (ORed with TX)").strong());
                help_button(&mut c, "oc_tune_help", TUNE_HELP);
            }
            let r = new_row(&mut right, pw, 22.0);
            for i in 0..7 {
                right.painter().text(egui::pos2(r.left() + i as f32 * PITCH + BOX / 2.0, r.center().y), egui::Align2::CENTER_CENTER, format!("{}", i + 1), strong.clone(), color);
            }
            let r = new_row(&mut right, pw, ROW_H);
            for i in 0..7u8 {
                let mask = 1u8 << i;
                let mut on = connected.oc_tune & mask != 0;
                let x0 = i as f32 * PITCH;
                let mut c = child(&mut right, r, x0, x0 + PITCH, false, 0.0);
                if touch_checkbox_sized(&mut c, &mut on, "", BOX).changed() {
                    if on { connected.oc_tune |= mask } else { connected.oc_tune &= !mask }
                    changed = true;
                }
            }
            right.add_space(14.0);
            for (label, id, memory) in [("Full Tune (ms):", "oc_full_ms", false), ("Memory Tune (ms):", "oc_mem_ms", true)] {
                let r = new_row(&mut right, pw, 26.0);
                right.painter().text(egui::pos2(r.left() + 2.0, r.center().y), egui::Align2::LEFT_CENTER, label, strong.clone(), color);
                let r = new_row(&mut right, pw, 46.0);
                let cur = if memory { connected.oc_memory_tune_ms } else { connected.oc_full_tune_ms };
                let mut v = cur as f64;
                if spin(&mut right, r, 0.0, 98.0 + 110.0, id, &mut v, 0.0, 9999.0, 10.0, 0) {
                    let n = v.round().clamp(0.0, 9999.0) as u32;
                    if memory {
                        connected.oc_memory_tune_ms = n;
                    } else {
                        connected.oc_full_tune_ms = n;
                    }
                    changed = true;
                }
            }
            right.add_space(6.0);
            let r = new_row(&mut right, pw, 40.0);
            {
                let mut c = child(&mut right, r, 0.0, pw, false, 0.0);
                let mut on = connected.tune_full_armed;
                if touch_checkbox_sized(&mut c, &mut on, "Arm Full Tune", BOX).changed() {
                    connected.tune_full_armed = on;
                    if on {
                        connected.tune_memory_armed = false;
                    }
                }
            }
            let r = new_row(&mut right, pw, 40.0);
            {
                let mut c = child(&mut right, r, 0.0, pw, false, 0.0);
                let mut on = connected.tune_memory_armed;
                if touch_checkbox_sized(&mut c, &mut on, "Arm Memory Tune", BOX).changed() {
                    connected.tune_memory_armed = on;
                    if on {
                        connected.tune_full_armed = false;
                    }
                }
            }
        });

    (close_now, changed)
}
