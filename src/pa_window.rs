//! The "PA" window (deskHPSDR pa_menu.c, "Calibrate" page): full-screen, MAX Power, Transmit out of band, Save / Import
//! (deskHPSDR .props files) and the per-band PA calibration (dB) as spin buttons. The values are the same
//! `ConnectedState::pa_calibration` entries the old Settings -> PA Calibration tab edits (by band name).

use crate::noise_window::choice_combo_h;
use crate::tx_window::{child, new_row, spin};
use crate::{help_button, kiosk_accent_button, touch_checkbox, ConnectedState};
use std::sync::atomic::Ordering;

const ROW_H: f32 = 44.0;
/// Rows holding 40 px chips/buttons (10 px gap below).
const TAB_ROW_H: f32 = 50.0;
/// deskHPSDR band.c clamps every calibration to this range.
pub(crate) const PA_MIN: f32 = 38.8;
pub(crate) const PA_MAX: f32 = 70.0;
const MAX_POWER_LIST: [u32; 14] = [1, 5, 10, 15, 20, 25, 30, 50, 75, 100, 125, 200, 500, 1000];
/// deskHPSDR's band enum: index of the first transverter slot (BANDS) and of "Gen".
const XVTR_BASE: usize = 24;
const GEN_INDEX: usize = 23;
/// (name used as key in `pa_calibration`, deskHPSDR band index). The first 12 are what a Hermes Lite reaches (radio_max_band).
const BAND_TABLE: [(&str, usize); 14] = [
    ("136kHz", 0),
    ("472kHz", 1),
    ("160m", 2),
    ("80m", 3),
    ("60m", 4),
    ("40m", 5),
    ("30m", 6),
    ("20m", 7),
    ("17m", 8),
    ("15m", 9),
    ("12m", 10),
    ("10m", 11),
    ("8m", 12),
    ("6m", 13),
];

const HL2_HELP: &str = "Hermes Lite 2 or compatible SDR devices:\n1. Set all bands to a value of 38.8 and MAX Power to 5W for full 5W output!\n2. Set PA enable in SDR Device!";
const OOB_HELP: &str = "Off (default): TX is blocked outside the defined ham band allocations, e.g. on \"Gen\". Enable only for MARS/CAP or other explicitly authorized out-of-band operation -- this does not check any regulatory database, it only removes this app's own safety check.";

pub(crate) fn clamp_db(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(PA_MIN, PA_MAX)
    } else {
        PA_MIN
    }
}

/// Every stored calibration clamped to deskHPSDR's range (used when the config is loaded).
pub(crate) fn clamped(map: &std::collections::HashMap<String, f32>) -> std::collections::HashMap<String, f32> {
    map.iter().map(|(k, v)| (k.clone(), clamp_db(*v))).collect()
}

fn is_hl2(connected: &ConnectedState) -> bool {
    matches!(connected.device.board, crate::Boards::HermesLite | crate::Boards::HermesLite2)
}

/// The bands shown, in deskHPSDR order: (key, title, deskHPSDR index).
fn visible_bands(connected: &ConnectedState, with_gen: bool) -> Vec<(String, String, usize)> {
    let max = if is_hl2(connected) { 12 } else { BAND_TABLE.len() };
    let mut v: Vec<(String, String, usize)> = Vec::new();
    if with_gen {
        v.push(("Gen".to_string(), "Gen".to_string(), GEN_INDEX));
    }
    for (name, idx) in BAND_TABLE.iter().take(max) {
        v.push((name.to_string(), name.to_string(), *idx));
    }
    for (i, x) in connected.xvtrs.iter().enumerate() {
        if !x.name.is_empty() {
            v.push((x.name.clone(), x.name.clone(), XVTR_BASE + i));
        }
    }
    v
}

/// Every band that has a title (what deskHPSDR saves/loads): the table, Gen and the named XVTR slots.
fn all_bands(connected: &ConnectedState) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = BAND_TABLE.iter().map(|(n, i)| (n.to_string(), *i)).collect();
    v.push(("Gen".to_string(), GEN_INDEX));
    for (i, x) in connected.xvtrs.iter().enumerate() {
        if !x.name.is_empty() {
            v.push((x.name.clone(), XVTR_BASE + i));
        }
    }
    v
}

