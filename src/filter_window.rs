//! The "Set RX Filter" menu (deskHPSDR filter_menu.c): same in-app window style as the BAND / MODE menus (Close top left, grid of
//! 5 lit buttons per row), plus the ESSB / digital special filters, "Set TX = RX filter edges" and the editable Var1 / Var2
//! filters with Default buttons. The tables are in `filters.rs`.

use crate::filters::{self, FilterEntry};
use crate::spectrum::Mode;
use crate::{chip_button, spin_buttons_full, std_checkbox, touch_close_button, ConnectedState};

const KEY_W: f32 = 108.0;
const KEY_H: f32 = 46.0;
const GAP: f32 = 6.0;

fn key(mode: Mode) -> String {
    mode.label().to_string()
}

/// (low, high) of Var1 and Var2 of `mode` as the user left them (or the defaults).
fn vars(connected: &ConnectedState, mode: Mode) -> [(i32, i32); 2] {
    match connected.filter_vars.get(&key(mode)) {
        Some(v) => [(v[0], v[1]), (v[2], v[3])],
        None => filters::var_defaults(mode),
    }
}

fn set_vars(connected: &mut ConnectedState, mode: Mode, v: [(i32, i32); 2]) {
    connected.filter_vars.insert(key(mode), [v[0].0, v[0].1, v[1].0, v[1].1]);
}

/// Edges of entry `idx` (0..15 fixed, 15 = Var1, 16 = Var2).
fn edges(connected: &ConnectedState, mode: Mode, idx: usize) -> (i32, i32) {
    if idx >= 15 {
        vars(connected, mode)[idx - 15]
    } else {
        let t: &[FilterEntry; 15] = filters::fixed_filters(mode);
        (t[idx].low, t[idx].high)
    }
}

/// Sets the RX (and TX) passband to the audio-domain edges, remembering the choice.
pub fn apply(connected: &mut ConnectedState, mode: Mode, idx: usize) {
    let (low, high) = edges(connected, mode, idx);
    let pb = filters::passband_for_edges(mode, low, high, crate::spectrum::cw_pitch_hz());
    connected.spectrum.set_explicit_passband(Some(pb));
    if let Some(tx) = &connected.tx_handle {
        tx.set_explicit_passband(Some(pb));
    }
    // Keep the main window's "Filter width" slider showing something meaningful.
    let width = (high - low).abs().max(25) as f64;
    connected.spectrum.set_width_hz(width);
    if let Some(tx) = &connected.tx_handle {
        tx.set_width_hz(width);
    }
    connected.width_memory.insert(key(mode), width);
    connected.filter_sel.insert(key(mode), idx);
    connected.filter_menu_applied = true;
    connected.filter_last_width = connected.spectrum.width_hz();
}

/// Per frame: when the mode changes, the filter chosen in this menu for the new mode is applied again; when the width
/// slider is moved the explicit passband of the menu is dropped so the slider works again.
pub fn tick(connected: &mut ConnectedState) {
    let mode = connected.spectrum.mode();
    if connected.filter_last_mode != Some(mode) {
        connected.filter_last_mode = Some(mode);
        if connected.filter_menu_applied {
            if let Some(&idx) = connected.filter_sel.get(&key(mode)) {
                apply(connected, mode, idx);
            } else {
                connected.spectrum.set_explicit_passband(None);
                if let Some(tx) = &connected.tx_handle {
                    tx.set_explicit_passband(None);
                }
                connected.filter_menu_applied = false;
            }
        }
        return;
    }
    if connected.filter_menu_applied && (connected.spectrum.width_hz() - connected.filter_last_width).abs() > 0.5 {
        connected.spectrum.set_explicit_passband(None);
        if let Some(tx) = &connected.tx_handle {
            tx.set_explicit_passband(None);
        }
        connected.filter_menu_applied = false;
    }
}

