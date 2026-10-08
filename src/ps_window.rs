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

/// Gap between the bottom of the PS menu / bar and the top of the toolbar, in px (the toolbar height and margin are added to it).
const BOTTOM_GAP_PX: f32 = 10.0;

/// The menu is reduced to a small bar (button "Hide") so the spectrum and the waterfall can be seen.
static HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Opens in the small bar size (the toolbar / MIDI "PS" key).
pub(crate) fn show_bar() {
    HIDDEN.store(true, Ordering::Relaxed);
}

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
const NOISE_HELP: &str = "Transmit band-limited Gaussian noise at the selected generator level (-12 to +3 dB), a calibration signal for PureSignal like Two Tone. The normal TX filter limits the noise to the transmit passband. Use short test periods: the average PA power is high. Changes of the level take effect at once.";
const MON_HELP: &str = "Show the received PureSignal feedback signal (what comes back from the amplifier) on the TX spectrum instead of the transmitted signal. Use it with OFF and Restart to see the effect of the correction. This only changes the display and does not affect the correction.";
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
    value_box_w(ui, text, color, BOX_W);
}

/// A read-only value box of a given width (the small bar uses narrower ones).
fn value_box_w(ui: &mut egui::Ui, text: &str, color: egui::Color32, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
    ui.painter().rect(rect, 5.0, egui::Color32::from_gray(22), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(20.0), color);
}

/// Two Tone pressed: Two Tone and Noise exclude each other; with the noise running it is stopped first and Two Tone starts after it.
fn two_tone_pressed(connected: &mut ConnectedState) {
    if connected.noise_active {
        connected.noise_request = Some(false);
        connected.two_tone_after_noise = true;
    } else {
        connected.toolbar_two_tone_request = true;
    }
}

/// OFF: reset only (SetPSControl reset=1), the correction stops being applied.
fn do_off(connected: &mut ConnectedState) {
    connected.ps_enabled = false;
    if let Some(tx) = &connected.tx_handle {
        tx.set_ps_enabled(false);
    }
}

/// Restart (ps_menu.c resume_cb): with Two Tone and Auto Attenuate the attenuation starts again from 0.
fn do_restart(connected: &mut ConnectedState) {
    if connected.two_tone_active && connected.ps_auto_attenuate {
        att_set(connected, 0);
        auto_reset();
    }
    connected.ps_enabled = true;
    if let Some(tx) = &connected.tx_handle {
        tx.set_ps_enabled(true);
        tx.ps_calibrate();
    }
}

/// MON: the TX spectrum shows the feedback (display only, saved).
fn do_mon(connected: &mut ConnectedState, on: bool) {
    connected.ps_mon = on;
    connected.settings_dirty.store(true, Ordering::Relaxed);
}