fn value_of(connected: &ConnectedState, key: &str) -> f32 {
    clamp_db(connected.pa_calibration.get(key).copied().unwrap_or(crate::radio::DEFAULT_PA_GAIN_DB))
}

fn props_name(connected: &ConnectedState) -> String {
    let name: String = connected.device.board_label().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!("pa_calibration_{name}.props")
}

fn save(connected: &ConnectedState) -> bool {
    let Some(dir) = crate::config::settings_dir() else { return false };
    let mut text = String::new();
    for (key, idx) in all_bands(connected) {
        text.push_str(&format!("band.{}.pa_calibration={:.1}\n", idx, value_of(connected, &key)));
    }
    std::fs::write(dir.join(props_name(connected)), text).is_ok()
}

/// Reads a deskHPSDR pa_calibration .props file; unknown keys are ignored, values clamped. True when the file could be read.
fn import(connected: &mut ConnectedState, path: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let bands = all_bands(connected);
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else { continue };
        let Some(idx) = k.trim().strip_prefix("band.").and_then(|r| r.strip_suffix(".pa_calibration")).and_then(|n| n.parse::<usize>().ok()) else {
            continue;
        };
        let Ok(val) = v.trim().parse::<f32>() else { continue };
        if let Some((key, _)) = bands.iter().find(|(_, i)| *i == idx) {
            connected.pa_calibration.insert(key.clone(), clamp_db(val));
        }
    }
    true
}

