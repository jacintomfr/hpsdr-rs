//! The "WDSP EQ Menu" window (deskHPSDR equalizer_menu.c): RX1 / TX / extra-receiver 12-point EQ with an interactive
//! curve plot, curve type, NURBS weights, a 12-row spin table, per-mode EQ and the three mic-profile slots.
//!
//! The window state that is not part of the radio (which side is shown, the point being dragged, the profile
//! description being typed) lives in egui's temp memory; the per-mode EQ maps and the active profile slot live in
//! `ConnectedState` so they are saved with the configuration.

use crate::eq_curve as ec;
use crate::eq_profiles as ep;
use crate::spectrum::EqualizerParams;
use crate::{chip_button, spin_buttons_full, toggle_chip, ConnectedState};

/// DSP sample rate of the EQ channels (RX and TX run at 48 kHz in this program).
const DSP_RATE_HZ: f64 = 48000.0;

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Rx1,
    Tx,
    Extra(usize),
}

fn get_eq(c: &ConnectedState, side: Side) -> Option<EqualizerParams> {
    match side {
        Side::Rx1 => Some(c.spectrum.eq()),
        Side::Tx => c.tx_handle.as_ref().map(|t| t.eq()),
        Side::Extra(i) => c.extra_receivers.get(i).map(|r| r.lock().unwrap().spectrum.eq()),
    }
}