/// Returns (close, changed).
pub fn ps_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mut close_now = false;
    let mut changed = false;
    let status = connected.tx_handle.as_ref().map(|t| *t.ps_status.lock().unwrap()).unwrap_or_default();
    let ps_on = connected.puresignal_enabled;
    // The engine only updates its readings while keyed, so they are shown only then (the last values would otherwise stay on screen).
    let tx_live = ps_on && connected.session.mox_active();
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
    let mut mon: Option<bool> = None;
    let mut noise_toggle = false;
    let mut noise_level: Option<i32> = None;
    let noise_now = connected.noise_active;
    let noise_db = connected.noise_level_db;
    let mon_now = connected.ps_mon;
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
            // No window shadow: it would darken the top of the toolbar this window sits right above (same as the NEW MENU).
            .frame(egui::Frame::window(ui.style()).corner_radius(0.0).shadow(egui::Shadow::NONE))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -(crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN + BOTTOM_GAP_PX)))
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                ui.horizontal(|ui| {
                    if crate::kiosk_accent_button(ui, "Show").clicked() {
                        show = true;
                    }
                    value_box_w(ui, &if tx_live { format!("{}", status.feedback_level) } else { String::new() }, if tx_live { level_color(status.feedback_level) } else { egui::Color32::from_gray(120) }, 80.0);
                    let (txt, col) = if !tx_live {
                        ("", egui::Color32::from_gray(120))
                    } else if status.correcting {
                        ("Correcting", egui::Color32::from_rgb(80, 200, 80))
                    } else {
                        ("no corr.", egui::Color32::from_rgb(230, 70, 70))
                    };
                    value_box(ui, txt, col);
                    value_box_w(ui, &format!("ATT {att}"), egui::Color32::WHITE, 110.0);
                    let b = egui::Button::new(egui::RichText::new("Two Tone").color(if two_tone { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if two_tone { egui::Color32::from_rgb(230, 140, 20) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(5.0);
                    if ui.add(b).clicked() {
                        tt = true;
                    }
                    let nbb = egui::Button::new(egui::RichText::new("Noise").color(if noise_now { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if noise_now { egui::Color32::from_rgb(230, 140, 20) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(80.0, 40.0))
                        .corner_radius(5.0);
                    if ui.add(nbb).clicked() {
                        noise_toggle = true;
                    }
                    let mb = egui::Button::new(egui::RichText::new("MON").color(if mon_now { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                        .fill(if mon_now { egui::Color32::from_rgb(70, 150, 245) } else { egui::Color32::from_gray(48) })
                        .min_size(egui::vec2(70.0, 40.0))
                        .corner_radius(5.0);
                    if ui.add(mb).clicked() {
                        mon = Some(!mon_now);
                    }
                    if ui.add_enabled(ps_on, egui::Button::new("OFF").min_size(egui::vec2(60.0, 40.0))).clicked() {
                        off = true;
                    }
                    if ui.add_enabled(ps_on, egui::Button::new("Restart").min_size(egui::vec2(90.0, 40.0))).clicked() {
                        restart = true;
                    }
                });
            });
        if show {
            HIDDEN.store(false, Ordering::Relaxed);
        }
        if tt {
            two_tone_pressed(connected);
        }
        if off {
            do_off(connected);
        }
        if restart {
            do_restart(connected);
        }
        if let Some(v) = mon {
            do_mon(connected, v);
        }
        if noise_toggle {
            connected.noise_request = Some(!noise_now);
        }
        return (false, false);
    }
    let mut hide_now = false;
    egui::Window::new("ps_menu")
        .id(egui::Id::new("ps_menu_window"))
        .title_bar(false)
        .frame(egui::Frame::window(ui.style()).corner_radius(0.0).shadow(egui::Shadow::NONE))
        .collapsible(false)
        .resizable(false)
        // Same placement rule as the AGC / DSP menus: centred, anchored above the toolbar with a gap.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -(crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN + BOTTOM_GAP_PX)))
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
                ui.add_space(24.0);
                let nb = egui::Button::new(egui::RichText::new("Noise").color(if noise_now { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                    .fill(if noise_now { egui::Color32::from_rgb(230, 140, 20) } else { egui::Color32::from_gray(48) })
                    .min_size(egui::vec2(100.0, ROW_H))
                    .corner_radius(5.0);
                if ui.add(nb).clicked() {
                    noise_toggle = true;
                }
                let mut nv = noise_db as f64;
                if spin_buttons_full(ui, "ps_noise_level", &mut nv, -12.0, 3.0, 1.0, 0, None, 52.0).changed() {
                    noise_level = Some(nv.round() as i32);
                }
                help_button(ui, "ps_help_noise", NOISE_HELP);
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
                let mb = egui::Button::new(egui::RichText::new("MON").color(if mon_now { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                    .fill(if mon_now { egui::Color32::from_rgb(70, 150, 245) } else { egui::Color32::from_gray(48) })
                    .min_size(egui::vec2(80.0, 40.0))
                    .corner_radius(5.0);
                if ui.add(mb).clicked() {
                    mon = Some(!mon_now);
                }
                help_button(ui, "ps_help_mon", MON_HELP);
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
                value_box(ui, &if tx_live { format!("{}", status.feedback_level) } else { String::new() }, if tx_live { level_color(status.feedback_level) } else { egui::Color32::from_gray(120) });
                label(ui, "Correcting", LABEL_W);
                let (txt, col) = if !tx_live {
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
                value_box(ui, &if tx_live { status.feedback_level.to_string() } else { String::new() }, g);
                label(ui, "cor.cnt", LABEL_W);
                value_box(ui, &if tx_live { status.cal_count.to_string() } else { String::new() }, g);
                label(ui, "sln.chk", LABEL_W);
                value_box(ui, &if tx_live { status.solution_check.to_string() } else { String::new() }, g);
            });
            // Row 5: status | TX ATT (the TX ATT used to have a row of its own; this row had nothing on the right).
            ui.horizontal(|ui| {
                label(ui, "status", LABEL_W);
                value_box(ui, &if tx_live { crate::tx::ps_state_name(status.state).to_string() } else { String::new() }, egui::Color32::from_gray(225));
                label(ui, "TX ATT", LABEL_W);
                if auto_on {
                    // Automatic: shown, not editable (deskHPSDR shows an entry instead of the spin).
                    value_box(ui, &if ps_on { att.to_string() } else { String::new() }, egui::Color32::from_gray(225));
                } else if spin_buttons_full(ui, "ps_txatt", &mut att_value, att_lo as f64, att_hi as f64, 1.0, 0, None, 78.0).changed() {
                    new_att = Some(att_value.round() as i32);
                }
                help_button(ui, "ps_help_att", ATT_HELP);
            });

            // Row 6: GetPk | SetPk.
            ui.horizontal(|ui| {
                label(ui, "GetPk", LABEL_W);
                value_box(ui, &if tx_live { format!("{:.3}", status.max_tx) } else { String::new() }, egui::Color32::from_gray(225));
                help_button(ui, "ps_help_peak", PEAK_HELP);
                ui.add_space(8.0);
                label(ui, "SetPk", 90.0);
                if spin_buttons_full(ui, "ps_setpk", &mut peak_value, 0.01, 1.01, 0.001, 3, None, 78.0).changed() {
                    new_peak = Some(peak_value);
                }
            });
        });

    if hide_now {
        HIDDEN.store(true, Ordering::Relaxed);
    }
    if close_now {
        HIDDEN.store(false, Ordering::Relaxed);
        // ps_menu.c cleanup(): closing the menu stops Two Tone and Noise.
        if connected.noise_active {
            connected.noise_request = Some(false);
        }
        if connected.two_tone_active {
            connected.toolbar_two_tone_request = true;
        }
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
        if on {
            maybe_warn(connected);
        }
        changed = true;
    }
    if toggle_two_tone {
        two_tone_pressed(connected);
    }
    if noise_toggle {
        connected.noise_request = Some(!noise_now);
    }
    if let Some(v) = noise_level {
        connected.noise_level_db = v.clamp(-12, 3);
        connected.settings_dirty.store(true, Ordering::Relaxed);
    }
    if let Some(a) = auto {
        connected.ps_auto_attenuate = a;
        auto_reset();
        changed = true;
    }
    if off {
        do_off(connected);
    }
    if restart {
        do_restart(connected);
    }
    if let Some(v) = mon {
        do_mon(connected, v);
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
    /// cor.cnt and time of the last attenuation change: the next decision waits for a new calibration (or 4 s).
    cnt_at_change: i32,
    changed_at: Option<Instant>,
}

static AUTO: Mutex<Auto> = Mutex::new(Auto { last_tick: None, old_level: -1, count: 0, cnt_at_change: -1, changed_at: None });

fn auto_reset() {
    let mut a = AUTO.lock().unwrap();
    a.old_level = -1;
    a.count = 0;
    a.changed_at = None;
}

/// Every frame. While PureSignal is on, Two Tone is transmitting and Auto Attenuate is on, every 100 ms (deskHPSDR's timer):
/// when the feedback level has a new value (or after more than 10 ticks) and is outside 150..155, the attenuation moves by
/// 20*log10(level/152.293) (+-15 dB for a very strong / very weak level) and the calibration is restarted.
pub(crate) fn auto_tick(connected: &mut ConnectedState, ctx: &egui::Context) {
    if !connected.ps_auto_attenuate || !connected.puresignal_enabled || !connected.two_tone_active || !connected.session.mox_active() {
        return;
    }
    let Some((level, cal_cnt)) = connected.tx_handle.as_ref().map(|t| {
        let s = t.ps_status.lock().unwrap();
        (s.feedback_level, s.cal_count)
    }) else {
        return;
    };
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
    // After a change, the old level is stale until the engine has finished a new calibration: wait for cor.cnt to move (or 4 s).
    let fresh = a.changed_at.map_or(true, |t| cal_cnt != a.cnt_at_change || t.elapsed() > Duration::from_secs(4));
    let newcal = newcal && fresh;
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
            {
                let mut a = AUTO.lock().unwrap();
                a.cnt_at_change = cal_cnt;
                a.changed_at = Some(Instant::now());
            }
            if let Some(tx) = &connected.tx_handle {
                tx.ps_calibrate(); // reset, then resume
            }
        }
    }
}

// ---- Warning: PureSignal switched on with 0 dB of TX attenuation (ps_menu.c ps_zero_att_warning_show) -----------------------

/// Shows the warning when PureSignal is on with 0 dB of attenuation and "Don't show this warning again" has not been ticked.
pub(crate) fn maybe_warn(connected: &mut ConnectedState) {
    if att_get(connected) == 0 && !connected.ps_hide_zero_att {
        connected.ps_zero_att_popup = true;
    }
}

/// The warning window (drawn every frame, on top of everything).
pub(crate) fn warning_window(ui: &mut egui::Ui, connected: &mut ConnectedState) {
    if !connected.ps_zero_att_popup {
        return;
    }
    let dont_id = egui::Id::new("ps_zero_att_dont_show");
    let mut dont: bool = ui.ctx().data(|d| d.get_temp(dont_id)).unwrap_or(false);
    let mut ok = false;
    egui::Window::new("ps_zero_att_warning")
        .id(egui::Id::new("ps_zero_att_warning_win"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            ui.set_max_width(600.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("PureSignal TX Attenuation").color(egui::Color32::from_rgb(230, 40, 40)).strong().size(22.0));
            });
            ui.add_space(8.0);
            ui.label("TX attenuation is currently set very low (0 dB).");
            ui.add_space(6.0);
            ui.label("Please verify that the PureSignal feedback level is appropriate or execute a new PS calibration.");
            ui.add_space(6.0);
            ui.label("A feedback level that is too high may cause ADC overload.");
            ui.add_space(6.0);
            ui.label("Note: External attenuation in the feedback path may make 0 dB a valid setting.");
            ui.add_space(10.0);
            std_checkbox(ui, &mut dont, "Don't show this warning again");
            ui.add_space(10.0);
            if ui.add(egui::Button::new("OK").min_size(egui::vec2(560.0, 44.0))).clicked() {
                ok = true;
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(dont_id, dont));
    if ok {
        connected.ps_zero_att_popup = false;
        if dont {
            connected.ps_hide_zero_att = true;
            connected.settings_dirty.store(true, Ordering::Relaxed);
        }
        ui.ctx().data_mut(|d| d.insert_temp(dont_id, false));
    }
}