fn list_props() -> Vec<std::path::PathBuf> {
    let Some(dir) = crate::config::settings_dir() else { return Vec::new() };
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map(|e| e == "props").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Returns (close, changed).
pub fn pa_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let popup_id = egui::Id::new("pa_window_popup");
    let msg_id = egui::Id::new("pa_window_msg");

    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
    egui::Window::new("PA Calibration")
        .id(egui::Id::new("pa_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .fixed_pos(screen.min)
        .constrain_to(screen)
        .fixed_size(screen.size() - egui::vec2(58.0, 10.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            let w = screen.width() - 58.0;
            ui.set_width(w);
            ui.set_min_height(screen.height() - 10.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - PA Calibration", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

            // Header row; CLOSE (96 px wide, 24 px from the screen edge) takes the right end, so the controls stop at 866.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                c.label("MAX Power");
                let sel = MAX_POWER_LIST.iter().position(|v| *v == connected.max_tx_power_watts);
                let mut labels: Vec<String> = MAX_POWER_LIST.iter().map(|v| if *v == 1000 { "1KW".to_string() } else { format!("{v}W") }).collect();
                if sel.is_none() {
                    // A value from the old slider that is not in the list: shown as it is, until another entry is picked.
                    labels.push(format!("{}W", connected.max_tx_power_watts));
                }
                let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
                if let Some(i) = choice_combo_h(&mut c, "pa_max_power", 90.0, sel.unwrap_or(MAX_POWER_LIST.len()), &items, 40.0) {
                    if let Some(v) = MAX_POWER_LIST.get(i) {
                        connected.max_tx_power_watts = *v;
                        let capped = connected.session.tx_power_watts.load(Ordering::Relaxed).min(*v);
                        connected.session.tx_power_watts.store(capped, Ordering::Relaxed);
                        changed = true;
                    }
                }
                let mut oob = connected.allow_out_of_band_tx.load(Ordering::Relaxed);
                if touch_checkbox(&mut c, &mut oob, "Transmit out of band").changed() {
                    connected.allow_out_of_band_tx.store(oob, Ordering::Relaxed);
                    changed = true;
                }
                help_button(&mut c, "pa_oob", OOB_HELP);
                if c.add(egui::Button::new(egui::RichText::new("Save").strong()).min_size(egui::vec2(90.0, 40.0))).clicked() {
                    let ok = save(connected);
                    c.ctx().data_mut(|d| d.insert_temp(msg_id, if ok { "PA calibration saved" } else { "ERROR: PA calibration not saved" }.to_string()));
                }
                if c.add(egui::Button::new(egui::RichText::new("Import").strong()).min_size(egui::vec2(110.0, 40.0))).clicked() {
                    c.ctx().data_mut(|d| d.insert_temp(popup_id, true));
                }
            }
            egui::Area::new(egui::Id::new("pa_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(4.0);

            // Page tabs ("Calibrate" | "Linearization"; the last page is remembered) with the Hermes Lite hint behind a "!".
            let page_id = egui::Id::new("pa_window_page");
            let mut page: u8 = ui.ctx().data(|d| d.get_temp::<u8>(page_id)).unwrap_or(0);
            let row = new_row(ui, w, TAB_ROW_H);
            {
                let mut c = child(ui, row, 0.0, 700.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                for (i, title, cw) in [(0u8, "Calibrate", 150.0), (1u8, "Linearization", 170.0)] {
                    if c.add(crate::chip_button(title, page == i).min_size(egui::vec2(cw, 40.0))).clicked() {
                        page = i;
                    }
                }
                if page == 0 && is_hl2(connected) && !connected.device.is_radioberry {
                    help_button(&mut c, "pa_hl2", HL2_HELP);
                }
            }
            if page != 1 {
                // Leaving the Linearization page ends a running measurement (TUNE released).
                measure_abort(connected, "Left the Linearization page");
            }
            ui.ctx().data_mut(|d| d.insert_temp(page_id, page));

            if page == 1 {
                if linearization_page(ui, connected, w) {
                    changed = true;
                }
            } else {
                // The bands: column pairs (label + spin), filled top to bottom like deskHPSDR.
                let with_gen = connected.allow_out_of_band_tx.load(Ordering::Relaxed);
                let bands = visible_bands(connected, with_gen);
                let n = bands.len();
                let cols = if n <= 12 { 2 } else { 3 };
                let rows = if n <= 12 { 6 } else { n.div_ceil(cols) };
                let pitch = w / cols as f32;
                for r in 0..rows {
                    let row = new_row(ui, w, ROW_H);
                    for col in 0..cols {
                        let Some((key, title, _)) = bands.get(col * rows + r) else { continue };
                        let x0 = pitch * col as f32;
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let color = ui.visuals().text_color();
                        ui.painter().text(egui::pos2(row.left() + x0 + 4.0, row.center().y), egui::Align2::LEFT_CENTER, title, font, color);
                        let mut v = value_of(connected, key) as f64;
                        let sx = x0 + 96.0;
                        if spin(ui, row, sx, sx + 98.0 + 110.0, &format!("pa_cal_{key}"), &mut v, PA_MIN as f64, PA_MAX as f64, 0.1, 1) {
                            connected.pa_calibration.insert(key.clone(), clamp_db(((v * 10.0).round() / 10.0) as f32));
                            changed = true;
                        }
                    }
                }
            }
        });

    if close_now {
        measure_abort(connected, "PA window closed");
    }

    // Import: in-app file list (no native dialog on the touch panel).
    if ui.ctx().data(|d| d.get_temp::<bool>(popup_id)).unwrap_or(false) {
        let mut close = false;
        egui::Window::new("pa_import_files")
            .id(egui::Id::new("pa_import_files"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(520.0);
                ui.label(egui::RichText::new("Import deskHPSDR PA Calibration").strong());
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for path in list_props() {
                        let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::Button::new(&fname).min_size(egui::vec2(500.0, 40.0))).clicked() {
                            let ok = import(connected, &path);
                            if ok {
                                changed = true;
                            }
                            ui.ctx().data_mut(|d| d.insert_temp(msg_id, if ok { "PA calibration loaded" } else { "ERROR: PA calibration not loaded" }.to_string()));
                            close = true;
                        }
                    }
                });
                ui.add_space(6.0);
                if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(110.0, 40.0))).clicked() {
                    close = true;
                }
            });
        if close {
            ui.ctx().data_mut(|d| d.insert_temp(popup_id, false));
        }
    }

    // Confirmation message.
    if let Some(msg) = ui.ctx().data(|d| d.get_temp::<String>(msg_id)) {
        let mut ok = false;
        egui::Window::new("pa_message")
            .id(egui::Id::new("pa_message"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(300.0);
                ui.label(&msg);
                ui.add_space(8.0);
                if ui.add(egui::Button::new("OK").min_size(egui::vec2(110.0, 40.0))).clicked() {
                    ok = true;
                }
            });
        if ok {
            ui.ctx().data_mut(|d| d.remove::<String>(msg_id));
        }
    }
    (close_now, changed)
}