/// Applies `eq` to the radio and keeps the per-mode map of the current mode group up to date.
fn put_eq(c: &mut ConnectedState, side: Side, eq: EqualizerParams) {
    let group = ep::eq_mode_group(c.spectrum.mode()).to_string();
    match side {
        Side::Rx1 => {
            c.spectrum.set_eq(eq);
            c.rx_eq_by_mode.insert(group, eq);
        }
        Side::Tx => {
            if let Some(t) = c.tx_handle.as_ref() {
                t.set_eq(eq);
            }
            c.tx_eq_by_mode.insert(group, eq);
        }
        Side::Extra(i) => {
            if let Some(r) = c.extra_receivers.get(i) {
                let r = r.lock().unwrap();
                r.spectrum.set_eq(eq);
                r.settings_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

/// Per frame: when the mode moves to another EQ group (SSB / CW / DIG / AM ...), remember the EQ of the group being left
/// and load the one of the new group (deskHPSDR's per-mode EQ), for RX1 and TX.
pub fn mode_tick(c: &mut ConnectedState) {
    let mode = c.spectrum.mode();
    let group = ep::eq_mode_group(mode);
    if c.eq_mode_group_seen.is_empty() {
        c.eq_mode_group_seen = group.to_string();
        return;
    }
    if c.eq_mode_group_seen == group {
        return;
    }
    let old = std::mem::replace(&mut c.eq_mode_group_seen, group.to_string());
    c.rx_eq_by_mode.insert(old.clone(), c.spectrum.eq());
    let rx = c.rx_eq_by_mode.get(group).copied().unwrap_or_else(|| ep::default_eq_for_mode(mode, false));
    c.spectrum.set_eq(rx);
    if let Some(t) = c.tx_handle.as_ref() {
        c.tx_eq_by_mode.insert(old, t.eq());
        let tx = c.tx_eq_by_mode.get(group).copied().unwrap_or_else(|| ep::default_eq_for_mode(mode, true));
        t.set_eq(tx);
    }
}

fn sides(c: &ConnectedState) -> Vec<(Side, String)> {
    let mut v = vec![(Side::Rx1, "RX1 EQ Settings".to_string())];
    if c.tx_handle.is_some() {
        v.push((Side::Tx, "TX EQ Settings".to_string()));
    }
    for i in 0..c.extra_receivers.len() {
        v.push((Side::Extra(i), format!("RX{} EQ Settings", i + 2)));
    }
    v
}

/// Returns (close, changed). Draws the window like deskHPSDR's "WDSP EQ Menu": full screen, curve plot on top, the
/// spin-box table below.
pub fn eq_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;

    let side_id = egui::Id::new("eq_window_side");
    let drag_id = egui::Id::new("eq_window_drag");
    let desc_id = egui::Id::new("eq_window_desc");
    let mut side: Side = ui.ctx().data(|d| d.get_temp(side_id)).unwrap_or(Side::Rx1);
    let side_list = sides(connected);
    if !side_list.iter().any(|(s, _)| *s == side) {
        side = Side::Rx1;
    }

    let profile_names = ep::slot_descriptions();
    let title = match connected.mic_profile_nr {
        Some(n) if (1..=3).contains(&n) => format!("hpsdr-rs - WDSP EQ Menu (Mic Profile: {})", profile_names[n - 1]),
        _ => "hpsdr-rs - WDSP EQ Menu".to_string(),
    };

    let frame = egui::Frame::window(ui.style()).inner_margin(4.0).corner_radius(0.0);
    egui::Window::new("WDSP EQ Menu")
        .id(egui::Id::new("eq_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .fixed_pos(screen.min)
        .constrain_to(screen)
        .fixed_size(screen.size() - egui::vec2(10.0, 10.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            let avail_w = screen.width() - 10.0;
            ui.set_width(avail_w);
            ui.set_min_height(screen.height() - 10.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
            ui.scope(|ui| {
                    // Title strip.
                    let (tr, _) = ui.allocate_exact_size(egui::vec2(avail_w, 22.0), egui::Sense::hover());
                    ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, &title, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

                    // Row 1: Close, the RX/TX radios, the mic profiles.
                    ui.horizontal(|ui| {
                        ui.set_height(36.0);
                        for (s, label) in &side_list {
                            ui.radio_value(&mut side, *s, label);
                            ui.add_space(24.0);
                        }
                        if connected.tx_handle.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if let Some(n) = connected.mic_profile_nr {
                                    let mut desc: String = ui.ctx().data(|d| d.get_temp(desc_id)).unwrap_or_else(|| {
                                        ep::load_slot(n).map(|p| p.desc).unwrap_or_default()
                                    });
                                    if ui.add(chip_button("Save", false).min_size(egui::vec2(60.0, 30.0))).clicked() {
                                        if let Some(tx) = connected.tx_handle.as_ref() {
                                            let mut p = ep::MicProfile::capture(tx, connected.spectrum.eq());
                                            p.desc = desc.clone();
                                            let _ = ep::save_slot(n, &p);
                                        }
                                    }
                                    ui.add(egui::TextEdit::singleline(&mut desc).desired_width(110.0));
                                    ui.ctx().data_mut(|d| d.insert_temp(desc_id, desc));
                                }
                                for n in (1..=3usize).rev() {
                                    let active = connected.mic_profile_nr == Some(n);
                                    if toggle_chip(ui, &n.to_string(), active, 30.0, &format!("Load mic profile {n}: {}", profile_names[n - 1])).clicked() {
                                        if let (Some(p), Some(tx)) = (ep::load_slot(n), connected.tx_handle.as_ref()) {
                                            let rx = p.apply(tx);
                                            let tx_eq = tx.eq();
                                            connected.spectrum.set_eq(rx);
                                            let group = ep::eq_mode_group(connected.spectrum.mode()).to_string();
                                            connected.rx_eq_by_mode.insert(group.clone(), rx);
                                            connected.tx_eq_by_mode.insert(group, tx_eq);
                                            connected.mic_profile_nr = Some(n);
                                            ui.ctx().data_mut(|d| d.insert_temp(desc_id, p.desc.clone()));
                                            changed = true;
                                        }
                                    }
                                }
                                ui.label("Mic profile:");
                            });
                        }
                    });
                    let Some(mut eq) = get_eq(connected, side) else {
                        return;
                    };
                    let is_tx = side == Side::Tx;
                    let mut dirty = false;

                    // Row 2: Enable ... "Added Frequency-Independent Gain:" [spin].
                    ui.horizontal(|ui| {
                        ui.set_height(36.0);
                        if ui.checkbox(&mut eq.enabled, "Enable").changed() {
                            dirty = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut v = eq.preamp12_db as f64;
                            // The spin keeps its own left-to-right order (-, +) inside this right-to-left row.
                            ui.allocate_ui_with_layout(egui::vec2(262.0, 34.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                if spin_buttons_full(ui, "eqw_preamp", &mut v, -20.0, 20.0, 1.0, 0, None, 160.0).changed() {
                                    eq.preamp12_db = v.round() as i32;
                                    dirty = true;
                                }
                            });
                            ui.label("Added Frequency-Independent Gain:");
                        });
                    });

                    // The framed plot with the curve row under it; its height takes what the table leaves.
                    let plot_h = (screen.height() - 470.0).max(100.0);
                    egui::Frame::NONE
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(150)))
                        .inner_margin(3.0)
                        .show(ui, |ui| {
                    let plot_w = ui.available_width();
                    // ---- plot
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(plot_w, plot_h), egui::Sense::click_and_drag());
                    let (pw, ph) = ec::plot_size(plot_w as f64, plot_h as f64);
                    let origin = rect.min + egui::vec2(ec::PAD_LEFT as f32, ec::PAD_TOP as f32);
                    let to_screen = |x: f64, y: f64| egui::pos2(origin.x + x as f32, origin.y + y as f32);

                    // Dragged copy shown while the pointer is down; the DSP gets it on release.
                    let mut drag: Option<(Side, usize, EqualizerParams)> = ui.ctx().data(|d| d.get_temp(drag_id));
                    if drag.as_ref().is_some_and(|d| d.0 != side) {
                        drag = None;
                    }
                    if resp.drag_started() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            let pts = ec::point_positions(&eq, pw, ph);
                            let local = (p.x as f64 - origin.x as f64, p.y as f64 - origin.y as f64);
                            if let Some(i) = ec::nearest_point(&pts, local.0, local.1, ec::HIT_RADIUS) {
                                drag = Some((side, i, eq));
                            }
                        }
                    }
                    if let Some((s, i, mut d)) = drag {
                        if resp.dragged() {
                            if let Some(p) = resp.interact_pointer_pos() {
                                let f = ec::freq_for_x(p.x as f64 - origin.x as f64, pw);
                                let g = ec::gain_for_y(p.y as f64 - origin.y as f64, ph);
                                let (nf, ng) = ec::drag_clamp_params(&d, i, f, g);
                                d.freqs_12_hz[i] = nf;
                                d.bands_12_db[i] = ng;
                            }
                            drag = Some((s, i, d));
                        } else {
                            // Released: apply once (the FIR rebuild is heavy, so not while moving).
                            eq = d;
                            dirty = true;
                            drag = None;
                        }
                    }
                    ui.ctx().data_mut(|d| match drag {
                        Some(v) => {
                            d.insert_temp(drag_id, v);
                        }
                        None => d.remove::<(Side, usize, EqualizerParams)>(drag_id),
                    });
                    let shown = drag.map(|d| d.2).unwrap_or(eq);

                    // Mouse wheel over a point: its NURBS weight (0.1 .. 99.9).
                    if eq.nurbs_r && eq.curve_deg > 0 && resp.hovered() {
                        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
                        if wheel.abs() > 0.5 {
                            if let Some(p) = resp.hover_pos() {
                                let pts = ec::point_positions(&eq, pw, ph);
                                if let Some(i) = ec::nearest_point(&pts, p.x as f64 - origin.x as f64, p.y as f64 - origin.y as f64, ec::HIT_RADIUS) {
                                    let w = eq.weights_x10[i] as i32 + if wheel > 0.0 { 1 } else { -1 };
                                    eq.weights_x10[i] = w.clamp(1, 999) as u16;
                                    dirty = true;
                                }
                            }
                        }
                    }

                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));
                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
                    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(55));
                    let label_col = egui::Color32::from_gray(170);
                    let font = egui::FontId::proportional(11.0);
                    for (g, lab) in ec::GAIN_LINES.iter().zip(ec::GAIN_LABELS.iter()) {
                        let y = ec::y_for_gain(*g as f64, ph);
                        painter.line_segment([to_screen(0.0, y), to_screen(pw, y)], grid);
                        painter.text(to_screen(-4.0, y), egui::Align2::RIGHT_CENTER, *lab, font.clone(), label_col);
                    }
                    for (f, lab) in ec::FREQ_LINES.iter().zip(ec::FREQ_LABELS.iter()) {
                        let x = ec::x_for_freq(*f as f64, pw);
                        painter.line_segment([to_screen(x, 0.0), to_screen(x, ph)], grid);
                        painter.text(to_screen(x, ph + 4.0), egui::Align2::CENTER_TOP, *lab, font.clone(), label_col);
                    }
                    painter.text(
                        egui::pos2(rect.center().x, rect.bottom() - 2.0),
                        egui::Align2::CENTER_BOTTOM,
                        "",
                        font.clone(),
                        label_col,
                    );
                    painter.text(
                        to_screen(6.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        if is_tx { "TX EQ Curve" } else { "RX EQ Curve" },
                        egui::FontId::proportional(12.0),
                        label_col,
                    );
                    let curve = ec::curve_for_params(&shown, DSP_RATE_HZ);
                    let line: Vec<egui::Pos2> = curve.iter().map(|(hz, db)| to_screen(ec::x_for_freq(*hz, pw), ec::y_for_gain(*db, ph))).collect();
                    let line_col = egui::Color32::from_rgb(120, 190, 255);
                    if line.len() >= 2 {
                        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, line_col)));
                    }
                    for p in ec::point_positions(&shown, pw, ph) {
                        painter.circle_filled(to_screen(p.0, p.1), ec::POINT_RADIUS as f32, egui::Color32::from_rgb(232, 150, 46));
                    }

                    // ---- curve type
                    ui.horizontal(|ui| {
                        ui.label("Curve:");
                        let cur = ec::index_for_curve_degree(eq.curve_deg);
                        egui::ComboBox::from_id_salt("eqw_curve")
                            .selected_text(ec::CURVE_LABELS[cur])
                            .show_ui(ui, |ui| {
                                for (i, l) in ec::CURVE_LABELS.iter().enumerate() {
                                    if ui.selectable_label(cur == i, *l).clicked() {
                                        eq.curve_deg = ec::curve_degree_for_index(i);
                                        dirty = true;
                                    }
                                }
                            });
                        if ui.checkbox(&mut eq.nurbs_r, "NURBS weights").changed() {
                            dirty = true;
                        }
                        ui.weak("Drag points; DSP updates on release. Curve excludes the frequency-independent gain.");
                    });
                        });

                    // ---- table: point i in the two left columns, i + 6 in the two right ones
                    let gap = 10.0f32;
                    let f_col = (avail_w - gap) / 2.0 * 0.52;
                    let g_col = (avail_w - gap) / 2.0 * 0.48;
                    let col_x = [0.0, f_col, f_col + g_col + gap, 2.0 * f_col + g_col + gap];
                    let col_w = [f_col, g_col, f_col, g_col];
                    let (hr, _) = ui.allocate_exact_size(egui::vec2(avail_w, 18.0), egui::Sense::hover());
                    for c in 0..4 {
                        ui.painter().text(
                            egui::pos2(hr.left() + col_x[c] + col_w[c] / 2.0, hr.center().y),
                            egui::Align2::CENTER_CENTER,
                            if c % 2 == 0 { "Frequency" } else { "Gain" },
                            egui::FontId::proportional(15.0),
                            egui::Color32::from_gray(225),
                        );
                    }
                    for row in 0..6 {
                        let (row_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 34.0), egui::Sense::hover());
                        for c in 0..4 {
                            let cell = egui::Rect::from_min_size(egui::pos2(row_rect.left() + col_x[c], row_rect.top()), egui::vec2(col_w[c], 34.0));
                            let mut cell_ui = ui.new_child(egui::UiBuilder::new().max_rect(cell).layout(egui::Layout::left_to_right(egui::Align::Center)));
                            let i = row + 6 * (c / 2);
                            if c % 2 == 0 {
                                let mut f = eq.freqs_12_hz[i] as f64;
                                if spin_buttons_full(&mut cell_ui, &format!("eqw_f{i}"), &mut f, 10.0, 16000.0, 10.0, 0, None, col_w[c] - 98.0).changed() {
                                    eq.freqs_12_hz[i] = ec::clamp_freq_for_spin_params(&eq, i, f.round() as i32);
                                    dirty = true;
                                }
                            } else {
                                let mut g = eq.bands_12_db[i] as f64;
                                if spin_buttons_full(&mut cell_ui, &format!("eqw_g{i}"), &mut g, -20.0, 20.0, 1.0, 0, None, col_w[c] - 98.0).changed() {
                                    eq.bands_12_db[i] = (g.round() as i32).clamp(-20, 20);
                                    dirty = true;
                                }
                            }
                        }
                    }

                    egui::Area::new(egui::Id::new("eq_window_close"))
                        .order(egui::Order::Foreground)
                        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -26.0))
                        .show(ui.ctx(), |ui| {
                            if crate::kiosk_accent_button(ui, "CLOSE").clicked() {
                                close_now = true;
                            }
                        });

                    if dirty {
                        put_eq(connected, side, eq);
                        changed = true;
                    }
                });
        });
    ui.ctx().data_mut(|d| d.insert_temp(side_id, side));
    (close_now, changed)
}
