//! The "PureSignal" menu (deskHPSDR ps_menu.c, the PS button of the Menu): same in-app window style as the AGC / DSP menus
//! (compact overlay anchored above the toolbar, Close top left, stays open until Close).
//!
//! Layout of deskHPSDR's dialog, row by row: Enable PS | Two Tone | Auto Attenuate | OFF | Restart; OneShot | PS Stability;
//! Feedback Lvl | Correcting; feedbk | cor.cnt | sln.chk; status; GetPk | SetPk | TX ATT.
//! Not in this version yet (they need engine support that does not exist here): Noise generator, MON (feedback spectrum),
//! PS FeedBk ANT and AmpView.
//!
//! Everything shown is wired to the PureSignal engine of tx.rs; the Auto Attenuate algorithm of ps_menu.c
//! (ps_calibration_timer) runs in `auto_tick`, called every frame from main.rs.

use crate::noise_window::choice_combo_h;
use crate::{help_button, spin_buttons_full, std_checkbox, touch_close_button, Boards, ConnectedState};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ROW_H: f32 = 48.0;

/// The menu is reduced to a small bar (button "Hide") so the spectrum and the waterfall can be seen.
static HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called when the menu is opened from the Menu: always starts in full size.
pub(crate) fn show_full() {
    HIDDEN.store(false, Ordering::Relaxed);
}
const GAP: f32 = 6.0;
const LABEL_W: f32 = 165.0;
const BOX_W: f32 = 130.0;

const ENABLE_HELP: &str = "Enable PureSignal [ADP = Adaptive Predistortion].";
const AUTO_HELP: &str = "Automatically adjusts the TX attenuation during PS calibration (needs Two Tone).";
const STABILITY_HELP: &str = "PureSignal 3 compression/deadlock check threshold.\nStrict: 0.06 (highest solution validation)\nMedium: 0.04 (balanced stability and output power)\nRelaxed: 0.02 (more tolerant of highly compressed power amplifiers)";
const ONESHOT_HELP: &str = "One calibration, then the correction is only applied (for constant-envelope digital modes).";
const PEAK_HELP: &str = "GetPk: the largest TX envelope WDSP has measured.\nSetPk: the hardware peak the engine assumes for this radio (0.01 to 1.01); a change restarts the calibration.";
const ATT_HELP: &str = "Attenuation of the feedback path during transmit. On the Hermes Lite 2 it sets the receiver gain while transmitting (-29 to +31). Aim for a Feedback Lvl of 140 to 165; Auto Attenuate does that for you.";

fn is_hl2(connected: &ConnectedState) -> bool {
    matches!(connected.device.board, Boards::HermesLite | Boards::HermesLite2)
}

/// Valid range of the feedback attenuation: -29..31 on the Hermes Lite 2 (deskHPSDR), 0..31 elsewhere.
fn att_range(connected: &ConnectedState) -> (i32, i32) {
    if is_hl2(connected) { (-29, 31) } else { (0, 31) }
}

/// The feedback attenuation (signed; stored as its two's complement in the radio session's u32).
pub(crate) fn att_get(connected: &ConnectedState) -> i32 {
    connected.session.ps_tx_attenuation.load(Ordering::Relaxed) as i32
}

pub(crate) fn att_set(connected: &mut ConnectedState, v: i32) {
    let (lo, hi) = att_range(connected);
    connected.session.ps_tx_attenuation.store(v.clamp(lo, hi) as u32, Ordering::Relaxed);
    connected.settings_dirty.store(true, Ordering::Relaxed);
}

/// Feedback level colour (ps_menu.c info_thread): > 181 blue, > 128 green, > 90 yellow, else red.
fn level_color(level: i32) -> egui::Color32 {
    if level > 181 {
        egui::Color32::from_rgb(80, 140, 240)
    } else if level > 128 {
        egui::Color32::from_rgb(80, 200, 80)
    } else if level > 90 {
        egui::Color32::from_rgb(230, 210, 60)
    } else {
        egui::Color32::from_rgb(230, 70, 70)
    }
}

/// A fixed-width label cell.
fn label(ui: &mut egui::Ui, text: &str, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::hover());
    ui.painter().text(rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, text, egui::FontId::proportional(18.0), egui::Color32::WHITE);
}

/// A read-only value box.
fn value_box(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(BOX_W, 40.0), egui::Sense::hover());
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(22), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(20.0), color);
}

