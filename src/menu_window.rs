//! The "Menu" window (deskHPSDR's main menu screen): full-screen, one grid of equal touch buttons that reaches every
//! menu of the app. Every button reuses the code path that already opens the target (a full-screen window flag, a
//! compact popup, a VFO window or a Settings tab); nothing here has logic of its own besides closing the other windows
//! first, so two full-screen windows are never open together. Opened by the MIDI / toolbar action NEW MENU.

use crate::tx_window::{child, new_row};
use crate::{help_button, kiosk_accent_button, ConnectedState, FrequencyEntry, SettingsTab};

/// Button height and gaps (see docs/menu-screen.md for the height budget).
const BTN_H: f32 = 48.0;
const GAP_X: f32 = 12.0;
const GAP_Y: f32 = 12.0;
const COLS: usize = 6;

const NOT_IMPLEMENTED: &str = "Not implemented in this version.";
const NOT_AVAILABLE: &str = "Not available in this version.";

#[derive(Clone, Copy)]
enum Target {
    Sdr,
    Dsp,
    /// The PureSignal menu (ps_window.rs).
    Ps,
    /// The CW menu (cw_window.rs).
    Cw,
    /// The OC Output window (oc_window.rs).
    Oc,
    Toolbar,
    Vfo,
    Rx,
    Tx,
    Pa,
    Display,
    Noise,
    Eq,
    Vox,
    Band,
    Mode,
    Filter,
    Agc,
    Meter,
    Rade,
    /// Back to the device list (what Stop does).
    Discovery,
    /// Stop the session and reconnect to the same radio.
    Restart,
    /// Minimise the window.
    Iconify,
    /// Opens the Settings window on this tab.
    Tab(SettingsTab),
    /// Visible but inert, with a "!" help.
    Grey,
}

/// (label, target), rows top to bottom, 6 columns; `None` = empty cell.
const GRID: [[Option<(&str, Target)>; COLS]; 6] = [
    [
        Some(("SDR Device", Target::Sdr)),
        Some(("VFO", Target::Vfo)),
        Some(("RX", Target::Rx)),
        Some(("TX", Target::Tx)),
        Some(("DSP", Target::Dsp)),
        Some(("Toolbar", Target::Toolbar)),
    ],
    [
        Some(("Screen", Target::Tab(SettingsTab::Screen))),
        Some(("Band", Target::Band)),
        Some(("RX Filter", Target::Filter)),
        Some(("PA", Target::Pa)),
        Some(("WDSP EQ", Target::Eq)),
        Some(("CAT/TCI", Target::Tab(SettingsTab::Network))),
    ],
    [
        Some(("Display", Target::Display)),
        Some(("BandStack", Target::Grey)),
        Some(("Noise", Target::Noise)),
        Some(("VOX", Target::Vox)),
        Some(("Ant", Target::Tab(SettingsTab::Antenna))),
        Some(("MIDI", Target::Tab(SettingsTab::Midi))),
    ],
    [
        Some(("Meter", Target::Meter)),
        Some(("Mode", Target::Mode)),
        Some(("AGC", Target::Agc)),
        Some(("PS", Target::Ps)),
        Some(("OC Output", Target::Oc)),
        None,
    ],
    [
        Some(("XVTR", Target::Tab(SettingsTab::Xvtr))),
        Some(("Memory", Target::Grey)),
        None,
        Some(("RADE", Target::Rade)),
        Some(("Extras", Target::Grey)),
        None,
    ],
    [Some(("Discovery", Target::Discovery)), None, None, Some(("CW", Target::Cw)), None, None],
];

/// Closes every overlay that could share the screen with the Menu: the full-screen windows, the compact popups and the
/// Settings window.
pub(crate) fn close_overlays(c: &mut ConnectedState) {
    c.fnc_list_open = false;
    c.rx_window_open = false;
    c.tx_window_open = false;
    c.pa_window_open = false;
    c.toolbar_window_open = false;
    c.display_window_open = false;
    c.dsp_window_open = false;
    c.ps_window_open = false;
    c.cw_window_open = false;
    c.oc_window_open = false;
    c.noise_window_open = false;
    c.sdr_window_open = false;
    c.eq_window_open = false;
    c.vox_window_open = false;
    c.band_window_open = false;
    c.mode_window_open = false;
    c.filter_window_open = false;
    c.agc_window_open = false;
    c.meter_window_open = false;
    c.show_settings_window = false;
}

/// True while any window that the Menu can open is on screen.
pub(crate) fn any_overlay_open(c: &ConnectedState) -> bool {
    c.rx_window_open
        || c.tx_window_open
        || c.pa_window_open
        || c.toolbar_window_open
        || c.display_window_open
        || c.dsp_window_open
        || c.ps_window_open
        || c.cw_window_open
        || c.oc_window_open
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
}

/// Every frame: a window opened from the Menu has been closed (nothing is open any more) -> back to the Menu instead of
/// leaving the screen empty. Also covers targets that open nothing (e.g. TX without a transmitter, the RADE toggle).
pub(crate) fn return_tick(c: &mut ConnectedState) {
    if !c.menu_return {
        return;
    }
    if c.menu_window_open {
        c.menu_return = false;
    } else if !any_overlay_open(c) {
        c.menu_return = false;
        c.menu_window_open = true;
    }
}

/// The NEW MENU MIDI / toolbar action.
pub(crate) fn toggle(c: &mut ConnectedState) {
    c.menu_return = false;
    if c.menu_window_open {
        c.menu_window_open = false;
    } else {
        close_overlays(c);
        c.menu_window_open = true;
    }
}