// ---------------------------------------------------------------------------------------------------------------------
// "Linearization" page: the per-band drive-level correction (dB at 10%..90% of MAX Power), the same
// `ConnectedState::pa_drive_adjust` data (key = band name, [f32; 9], range -20..=20 dB, step 0.1, Reset = entry removed)
// the old Settings -> PA Calibration tab edits, plus the measurement assistant.
//
// Height budget (1024x600 kiosk; window content = 590 px minus 8 px frame margin, minus title 22 + header 54 + gap 4
// = about 500 px): tabs 50 + band chips 50 + 3 table rows 3x44 = 132 + Reset row 50 + gap 10 + measure rows 50 + 44 + 44
// = 430 px. Width: 966 px (24 px side margins), 11 chips of 76 px with 13 px gaps, table cells 322 px
// (label 130 + spin 98 + 78 box + 16 px gap).
// ---------------------------------------------------------------------------------------------------------------------

const ADJ_MIN: f64 = -20.0;
const ADJ_MAX: f64 = 20.0;
/// Hard limit of one keyed point.
const POINT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const MEASURE_HELP: &str = "Measurement assistant: connect a DUMMY LOAD and an external wattmeter to the antenna output first. The transmitter is keyed (TUNE) at 10% to 90% of MAX Power, one point at a time, for at most 20 s each, with the FULL power of that point (not Tune Power). Read the external wattmeter, enter the value and press Next: the correction of that point is stored. Cancel, closing the window, leaving this page, an SWR trip or any error releases TX at once.";

/// State of the measurement assistant (one field of ConnectedState). `active` = a run is in progress (keyed or waiting for Key).
pub(crate) struct Measure {
    active: bool,
    /// Message shown after the run ended or a start was refused.
    ended: Option<String>,
    band: String,
    point: usize,
    /// Commanded power of the point (integer watts, the resolution of the TX power setting).
    watts: u32,
    /// TX power to restore after every unkey.
    saved_watts: u32,
    /// Correction (dB) that was applied while the point was keyed: the base of the new value.
    applied_db: f32,
    keyed_at: Option<std::time::Instant>,
    /// Externally measured watts.
    measured: f64,
    /// Last frame the page was drawn (the tick releases TX when this is stale).
    last_ui: std::time::Instant,
}

impl Measure {
    fn idle() -> Self {
        Measure {
            active: false,
            ended: None,
            band: String::new(),
            point: 0,
            watts: 0,
            saved_watts: 0,
            applied_db: 0.0,
            keyed_at: None,
            measured: 0.0,
            last_ui: std::time::Instant::now(),
        }
    }
}

/// Commanded watts of point `k` (0..9 = 10%..90%): integer (the TX power setting is whole watts), 1..=max.
fn point_watts(max: u32, k: usize) -> u32 {
    (((max as f32) * (k as f32 + 1.0) / 10.0).round() as u32).clamp(1, max.max(1))
}

/// True when point `k` is a whole number of watts at this MAX Power. The TX power setting is whole watts, so only such
/// points can be commanded exactly; the others (e.g. 0.5 W of a 5 W radio) are left to the manual spins.
fn point_exact(max: u32, k: usize) -> bool {
    (max * (k as u32 + 1)) % 10 == 0
}

/// First point at or after `from` that the assistant can measure (None when there is none left).
fn next_exact_point(max: u32, from: usize) -> Option<usize> {
    (from..9).find(|&k| point_exact(max, k))
}

/// Releases TX: TUNE off and the TX power back to its saved value. No-op when not keyed by the assistant.
fn unkey(connected: &mut ConnectedState) {
    let Some(m) = connected.pa_measure.as_ref() else { return };
    if m.keyed_at.is_none() {
        return;
    }
    let saved = m.saved_watts;
    if connected.tune_active {
        crate::tune_set(connected, false);
    } else {
        connected.session.set_mox(false);
        if let Some(tx) = &connected.tx_handle {
            tx.set_tune(false);
        }
    }
    connected.pre_tune_power_watts = None;
    connected.session.tx_power_watts.store(saved, Ordering::Relaxed);
    if let Some(m) = connected.pa_measure.as_mut() {
        m.keyed_at = None;
    }
}

/// Ends the run (releasing TX first) with a message. Does nothing when no run is active.
pub(crate) fn measure_abort(connected: &mut ConnectedState, why: &str) {
    if !connected.pa_measure.as_ref().is_some_and(|m| m.active) {
        return;
    }
    unkey(connected);
    if let Some(m) = connected.pa_measure.as_mut() {
        m.active = false;
        m.ended = Some(why.to_string());
    }
}