/// Returns (close, changed).
pub fn filter_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mode = connected.spectrum.mode();
    let cols = 5usize;
    let width_shift = filters::width_shift_mode(mode);
    let ssb = matches!(mode, Mode::Lsb | Mode::Usb);
    let dig = matches!(mode, Mode::Digl | Mode::Digu);
    let selected = if connected.filter_menu_applied { connected.filter_sel.get(&key(mode)).copied() } else { None };
    let mut close_now = false;
    let mut changed = false;
    let mut pick: Option<usize> = None;
    let mut var_edit: Option<([(i32, i32); 2], usize)> = None;
    let mut var_default: Option<usize> = None;
    let mut v = vars(connected, mode);
    let tx_ok = connected.tx_handle.is_some() && !matches!(mode, Mode::Cwl | Mode::Cwu);
    let mut use_rx = connected.tx_handle.as_ref().map(|t| t.tx_extra().use_rx_filter).unwrap_or(false);
    let use_rx0 = use_rx;
    let table = filters::fixed_filters(mode);

    egui::Window::new("filter_menu")
        .id(egui::Id::new("filter_menu_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // Anchored to the bottom with a gap above the toolbar (never over it), whatever the height of this mode's page.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -58.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if touch_close_button(ui, KEY_H).clicked() {
                    close_now = true;
                }
                ui.label(format!("Set RX Filter {}", mode.label()));
                if tx_ok {
                    ui.add_space(10.0);
                    if std_checkbox(ui, &mut use_rx, "Set TX = RX filter edges").changed() {}
                }
            });
            let grid = |ui: &mut egui::Ui, id: &str, range: std::ops::Range<usize>, pick: &mut Option<usize>| {
                egui::Grid::new(id).num_columns(cols).spacing([GAP, GAP]).show(ui, |ui| {
                    let mut n = 0;
                    for i in range {
                        if table[i].title.is_empty() {
                            continue;
                        }
                        if ui.add(chip_button(table[i].title, selected == Some(i)).min_size(egui::vec2(KEY_W, KEY_H))).clicked() {
                            *pick = Some(i);
                        }
                        n += 1;
                        if n % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
            };
            if mode == Mode::Fmn {
                ui.label("FM: the deviation is fixed in this version.");
                return;
            }
            grid(ui, "filter_fixed_a", 0..10, &mut pick);
            if ssb || dig {
                ui.separator();
                if ssb {
                    ui.vertical_centered(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 90, 90), egui::RichText::new("Observe legal TX bandwidth limits when using ESSB !").strong());
                    });
                }
                grid(ui, "filter_fixed_b", 10..15, &mut pick);
            }
            ui.separator();
            // Var1 / Var2.
            ui.horizontal(|ui| {
                ui.add_space(KEY_W + GAP + 4.0);
                let (a, b) = if width_shift { ("Filter Width:", "Filter Shift:") } else { ("Filter Cut Low:", "Filter Cut High:") };
                ui.add_sized([150.0, 20.0], egui::Label::new(egui::RichText::new(a).strong()));
                ui.add_space(8.0);
                ui.add_sized([150.0, 20.0], egui::Label::new(egui::RichText::new(b).strong()));
            });
            for n in 0..2usize {
                let idx = 15 + n;
                ui.horizontal(|ui| {
                    let title = if n == 0 { "Var1" } else { "Var2" };
                    if ui.add(chip_button(title, selected == Some(idx)).min_size(egui::vec2(KEY_W, KEY_H))).clicked() {
                        pick = Some(idx);
                    }
                    let (low, high) = v[n];
                    // The two spins: low/high cut (or width/shift) as deskHPSDR shows them.
                    let (mut a, mut b, amin, amax, bmin, bmax) = if width_shift {
                        ((high - low) as f64, 0.5 * (high + low) as f64, 5.0, 20000.0, -10000.0, 10000.0)
                    } else if matches!(mode, Mode::Lsb | Mode::Digl) {
                        ((-high) as f64, (-low) as f64, 0.0, 8000.0, 0.0, 8000.0)
                    } else {
                        (low as f64, high as f64, 0.0, 8000.0, 0.0, 8000.0)
                    };
                    let (a0, b0) = (a, b);
                    let id = format!("filt_{n}");
                    spin_buttons_full(ui, &format!("{id}_a"), &mut a, amin, amax, 5.0, 0, None, 56.0);
                    ui.add_space(6.0);
                    spin_buttons_full(ui, &format!("{id}_b"), &mut b, bmin, bmax, 5.0, 0, None, 56.0);
                    if a != a0 || b != b0 {
                        let (mut nl, mut nh) = (low, high);
                        if a != a0 {
                            let val = a.round() as i32;
                            match mode {
                                Mode::Cwl | Mode::Cwu | Mode::Dsb | Mode::Am | Mode::Sam | Mode::Spec | Mode::Drm => {
                                    let shift = (low + high) / 2;
                                    nl = shift - val / 2;
                                    nh = shift + val / 2;
                                }
                                Mode::Lsb | Mode::Digl => nh = -val,
                                _ => nl = val,
                            }
                        }
                        if b != b0 {
                            let val = b.round() as i32;
                            match mode {
                                Mode::Cwl => {
                                    let width = high - low;
                                    nl = -val - width / 2;
                                    nh = -val + width / 2;
                                }
                                Mode::Cwu | Mode::Dsb | Mode::Am | Mode::Sam | Mode::Spec | Mode::Drm => {
                                    let width = high - low;
                                    nl = val - width / 2;
                                    nh = val + width / 2;
                                }
                                Mode::Lsb | Mode::Digl => nl = -val,
                                _ => nh = val,
                            }
                        }
                        v[n] = (nl, nh);
                        var_edit = Some((v, n));
                    }
                    ui.add_space(6.0);
                    if ui.add(chip_button("Default", false).min_size(egui::vec2(KEY_W - 10.0, KEY_H))).clicked() {
                        var_default = Some(n);
                    }
                });
            }
        });

    if let Some(i) = pick {
        apply(connected, mode, i);
        changed = true;
    }
    if let Some((nv, n)) = var_edit {
        set_vars(connected, mode, nv);
        if connected.filter_menu_applied && connected.filter_sel.get(&key(mode)) == Some(&(15 + n)) {
            apply(connected, mode, 15 + n);
        }
        changed = true;
    }
    if let Some(n) = var_default {
        let mut nv = vars(connected, mode);
        nv[n] = filters::var_defaults(mode)[n];
        set_vars(connected, mode, nv);
        if connected.filter_menu_applied && connected.filter_sel.get(&key(mode)) == Some(&(15 + n)) {
            apply(connected, mode, 15 + n);
        }
        changed = true;
    }
    if use_rx != use_rx0 {
        if let Some(tx) = connected.tx_handle.as_ref() {
            let mut ex = tx.tx_extra();
            ex.use_rx_filter = use_rx;
            tx.set_tx_extra(ex);
        }
        changed = true;
    }
    (close_now, changed)
}