fn open(c: &mut ConnectedState, target: Target) {
    c.menu_window_open = false;
    close_overlays(c);
    c.menu_return = true;   // closing what opens now brings the Menu back (return_tick)
    match target {
        Target::Sdr => c.sdr_window_open = true,
        Target::Vfo => c.frequency_entry = Some(FrequencyEntry { vfo_b: false, digits: String::new() }),
        Target::Rx => c.rx_window_open = true,
        Target::Tx => {
            if c.tx_handle.is_some() {
                c.tx_window_open = true;
            }
        }
        Target::Pa => c.pa_window_open = true,
        Target::Toolbar => c.toolbar_window_open = true,
        Target::Display => c.display_window_open = true,
        Target::Dsp => c.dsp_window_open = true,
        Target::Cw => c.cw_window_open = true,
        Target::Oc => c.oc_window_open = true,
        Target::Ps => {
            crate::ps_window::show_full();
            if c.tx_handle.is_some() {
                c.ps_window_open = true;
            }
        }
        Target::Noise => c.noise_window_open = true,
        Target::Eq => c.eq_window_open = true,
        Target::Vox => c.vox_window_open = true,
        Target::Band => c.band_window_open = true,
        Target::Mode => c.mode_window_open = true,
        Target::Filter => c.filter_window_open = true,
        Target::Agc => c.agc_window_open = true,
        Target::Meter => c.meter_window_open = true,
        Target::Rade => {
            // The RADE panel is part of the main screen, not an overlay: show it, do not come back to the Menu.
            c.menu_return = false;
            crate::toggle_rade_direct(c);
        }
        Target::Tab(tab) => {
            c.settings_tab = tab;
            c.show_settings_window = true;
        }
        Target::Discovery => {
            c.menu_return = false;
            c.menu_session_request = 1;
        }
        Target::Restart => {
            c.menu_return = false;
            c.menu_session_request = 2;
        }
        Target::Iconify => c.menu_return = false,   // the window command is sent by menu_window()
        Target::Grey => {}
    }
}

/// Returns (close, changed). `close` only ends the Menu; a button that opens another window closes the Menu itself.
pub fn menu_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut picked: Option<Target> = None;
    let title = match crate::config::settings_dir() {
        Some(d) => format!("hpsdr-rs - Menu [{}]", d.display()),
        None => "hpsdr-rs - Menu".to_string(),
    };

    // Not full screen: the window covers the spectrum/waterfall area and stops above the bottom toolbar (TOOLBAR_HEIGHT + TOOLBAR_MARGIN) and
    // the grey strip between them (the frame adds 8 px to this height: the window ends 553 px from the top of a 600 px screen, where the waterfall ends), so the toolbar stays
    // visible and usable. The buttons keep BTN_H; the space below About is simply empty.
    let btn_h = BTN_H;
    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    // No window shadow: it would darken the top of the bottom toolbar, which this window deliberately leaves visible.
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("Menu")
        .id(egui::Id::new("menu_window"))
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
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

            // Header row: the two inert controls on the left, CLOSE (96 px wide, 24 px from the screen edge) at the right.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, w - 110.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                for (label, target, bw) in [("Restart Protocol", Target::Restart, 230.0), ("Iconify", Target::Iconify, 150.0)] {
                    let b = egui::Button::new(egui::RichText::new(label).size(22.0).color(egui::Color32::from_gray(205)))
                        .fill(crate::chip_idle_fill())
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95)))
                        .corner_radius(5.0)
                        .min_size(egui::vec2(bw, 48.0));
                    if c.add(b).clicked() {
                        picked = Some(target);
                    }
                    c.add_space(12.0);
                }
            }
            egui::Area::new(egui::Id::new("menu_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(GAP_Y);

            // The grid of menu buttons.
            let cell_w = (w - GAP_X * (COLS as f32 - 1.0)) / COLS as f32;
            for (ri, cells) in GRID.iter().enumerate() {
                let row = new_row(ui, w, btn_h);
                for (ci, cell) in cells.iter().enumerate() {
                    let Some((label, target)) = cell else { continue };
                    let x0 = ci as f32 * (cell_w + GAP_X);
                    let rect = egui::Rect::from_min_size(egui::pos2(row.left() + x0, row.top()), egui::vec2(cell_w, btn_h));
                    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                    let text = egui::RichText::new(*label).size(22.0);
                    let tx_missing = matches!(target, Target::Tx) && connected.tx_handle.is_none();
                    let grey = matches!(target, Target::Grey) || tx_missing;
                    let btn = egui::Button::new(text.color(egui::Color32::from_gray(205)))
                        .fill(crate::chip_idle_fill())
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95)))
                        .corner_radius(5.0)
                        .min_size(rect.size());
                    if grey {
                        c.add_enabled(false, btn);
                    } else if c.add(btn).clicked() {
                        picked = Some(*target);
                    }
                }
                ui.add_space(GAP_Y);
            }

            // About, centred under the grid (wider button).
            let row = new_row(ui, w, btn_h);
            let aw = 2.0 * cell_w + GAP_X;
            let rect = egui::Rect::from_min_size(egui::pos2(row.center().x - aw / 2.0, row.top()), egui::vec2(aw, btn_h));
            let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
            let about = egui::Button::new(egui::RichText::new("About").size(22.0).color(egui::Color32::from_gray(205)))
                .fill(crate::chip_idle_fill())
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95)))
                .corner_radius(5.0)
                .min_size(rect.size());
            if c.add(about).clicked() {
                picked = Some(Target::Tab(SettingsTab::About));
            }
        });

    if let Some(t) = picked {
        if matches!(t, Target::Iconify) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        open(connected, t);
        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
    }
    (close_now, false)
}