/// Keys TUNE at the full power of the current point. Err(reason) when it must not transmit.
fn key_point(connected: &mut ConnectedState) -> Result<(), String> {
    let Some(m) = connected.pa_measure.as_ref() else { return Err("No measurement".into()) };
    let band = m.band.clone();
    let point = m.point;
    if !connected.tx_enabled || connected.tx_handle.is_none() {
        return Err("Transmit is not enabled".into());
    }
    if connected.session.hardware_tx_inhibit.load(Ordering::Relaxed) {
        return Err("TX inhibit input is asserted".into());
    }
    if connected.session.mox_active() || connected.tune_active || connected.two_tone_active || connected.cw_text_sending {
        return Err("The radio is already transmitting".into());
    }
    let freq = connected.session.tx_frequency_hz.load(Ordering::Relaxed);
    if !crate::tx_frequency_allowed(freq, connected.allow_out_of_band_tx.load(Ordering::Relaxed)) {
        return Err("TX frequency is outside the ham bands".into());
    }
    match crate::band_for_frequency(freq) {
        Some(b) if b.name == band => {}
        _ => return Err(format!("Tune to the {band} band first")),
    }
    let max = connected.max_tx_power_watts.max(1);
    let watts = point_watts(max, point);
    let saved = connected.session.tx_power_watts.load(Ordering::Relaxed).min(max);
    let applied = crate::resolved_pa_drive_adjust_db(&connected.pa_drive_adjust, freq, watts, max);
    // The drive gain for the new power is stored before keying (the per-frame update would lag one frame behind).
    let gain = crate::resolved_pa_gain_db(&connected.pa_calibration, freq) - applied;
    connected.session.pa_gain_db.store(gain.to_bits(), Ordering::Relaxed);
    connected.pre_tune_power_watts = Some(saved);
    connected.session.tx_power_watts.store(watts, Ordering::Relaxed);
    if let Some(tx) = &connected.tx_handle {
        tx.set_tune(true);
    }
    connected.session.set_mox(true);
    connected.tune_active = true;
    if let Some(m) = connected.pa_measure.as_mut() {
        m.saved_watts = saved;
        m.watts = watts;
        m.applied_db = applied;
        m.keyed_at = Some(std::time::Instant::now());
        m.ended = None;
    }
    Ok(())
}

/// Every frame (main.rs): releases TX when anything about the run is no longer valid.
pub(crate) fn measure_tick(connected: &mut ConnectedState) {
    let Some(m) = connected.pa_measure.as_ref() else { return };
    if !m.active {
        return;
    }
    let keyed = m.keyed_at.is_some();
    let mut reason: Option<&str> = None;
    if !connected.pa_window_open {
        reason = Some("PA window closed");
    } else if m.last_ui.elapsed() > std::time::Duration::from_millis(1500) {
        reason = Some("Left the Linearization page");
    } else if !connected.tx_enabled || connected.tx_handle.is_none() {
        reason = Some("Transmit is not available");
    } else if connected.session.hardware_tx_inhibit.load(Ordering::Relaxed) {
        reason = Some("TX inhibit input asserted");
    } else if keyed {
        let freq = connected.session.tx_frequency_hz.load(Ordering::Relaxed);
        if !connected.tune_active || !connected.session.mox_active() {
            reason = Some("TX was stopped elsewhere");
        } else if connected.session.tx_power_watts.load(Ordering::Relaxed) != m.watts {
            reason = Some("TX power was changed (SWR protection or another control)");
        } else if !crate::tx_frequency_allowed(freq, connected.allow_out_of_band_tx.load(Ordering::Relaxed))
            || crate::band_for_frequency(freq).map(|b| b.name) != Some(m.band.as_str())
        {
            reason = Some("The TX frequency left the band");
        } else if connected.tx_ui.swr_protection && connected.smoothed_swr >= connected.max_swr {
            reason = Some("SWR protection: check the dummy load");
        }
    }
    if let Some(r) = reason {
        measure_abort(connected, r);
        return;
    }
    if keyed && m.keyed_at.is_some_and(|t| t.elapsed() >= POINT_TIMEOUT) {
        // Hard timeout: TX off, the run stays at this point (Key again, Next or Cancel).
        unkey(connected);
        if let Some(m) = connected.pa_measure.as_mut() {
            m.ended = Some("20 s limit reached: TX released".to_string());
        }
    }
}