/// Returns (close, changed).
pub fn ps_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mut close_now = false;
    let mut changed = false;
    let status = connected.tx_handle.as_ref().map(|t| *t.ps_status.lock().unwrap()).unwrap_or_default();
    let ps_on = connected.puresignal_enabled;
    let hl2 = is_hl2(connected);
    let (att_lo, att_hi) = att_range(connected);
    let att = att_get(connected);
    let auto_on = connected.ps_auto_attenuate;
    let two_tone = connected.two_tone_active;

    // Actions collected while drawing, applied afterwards (the closure only borrows copies).
    let mut enable: Option<bool> = None;
    let mut toggle_two_tone = false;
    let mut auto: Option<bool> = None;
    let mut off = false;
    let mut restart = false;
    let mut oneshot: Option<bool> = None;
    let mut stability: Option<u8> = None;
    let mut new_peak: Option<f64> = None;
    let mut new_att: Option<i32> = None;
    let mut ps_enable_value = ps_on;
    let mut oneshot_value = connected.ps_oneshot;
    let mut auto_value = auto_on;
    let mut peak_value = connected.ps_hw_peak;
    let mut att_value = att as f64;
    let stability_now = connected.ps_stability.min(2) as usize;
    let diversity = connected.diversity_enabled;

    if HIDDEN.load(Ordering::Relaxed) {
        // The bar: Show, the live feedback numbers, Two Tone and the attenuation, so a calibration can be followed on the
        // spectrum above it.
        let mut show = false;
        let mut tt = false;
        egui::Window::new("ps_menu_bar")
            .id(egui::Id::new("ps_menu_bar_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -58.0))
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                ui.horizontal(|ui| {
                    if crate::kiosk_accent_button(ui, "Show").clicked() {
                        show = true;
                    }
                    label(ui, "Feedback", 100.0);
                    value_box(ui, &format!("{}", status.feedback_level), if ps_on { level_color(status.feedback_level) } else { egui::Color32::from_gray(120) });
                    let (txt, col) = if !ps_on {
                        ("", egui::Color32::from_gray(120))
                    } else if status.correcting {
                        ("Correcting", egui::Color32::from_rgb(80, 200, 80))
                    } else {
                        ("no corr.", egui::Color32::from_rgb(230, 70, 70))
                    };
                    value_box(ui, txt, col);
                    label(ui, "ATT", 60.0);
                    value_box(ui, &att.to_string(), egui::Color32::WHITE);
                    let b = egui::Button::new(egui::RichText::new("Two Tone").color(if two_tone { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if two_tone { egui::Color32::from_rgb(230, 140, 20) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(5.0);
                    if ui.add(b).clicked() {
                        tt = true;
                    }
                });
            });
        if show {
            HIDDEN.store(false, Ordering::Relaxed);
        }
        if tt {
            connected.toolbar_two_tone_request = true;
        }
        return (false, false);
    }
    let mut hide_now = false;
    egui::Window::new("ps_menu")
        .id(egui::Id::new("ps_menu_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // Same placement rule as the AGC / DSP menus: centred, anchored above the toolbar with a gap.
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
                ui.label("Pure Signal");
                ui.add_space(24.0);
                if ui.add(egui::Button::new("Hide").min_size(egui::vec2(100.0, ROW_H))).clicked() {
                    hide_now = true;
                }
            });

            // Row 1: Enable PS | Two Tone | Auto Attenuate | OFF | Restart.
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!diversity, |ui| {
                    if std_checkbox(ui, &mut ps_enable_value, "Enable PS").changed() {
                        enable = Some(ps_enable_value);
                    }
                });
                help_button(ui, "ps_help_enable", ENABLE_HELP);
                ui.add_space(8.0);
                let tt = egui::Button::new(egui::RichText::new("Two Tone").color(if two_tone { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                    .fill(if two_tone { egui::Color32::from_rgb(230, 140, 20) } else { egui::Color32::from_gray(48) })
                    .min_size(egui::vec2(110.0, 40.0))
                    .corner_radius(5.0);
                if ui.add(tt).clicked() {
                    toggle_two_tone = true;
                }
                ui.add_space(8.0);
                if std_checkbox(ui, &mut auto_value, "Auto Attenuate").changed() {
                    auto = Some(auto_value);
                }
                help_button(ui, "ps_help_auto", AUTO_HELP);
                ui.add_space(8.0);
                if ui.add_enabled(ps_on, egui::Button::new("OFF").min_size(egui::vec2(90.0, 40.0))).clicked() {
                    off = true;
                }
                if ui.add_enabled(ps_on, egui::Button::new("Restart").min_size(egui::vec2(100.0, 40.0))).clicked() {
                    restart = true;
                }
            });

            // Row 2: OneShot | PS Stability.
            ui.horizontal(|ui| {
                if std_checkbox(ui, &mut oneshot_value, "OneShot").changed() {
                    oneshot = Some(oneshot_value);
                }
                help_button(ui, "ps_help_oneshot", ONESHOT_HELP);
                ui.add_space(20.0);
                label(ui, "PS Stability:", LABEL_W);
                let items = [("Strict", true), ("Medium", true), ("Relaxed", true)];
                if let Some(i) = choice_combo_h(ui, "ps_stability", 130.0, stability_now, &items, 34.0) {
                    stability = Some(i as u8);
                }
                help_button(ui, "ps_help_stability", STABILITY_HELP);
            });

            ui.add_space(4.0);
            // Row 3: Feedback Lvl | Correcting (coloured like deskHPSDR) and the optimal range.
            ui.horizontal(|ui| {
                label(ui, "Feedback Lvl", LABEL_W);
                value_box(ui, &format!("{}", status.feedback_level), if ps_on { level_color(status.feedback_level) } else { egui::Color32::from_gray(120) });
                label(ui, "Correcting", LABEL_W);
                let (txt, col) = if !ps_on {
                    ("", egui::Color32::from_gray(120))
                } else if status.correcting {
                    ("yes", egui::Color32::from_rgb(80, 200, 80))
                } else {
                    ("no", egui::Color32::from_rgb(230, 70, 70))
                };
                value_box(ui, txt, col);
                ui.label("[Optimal feedback level between 140..165]");
            });
            // Row 4: feedbk | cor.cnt | sln.chk.
            ui.horizontal(|ui| {
                let g = egui::Color32::from_gray(225);
                label(ui, "feedbk", LABEL_W);
                value_box(ui, &if ps_on { status.feedback_level.to_string() } else { String::new() }, g);
                label(ui, "cor.cnt", LABEL_W);
                value_box(ui, &if ps_on { status.cal_count.to_string() } else { String::new() }, g);
                label(ui, "sln.chk", LABEL_W);
                value_box(ui, &if ps_on { status.solution_check.to_string() } else { String::new() }, g);
            });
            // Row 5: status.
            ui.horizontal(|ui| {
                label(ui, "status", LABEL_W);
                value_box(ui, &if ps_on { crate::tx::ps_state_name(status.state).to_string() } else { String::new() }, egui::Color32::from_gray(225));
            });

            ui.add_space(4.0);
            // Row 6: GetPk | SetPk | TX ATT.
            ui.horizontal(|ui| {
                label(ui, "GetPk", LABEL_W);
                value_box(ui, &if ps_on { format!("{:.3}", status.max_tx) } else { String::new() }, egui::Color32::from_gray(225));
                help_button(ui, "ps_help_peak", PEAK_HELP);
                ui.add_space(8.0);
                label(ui, "SetPk", 90.0);
                if spin_buttons_full(ui, "ps_setpk", &mut peak_value, 0.01, 1.01, 0.001, 3, None, 78.0).changed() {
                    new_peak = Some(peak_value);
                }
            });
            ui.horizontal(|ui| {
                label(ui, "TX ATT", LABEL_W);
                if auto_on {
                    // Automatic: shown, not editable (deskHPSDR shows an entry instead of the spin).
                    value_box(ui, &if ps_on { att.to_string() } else { String::new() }, egui::Color32::from_gray(225));
                } else if spin_buttons_full(ui, "ps_txatt", &mut att_value, att_lo as f64, att_hi as f64, 1.0, 0, None, 78.0).changed() {
                    new_att = Some(att_value.round() as i32);
                }
                help_button(ui, "ps_help_att", ATT_HELP);
                if hl2 {
                    ui.label("(HL2: -29..+31)");
                }
            });
        });

    if hide_now {
        HIDDEN.store(true, Ordering::Relaxed);
    }
    if close_now {
        HIDDEN.store(false, Ordering::Relaxed);
    }
    // ---- Apply the actions.
    if let Some(on) = enable {
        connected.puresignal_enabled = on;
        connected.session.set_puresignal_enabled(on);
        if let Some(tx) = &connected.tx_handle {
            tx.set_puresignal_enabled(on);
            if on {
                // tx_ps_onoff(1): the engine starts running again.
                connected.ps_enabled = true;
                tx.set_ps_enabled(true);
            }
        }
        changed = true;
    }
    if toggle_two_tone {
        connected.toolbar_two_tone_request = true;
    }
    if let Some(a) = auto {
        connected.ps_auto_attenuate = a;
        auto_reset();
        changed = true;
    }
    if off {
        // OFF: reset only (SetPSControl reset=1).
        connected.ps_enabled = false;
        if let Some(tx) = &connected.tx_handle {
            tx.set_ps_enabled(false);
        }
    }
    if restart {
        // Restart (ps_menu.c resume_cb): with Two Tone and Auto Attenuate the attenuation starts again from 0.
        if two_tone && connected.ps_auto_attenuate {
            att_set(connected, 0);
        }
        connected.ps_enabled = true;
        if let Some(tx) = &connected.tx_handle {
            tx.set_ps_enabled(true);
            tx.ps_calibrate();
        }
    }
    if let Some(v) = oneshot {
        connected.ps_oneshot = v;
        if let Some(tx) = &connected.tx_handle {
            tx.set_ps_oneshot(v);
            tx.ps_calibrate(); // ps_off_on()
        }
        changed = true;
    }
    if let Some(m) = stability {
        if m != connected.ps_stability {
            connected.ps_stability = m;
            if let Some(tx) = &connected.tx_handle {
                tx.set_ps_tolerance(m);
                tx.ps_calibrate(); // ps_off_on()
            }
            changed = true;
        }
    }
    if let Some(p) = new_peak {
        let p = p.clamp(0.01, 1.01);
        connected.ps_hw_peak = p;
        if let Some(tx) = &connected.tx_handle {
            tx.set_ps_hw_peak(p);
            tx.ps_calibrate(); // ps_off_on()
        }
        changed = true;
    }
    if let Some(v) = new_att {
        att_set(connected, v);
        changed = true;
    }
    (close_now, changed)
}

// ---- Auto Attenuate (ps_menu.c ps_calibration_timer) -----------------------------------------------------------------

/// State of the Auto Attenuate timer (one radio at a time, so one static is enough).
struct Auto {
    last_tick: Option<Instant>,
    old_level: i32,
    count: u32,
}

static AUTO: Mutex<Auto> = Mutex::new(Auto { last_tick: None, old_level: -1, count: 0 });

fn auto_reset() {
    let mut a = AUTO.lock().unwrap();
    a.old_level = -1;
    a.count = 0;
}

/// Every frame. While PureSignal is on, Two Tone is transmitting and Auto Attenuate is on, every 100 ms (deskHPSDR's timer):
/// when the feedback level has a new value (or after more than 10 ticks) and is outside 150..155, the attenuation moves by
/// 20*log10(level/152.293) (+-15 dB for a very strong / very weak level) and the calibration is restarted.
pub(crate) fn auto_tick(connected: &mut ConnectedState, ctx: &egui::Context) {
    if !connected.ps_auto_attenuate || !connected.puresignal_enabled || !connected.two_tone_active || !connected.session.mox_active() {
        return;
    }
    let Some(level) = connected.tx_handle.as_ref().map(|t| t.ps_status.lock().unwrap().feedback_level) else { return };
    ctx.request_repaint_after(Duration::from_millis(100));
    let mut a = AUTO.lock().unwrap();
    if a.last_tick.is_some_and(|t| t.elapsed() < Duration::from_millis(100)) {
        return;
    }
    a.last_tick = Some(Instant::now());
    let newcal = if level != a.old_level || a.count > 10 {
        a.old_level = level;
        a.count = 0;
        true
    } else {
        a.count += 1;
        false
    };
    drop(a);
    let (lo, hi) = att_range(connected);
    let att = att_get(connected);
    if newcal && ((level > 155 && att < hi) || (level < 150 && att > lo)) {
        let delta = if level > 275 {
            15 + if att < -15 { 15 } else { 0 }
        } else if level < 25 {
            -15
        } else {
            (20.0 * (level as f64 / 152.293).log10()).round() as i32
        };
        let new_att = (att + delta).clamp(lo, hi);
        if new_att != att {
            att_set(connected, new_att);
            if let Some(tx) = &connected.tx_handle {
                tx.ps_calibrate(); // reset, then resume
            }
        }
    }
}