/// The bands the drive code reads (BANDS) that the radio covers, like the old tab's grid.
fn lin_bands(connected: &ConnectedState) -> Vec<&'static str> {
    crate::BANDS
        .iter()
        .filter(|b| (b.low() as u64) >= connected.device.frequency_min && (b.high() as u64) <= connected.device.frequency_max)
        .map(|b| b.name)
        .collect()
}

fn stored_points(connected: &ConnectedState, band: &str) -> [f32; 9] {
    connected.pa_drive_adjust.get(band).copied().unwrap_or([0.0; 9])
}

/// The Linearization page below the tabs. Returns true when the configuration changed.
fn linearization_page(ui: &mut egui::Ui, connected: &mut ConnectedState, w: f32) -> bool {
    let mut changed = false;
    let ctx = ui.ctx().clone();
    let band_id = egui::Id::new("pa_lin_band");
    let reset_all_id = egui::Id::new("pa_lin_reset_all");
    let confirm_id = egui::Id::new("pa_meas_confirm");

    if connected.pa_measure.is_none() {
        connected.pa_measure = Some(Measure::idle());
    }
    if let Some(m) = connected.pa_measure.as_mut() {
        m.last_ui = std::time::Instant::now();
    }
    let measuring = connected.pa_measure.as_ref().is_some_and(|m| m.active);
    if measuring {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    let bands = lin_bands(connected);
    let cur_band = crate::band_for_frequency(connected.session.tx_frequency_hz.load(Ordering::Relaxed)).map(|b| b.name);
    let mut band: String = ctx.data(|d| d.get_temp::<String>(band_id)).filter(|b| bands.contains(&b.as_str())).unwrap_or_else(|| {
        cur_band.filter(|b| bands.contains(b)).or(bands.first().copied()).unwrap_or("").to_string()
    });
    let max = connected.max_tx_power_watts.max(1);

    // Band chips.
    let row = new_row(ui, w, TAB_ROW_H);
    for (i, name) in bands.iter().enumerate() {
        let x0 = i as f32 * 89.0;
        let mut c = child(ui, row, x0, x0 + 76.0, false, 0.0);
        let resp = c.add_enabled(!measuring, crate::chip_button(name, band == *name).min_size(egui::vec2(76.0, 40.0)));
        if resp.clicked() {
            band = name.to_string();
        }
    }
    ctx.data_mut(|d| d.insert_temp(band_id, band.clone()));

    // The 3 x 3 table (filled top to bottom): "10% (x.x W)" + dB spin.
    let mut points = stored_points(connected, &band);
    let mut edited = false;
    ui.add_enabled_ui(!measuring, |ui| {
        let pitch = w / 3.0;
        for r in 0..3 {
            let row = new_row(ui, w, ROW_H);
            for col in 0..3 {
                let idx = col * 3 + r;
                let pct = (idx + 1) * 10;
                let x0 = pitch * col as f32;
                let text = format!("{:>3}% ({:>5.1} W)", pct, max as f32 * pct as f32 / 100.0);
                let font = egui::TextStyle::Body.resolve(ui.style());
                let color = ui.visuals().text_color();
                ui.painter().text(egui::pos2(row.left() + x0 + 4.0, row.center().y), egui::Align2::LEFT_CENTER, text, font, color);
                let mut v = points[idx] as f64;
                let sx = x0 + 132.0;
                if spin(ui, row, sx, sx + 98.0 + 78.0, &format!("pa_lin_{band}_{idx}"), &mut v, ADJ_MIN, ADJ_MAX, 0.1, 1) {
                    points[idx] = (((v * 10.0).round() / 10.0) as f32).clamp(ADJ_MIN as f32, ADJ_MAX as f32);
                    edited = true;
                }
            }
        }
        // Reset row.
        let row = new_row(ui, w, TAB_ROW_H);
        let mut c = child(ui, row, 0.0, 600.0, false, 0.0);
        c.spacing_mut().item_spacing.x = 12.0;
        if c.add(egui::Button::new(format!("Reset {band}")).min_size(egui::vec2(150.0, 40.0))).clicked() {
            connected.pa_drive_adjust.remove(band.as_str());
            points = [0.0; 9];
            edited = false;
            changed = true;
        }
        if c.add(egui::Button::new("Reset all").min_size(egui::vec2(130.0, 40.0))).clicked() {
            ctx.data_mut(|d| d.insert_temp(reset_all_id, true));
        }
    });
    if edited {
        connected.pa_drive_adjust.insert(band.clone(), points);
        changed = true;
    }
    ui.add_space(10.0);

    // ---- Measure section
    let (active, keyed, point, watts, mut measured, ended) = {
        let m = connected.pa_measure.as_ref().unwrap();
        (m.active, m.keyed_at.is_some(), m.point, m.watts, m.measured, m.ended.clone())
    };
    let mut start = false;
    let mut key_again = false;
    let mut cancel = false;
    let mut next = false;
    let row = new_row(ui, w, TAB_ROW_H);
    {
        let mut c = child(ui, row, 0.0, 700.0, false, 0.0);
        c.spacing_mut().item_spacing.x = 12.0;
        c.label(egui::RichText::new("Measure").strong());
        help_button(&mut c, "pa_measure", MEASURE_HELP);
        if !active {
            if c.add(egui::Button::new(egui::RichText::new("Start").strong()).min_size(egui::vec2(110.0, 40.0))).clicked() {
                start = true;
            }
        } else {
            if !keyed && c.add(egui::Button::new("Key").min_size(egui::vec2(110.0, 40.0))).clicked() {
                key_again = true;
            }
            if c.add(egui::Button::new("Cancel").min_size(egui::vec2(110.0, 40.0))).clicked() {
                cancel = true;
            }
        }
    }
    // Status row: fixed slots.
    let row = new_row(ui, w, ROW_H);
    {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let color = ui.visuals().text_color();
        let p = ui.painter().clone();
        let y = row.center().y;
        if active {
            let target = point_watts(max, point).min(max);
            let fwd = {
                let (f, _, _) = crate::power_watts_and_swr(
                    connected.smoothed_fwd_power as u32,
                    connected.smoothed_rev_power as u32,
                    crate::power_meter_board(connected.device.board, connected.device.mac),
                );
                f
            };
            let _ = watts;
            p.text(egui::pos2(row.left() + 4.0, y), egui::Align2::LEFT_CENTER, format!("Point {}/9 ({}%)", point + 1, (point + 1) * 10), font.clone(), color);
            p.text(egui::pos2(row.left() + 220.0, y), egui::Align2::LEFT_CENTER, format!("Target {:>6.1} W", target as f32), font.clone(), color);
            let fwd_txt = if keyed { format!("App fwd {:>6.1} W", fwd) } else { "App fwd    -- W".to_string() };
            p.text(egui::pos2(row.left() + 440.0, y), egui::Align2::LEFT_CENTER, fwd_txt, font.clone(), egui::Color32::from_gray(150));
            let (txt, col) = if keyed { ("TX", egui::Color32::from_rgb(230, 50, 50)) } else { ("--", egui::Color32::from_gray(120)) };
            p.text(egui::pos2(row.left() + 680.0, y), egui::Align2::LEFT_CENTER, txt, egui::FontId::proportional(18.0), col);
            if let Some(msg) = &ended {
                p.text(egui::pos2(row.left() + 730.0, y), egui::Align2::LEFT_CENTER, msg, egui::FontId::proportional(12.0), egui::Color32::from_rgb(232, 150, 46));
            }
        } else if let Some(msg) = &ended {
            p.text(egui::pos2(row.left() + 4.0, y), egui::Align2::LEFT_CENTER, msg, font, egui::Color32::from_rgb(232, 150, 46));
        }
    }
    // Entry row: measured watts + Next.
    let row = new_row(ui, w, ROW_H);
    if active {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let color = ui.visuals().text_color();
        ui.painter().text(egui::pos2(row.left() + 4.0, row.center().y), egui::Align2::LEFT_CENTER, "Measured (W)", font, color);
        if spin(ui, row, 140.0, 140.0 + 98.0 + 110.0, "pa_meas_val", &mut measured, 0.1, 5000.0, 0.1, 1) {
            if let Some(m) = connected.pa_measure.as_mut() {
                m.measured = measured;
            }
        }
        let mut c = child(ui, row, 372.0, 372.0 + 110.0, false, -6.0);
        if c.add(egui::Button::new(egui::RichText::new("Next").strong()).min_size(egui::vec2(110.0, 40.0))).clicked() {
            next = true;
        }
    }

    // ---- Actions
    if start {
        ctx.data_mut(|d| d.insert_temp(confirm_id, true));
    }
    if cancel {
        measure_abort(connected, "Measurement cancelled");
    }
    if key_again {
        if let Err(e) = key_point(connected) {
            measure_abort(connected, &e);
        }
    }
    if next && active {
        // new_adjust = applied + 10*log10(target/measured). drive_byte_for_watts uses target_dbm = P_dBm - gain with
        // gain = cal - adjust: a PA that delivers less than commanded (measured < target) has a lower real gain than
        // assumed, so the assumed gain must drop, i.e. the adjust must RISE by 10*log10(target/measured).
        let (applied, target_w, pt) = {
            let m = connected.pa_measure.as_ref().unwrap();
            (m.applied_db as f64, m.watts as f64, m.point)
        };
        let new_adj = (applied + 10.0 * (target_w / measured.max(0.01)).log10()).clamp(ADJ_MIN, ADJ_MAX);
        let mut pts = stored_points(connected, &band);
        pts[pt] = ((new_adj * 10.0).round() / 10.0) as f32;
        connected.pa_drive_adjust.insert(band.clone(), pts);
        changed = true;
        unkey(connected);
        if let Some(m) = connected.pa_measure.as_mut() {
            match next_exact_point(max, pt + 1) {
                None => {
                    m.active = false;
                    m.ended = Some("Complete (points that are not whole watts: use the spins)".to_string());
                }
                Some(nx) => {
                    m.point = nx;
                    m.watts = point_watts(max, nx);
                    m.measured = m.watts as f64;
                    m.ended = Some("Stored. Press Key for the next point".to_string());
                }
            }
        }
    }

    // Confirmation (dummy load) before the first key-down.
    if ctx.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false) {
        let mut go = false;
        let mut no = false;
        egui::Window::new("pa_meas_confirm")
            .id(egui::Id::new("pa_meas_confirm_win"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(&ctx, |ui| {
                ui.set_max_width(480.0);
                ui.add(
                    egui::Label::new(format!(
                        "A DUMMY LOAD and an external wattmeter must be connected to the antenna output.\nThe transmitter will be keyed (TUNE) on {band}, only at the points of 10%..90% of MAX Power ({} W) that are a whole number of watts (up to {} W), for at most 20 s per point. The other points are set with the spins.",
                        max,
                        (0..9).rev().find(|&k| point_exact(max, k)).map(|k| point_watts(max, k)).unwrap_or(0)
                    ))
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Start").strong()).min_size(egui::vec2(120.0, 40.0))).clicked() {
                        go = true;
                    }
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(120.0, 40.0))).clicked() {
                        no = true;
                    }
                });
            });
        if go || no {
            ctx.data_mut(|d| d.insert_temp(confirm_id, false));
        }
        if go {
            let mut m = Measure::idle();
            m.band = band.clone();
            if let Some(first) = next_exact_point(max, 0) {
                m.point = first;
                m.watts = point_watts(max, first);
                m.measured = m.watts as f64;
                m.active = true;
                connected.pa_measure = Some(m);
                if let Err(e) = key_point(connected) {
                    measure_abort(connected, &e);
                }
            } else {
                m.ended = Some("No point is a whole number of watts at this MAX Power: use the spins".to_string());
                connected.pa_measure = Some(m);
            }
        }
    }

    // Reset all (asks first).
    if ctx.data(|d| d.get_temp::<bool>(reset_all_id)).unwrap_or(false) {
        let mut yes = false;
        let mut no = false;
        egui::Window::new("pa_lin_reset_all")
            .id(egui::Id::new("pa_lin_reset_all_win"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(&ctx, |ui| {
                ui.label("Reset the linearization of ALL bands to 0 dB?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Reset all").min_size(egui::vec2(120.0, 40.0))).clicked() {
                        yes = true;
                    }
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(120.0, 40.0))).clicked() {
                        no = true;
                    }
                });
            });
        if yes {
            connected.pa_drive_adjust.clear();
            changed = true;
        }
        if yes || no {
            ctx.data_mut(|d| d.insert_temp(reset_all_id, false));
        }
    }
    changed
}
