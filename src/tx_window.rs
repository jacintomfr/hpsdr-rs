//! The "TX Menu" window (deskHPSDR tx_menu.c): full-screen, four sections chosen with round radio buttons: TX Basic Settings,
//! WDSP TX Audio Tools, WDSP CFC and Peak Labels, plus the mic profile block (Activate / Save / Import / Export).

use crate::config::TxUiExtra;
use crate::eq_curve as ec;
use crate::eq_profiles as ep;
use crate::noise_window::{choice_combo, choice_combo_h, touch_radio};
use crate::tx::{TxExtra, CTCSS_HZ};
use crate::{kiosk_accent_button, spin_buttons_full, std_checkbox, ConnectedState};

const ROW_H: f32 = 40.0;
/// Row pitch of the dense TX grid: 34 px controls + 10 px gap.
const GRID_H: f32 = 44.0;
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 160, 255);

// ---- tiny layout helpers: x ranges are pixels from the left edge of the row (the content is 966 px wide)

pub(crate) fn child(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, right_to_left: bool, dy: f32) -> egui::Ui {
    let r = egui::Rect::from_min_max(egui::pos2(row.left() + x0, row.top() + dy), egui::pos2(row.left() + x1, row.bottom() + dy));
    let layout = if right_to_left {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.new_child(egui::UiBuilder::new().max_rect(r).layout(layout))
}

pub(crate) fn lbl(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, text: &str) {
    let _ = x0;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    ui.painter().text(egui::pos2(row.left() + x1 - 6.0, row.center().y), egui::Align2::RIGHT_CENTER, text, font, color);
}

fn lbl_blue(ui: &mut egui::Ui, row: egui::Rect, x1: f32, text: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(egui::pos2(row.left() + x1 - 6.0, row.center().y), egui::Align2::RIGHT_CENTER, text, font, BLUE);
}

/// Spin cell (- value +) in x0..x1 of the row; true when the user changed it.
pub(crate) fn spin(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, id: &str, v: &mut f64, min: f64, max: f64, step: f64, dec: usize) -> bool {
    let mut c = child(ui, row, x0, x1, false, -6.0);
    let box_w = (x1 - x0 - 98.0).max(30.0);
    spin_buttons_full(&mut c, id, v, min, max, step, dec, None, box_w).changed()
}

pub(crate) fn check(ui: &mut egui::Ui, row: egui::Rect, x0: f32, x1: f32, v: &mut bool, label: &str) -> bool {
    let mut c = child(ui, row, x0, x1, false, 0.0);
    std_checkbox(&mut c, v, label).changed()
}

pub(crate) fn new_row(ui: &mut egui::Ui, w: f32, h: f32) -> egui::Rect {
    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover()).0
}

pub(crate) fn sep(ui: &mut egui::Ui) {
    ui.add_space(3.0);
    ui.separator();
    ui.add_space(3.0);
}

/// Returns (close, changed).
pub fn tx_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let section_id = egui::Id::new("tx_window_section");
    let popup_id = egui::Id::new("tx_window_popup");
    let mut section: u8 = ui.ctx().data(|d| d.get_temp(section_id)).unwrap_or(0);
    let Some(tx) = connected.tx_handle.as_ref() else {
        return (true, false);
    };
    let mut ex: TxExtra = tx.tx_extra();
    let ex0 = ex;
    let mut tui: TxUiExtra = connected.tx_ui;
    let tui0 = tui;
    let mode = connected.spectrum.mode();
    let digital = matches!(mode, crate::spectrum::Mode::Digl | crate::spectrum::Mode::Digu);
    let local_mic = connected.session.tx_audio_source.load(std::sync::atomic::Ordering::Relaxed) == crate::radio::TX_AUDIO_SOURCE_LOCAL_MIC;
    let mic_name = if local_mic {
        connected.mic_input_device.clone().unwrap_or_else(|| "System Default".to_string())
    } else {
        "SDR Device Mic".to_string()
    };

    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
    egui::Window::new("TX Menu")
        .id(egui::Id::new("tx_window"))
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
            let title = format!("hpsdr-rs - TX Menu (Mic Profile: {})", mic_name);
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

            // Section selector (round radios) and CLOSE at the top right.
            let row = new_row(ui, w, 54.0);
            for (i, (x0, label)) in [(10.0, "TX Basic Settings"), (240.0, "WDSP TX Audio Tools"), (500.0, "WDSP CFC"), (660.0, "Peak Labels")].iter().enumerate() {
                let mut c = child(ui, row, *x0, *x0 + 250.0, false, 0.0);
                if touch_radio(&mut c, section == i as u8, label).clicked() {
                    section = i as u8;
                }
            }
            egui::Area::new(egui::Id::new("tx_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(4.0);

            match section {
                0 => basic(ui, w, connected, &mut ex, &mut tui, digital, local_mic, popup_id, &mut changed),
                1 => audio_tools(ui, w, connected, &mut ex),
                2 => cfc_section(ui, w, connected, &mut ex),
                _ => peaks_section(ui, w, &mut tui),
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(section_id, section));
    if ex != ex0 {
        if let Some(tx) = connected.tx_handle.as_ref() {
            tx.set_tx_extra(ex);
        }
        changed = true;
    }
    if tui != tui0 {
        connected.tx_ui = tui;
        changed = true;
    }
    (close_now, changed)
}

// ======================================================================= TX Basic Settings

#[allow(clippy::too_many_arguments)]
fn basic(ui: &mut egui::Ui, w: f32, connected: &mut ConnectedState, ex: &mut TxExtra, tui: &mut TxUiExtra, digital: bool, local_mic: bool, popup_id: egui::Id, changed: &mut bool) {
    let _ = digital;
    // ---- mic profile block
    let descs = connected.mic_profile_descs.clone();
    let slot = connected.mic_profile_nr.filter(|n| *n <= 2).unwrap_or(0);
    let row = new_row(ui, w, GRID_H);
    {
        let mut c = child(ui, row, 0.0, 620.0, false, 0.0);
        let items_owned: Vec<String> = (0..3).map(|i| format!("Mic Profile {} ({})", i, descs.get(i).cloned().unwrap_or_else(|| "NOMIC".to_string()))).collect();
        let items: Vec<(&str, bool)> = items_owned.iter().map(|s| (s.as_str(), true)).collect();
        if let Some(i) = choice_combo_h(&mut c, "tx_profile", 600.0, slot, &items, 34.0) {
            connected.mic_profile_nr = Some(i);
            *changed = true;
        }
        let exists = ep::slot_exists(slot);
        let mut c = child(ui, row, 624.0, 795.0, false, 0.0);
        if c.add_enabled(exists, egui::Button::new(egui::RichText::new("Activate").color(BLUE).strong()).min_size(egui::vec2(165.0, 34.0))).clicked() {
            if let (Some(p), Some(tx)) = (ep::load_slot(slot), connected.tx_handle.as_ref()) {
                let rx = p.apply(tx);
                connected.spectrum.set_eq(rx);
                let group = ep::eq_mode_group(connected.spectrum.mode()).to_string();
                connected.rx_eq_by_mode.insert(group.clone(), rx);
                connected.tx_eq_by_mode.insert(group, tx.eq());
                *ex = tx.tx_extra();
                *changed = true;
            }
        }
        let mut c = child(ui, row, 806.0, 966.0, false, 0.0);
        if c.add(egui::Button::new(egui::RichText::new("Save").color(BLUE).strong()).min_size(egui::vec2(155.0, 34.0))).clicked() {
            if let Some(tx) = connected.tx_handle.as_ref() {
                // The "NAME" of the slot is the description of the mic in use when it is saved (like deskHPSDR).
                let desc = if local_mic {
                    connected.mic_input_device.clone().unwrap_or_else(|| "System Default".to_string())
                } else {
                    "SDR Device Mic".to_string()
                };
                let mut p = ep::MicProfile::capture(tx, connected.spectrum.eq());
                p.extra = *ex;
                let _ = ep::save_slot(slot, &p);
                while connected.mic_profile_descs.len() < 3 {
                    connected.mic_profile_descs.push("NOMIC".to_string());
                }
                connected.mic_profile_descs[slot] = desc;
                *changed = true;
            }
        }
    }
    let row = new_row(ui, w, GRID_H);
    {
        let mut c = child(ui, row, 624.0, 795.0, false, 0.0);
        if c.add(egui::Button::new(egui::RichText::new("Import file").color(BLUE).strong()).min_size(egui::vec2(165.0, 34.0))).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(popup_id, 1u8));
        }
        let mut c = child(ui, row, 806.0, 966.0, false, 0.0);
        if c.add(egui::Button::new(egui::RichText::new("Export file").color(BLUE).strong()).min_size(egui::vec2(155.0, 34.0))).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(popup_id, 2u8));
        }
    }
    sep(ui);

    // ---- computer audio input / SDR radio input
    let row = new_row(ui, w, GRID_H);
    {
        let mut lm = local_mic;
        let mut c = child(ui, row, 0.0, 300.0, false, 0.0);
        if std_checkbox(&mut c, &mut lm, "Use computer audio input").changed() {
            let v = if lm { crate::radio::TX_AUDIO_SOURCE_LOCAL_MIC } else { crate::radio::TX_AUDIO_SOURCE_AUTO };
            connected.session.tx_audio_source.store(v, std::sync::atomic::Ordering::Relaxed);
            *changed = true;
        }
        let devices = crate::audio::list_input_devices();
        let mut items_owned: Vec<String> = vec!["(System Default)".to_string()];
        items_owned.extend(devices.iter().cloned());
        let items: Vec<(&str, bool)> = items_owned.iter().map(|s| (s.as_str(), true)).collect();
        let sel = connected.mic_input_device.as_ref().and_then(|n| devices.iter().position(|d| d == n)).map(|i| i + 1).unwrap_or(0);
        let mut c = child(ui, row, 310.0, 966.0, false, 0.0);
        if let Some(i) = choice_combo_h(&mut c, "tx_mic_device", 640.0, sel, &items, 34.0) {
            let name = if i == 0 { None } else { Some(devices[i - 1].clone()) };
            connected.mic_input_device = name.clone();
            if let Some(mic) = &connected.mic_input {
                let buffer = std::sync::Arc::clone(mic.buffer());
                if let Ok(new_mic) = crate::audio::MicInput::start(buffer, name.as_deref()) {
                    connected.mic_input = Some(new_mic);
                }
            }
            connected.session.tx_audio_source.store(crate::radio::TX_AUDIO_SOURCE_LOCAL_MIC, std::sync::atomic::Ordering::Relaxed);
            *changed = true;
        }
    }
    let row = new_row(ui, w, GRID_H);
    {
        lbl_blue(ui, row, 172.0, "SDR Radio Input");
        let mut c = child(ui, row, 176.0, 352.0, false, 0.0);
        c.add_enabled_ui(!local_mic, |ui| {
            // Mic Boost / Line In need protocol work in radio.rs (planned): listed, not selectable yet.
            let _ = choice_combo_h(ui, "tx_sdr_input", 170.0, 0, &[("Mic In", true), ("Mic Boost", false), ("Line In", false)], 34.0);
        });
        let mut c = child(ui, row, 362.0, 620.0, false, 0.0);
        c.add_enabled(false, egui::Button::new(egui::RichText::new("POST TX MONITOR").strong()).min_size(egui::vec2(240.0, 34.0)));
        lbl(ui, row, 624.0, 800.0, "SDR LineIn (dB)");
        let mut v = 0.0f64;
        let mut c = child(ui, row, 806.0, 966.0, false, -6.0);
        c.add_enabled_ui(false, |ui| {
            spin_buttons_full(ui, "tx_linein", &mut v, -34.5, 12.0, 1.5, 1, None, 62.0);
        });
    }
    sep(ui);

    // ---- main grid
    const LL: (f32, f32) = (0.0, 172.0);
    const LW: (f32, f32) = (176.0, 352.0);
    const MM: (f32, f32) = (362.0, 620.0);
    const RL: (f32, f32) = (624.0, 800.0);
    const RW: (f32, f32) = (806.0, 966.0);
    let max_swr = connected.max_swr as f64;
    // Row A
    let row = new_row(ui, w, GRID_H);
    lbl(ui, row, LL.0, LL.1, "TX Filter Low");
    let mut c = child(ui, row, LW.0, LW.1, false, 0.0);
    c.add_enabled_ui(!ex.use_rx_filter, |ui| {
        let mut v = ex.tx_filter_low_hz as f64;
        let mut cc = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(row.min + egui::vec2(LW.0, -6.0), egui::vec2(LW.1 - LW.0, ROW_H))));
        if spin_buttons_full(&mut cc, "tx_fl", &mut v, 0.0, 8000.0, 50.0, 0, None, LW.1 - LW.0 - 98.0).changed() {
            ex.tx_filter_low_hz = v.round() as i32;
        }
    });
    check(ui, row, MM.0, MM.1, &mut ex.use_rx_filter, "TX uses RX Filter");
    {
        let mut c = child(ui, row, 560.0, RW.0 - 12.0, true, 0.0);
        std_checkbox(&mut c, &mut ex.addgain_enable, "Local Mic PreAmp Gain");
        let mut v = ex.addgain_gain_db;
        if spin(ui, row, RW.0, RW.1, "tx_addgain", &mut v, 1.0, 20.0, 1.0, 0) {
            ex.addgain_gain_db = v.round();
        }
    }
    // Row B
    let row = new_row(ui, w, GRID_H);
    lbl(ui, row, LL.0, LL.1, "TX Filter High");
    {
        let mut c = child(ui, row, LW.0, LW.1, false, 0.0);
        c.add_enabled_ui(!ex.use_rx_filter, |ui| {
            let mut v = ex.tx_filter_high_hz as f64;
            let mut cc = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(row.min + egui::vec2(LW.0, -6.0), egui::vec2(LW.1 - LW.0, ROW_H))));
            if spin_buttons_full(&mut cc, "tx_fh", &mut v, 0.0, 8000.0, 50.0, 0, None, LW.1 - LW.0 - 98.0).changed() {
                ex.tx_filter_high_hz = v.round() as i32;
            }
        });
    }
    check(ui, row, MM.0, MM.1, &mut tui.tune_use_drive, "Tune Drive = TX drive");
    lbl(ui, row, RL.0, RL.1, "Tune Drive level");
    {
        let mut v = connected.tune_power_percent as f64;
        let step = tui.tune_drive_step.max(1) as f64;
        if spin(ui, row, RW.0, RW.1, "tx_tunedrive", &mut v, 0.0, 100.0, step, 0) {
            connected.tune_power_percent = v.round().clamp(0.0, 100.0) as u32;
        }
    }
    // Row C
    let row = new_row(ui, w, GRID_H);
    check(ui, row, MM.0, MM.1, &mut tui.drive_per_band, "Use Drive levels per Band");
    lbl(ui, row, RL.0, RL.1, "Tune Drive Stepping");
    {
        let mut c = child(ui, row, RW.0, RW.1, false, 0.0);
        let steps = [1u8, 5, 10, 20, 25];
        let sel = steps.iter().position(|s| *s == tui.tune_drive_step).unwrap_or(0);
        let labels: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
        let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
        if let Some(i) = choice_combo_h(&mut c, "tx_tunestep", RW.1 - RW.0 - 14.0, sel, &items, 34.0) {
            tui.tune_drive_step = steps[i];
        }
    }
    // Row D
    let row = new_row(ui, w, GRID_H);
    lbl(ui, row, LL.0, LL.1, "Panadapter High");
    {
        let mut v = connected.tx_db_high as f64;
        if spin(ui, row, LW.0, LW.1, "tx_panhigh", &mut v, -220.0, 100.0, 1.0, 0) {
            connected.tx_db_high = v as f32;
            *changed = true;
        }
    }
    check(ui, row, MM.0, MM.1, &mut tui.swr_protection, "SWR Protection");
    lbl(ui, row, RL.0, RL.1, "SWR alarm at");
    {
        let mut v = max_swr;
        if spin(ui, row, RW.0, RW.1, "tx_swralarm", &mut v, 1.0, 10.0, 0.1, 1) {
            connected.max_swr = ((v * 10.0).round() / 10.0) as f32;
            *changed = true;
        }
    }
    // Row E
    let row = new_row(ui, w, GRID_H);
    lbl(ui, row, LL.0, LL.1, "Panadapter Low");
    {
        let mut v = connected.tx_db_low as f64;
        if spin(ui, row, LW.0, LW.1, "tx_panlow", &mut v, -400.0, 100.0, 1.0, 0) {
            connected.tx_db_low = v as f32;
            *changed = true;
        }
    }
    check(ui, row, MM.0, MM.1, &mut ex.ctcss_enabled, "CTCSS Enable");
    lbl(ui, row, RL.0, RL.1, "CTCSS Frequency");
    {
        let mut c = child(ui, row, RW.0, RW.1, false, 0.0);
        let labels: Vec<String> = CTCSS_HZ.iter().map(|f| format!("{f:.1}")).collect();
        let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
        if let Some(i) = choice_combo_h(&mut c, "tx_ctcss", RW.1 - RW.0 - 14.0, ex.ctcss_index as usize, &items, 34.0) {
            ex.ctcss_index = i as u8;
        }
    }
    // Row F
    let row = new_row(ui, w, GRID_H);
    lbl(ui, row, LL.0, LL.1, "Panadapter Step");
    {
        let mut v = connected.tx_panadapter_step_db as f64;
        if spin(ui, row, LW.0, LW.1, "tx_panstep", &mut v, 5.0, 25.0, 5.0, 0) {
            connected.tx_panadapter_step_db = v as f32;
            *changed = true;
        }
    }
    {
        // deskHPSDR's "FM PreEmp/ALC" check is the inverse of its pre_emphasize variable.
        let mut on = !ex.fm_pre_emphasis;
        if check(ui, row, MM.0, MM.1, &mut on, "FM PreEmp/ALC") {
            ex.fm_pre_emphasis = !on;
        }
    }
    lbl(ui, row, RL.0, RL.1, "AM Carrier Level");
    {
        let mut v = ex.am_carrier_level;
        if spin(ui, row, RW.0, RW.1, "tx_amcarrier", &mut v, 0.0, 1.0, 0.1, 1) {
            ex.am_carrier_level = (v * 10.0).round() / 10.0;
        }
    }
    // Row G
    let row = new_row(ui, w, GRID_H);
    check(ui, row, MM.0, MM.1, &mut tui.tx_display_filled, "Fill TX Panadapter");
    lbl(ui, row, RL.0, RL.1, "Max Digi Drv");
    {
        let mut v = tui.max_digi_drive as f64;
        if spin(ui, row, RW.0, RW.1, "tx_maxdigi", &mut v, 1.0, 100.0, 1.0, 0) {
            tui.max_digi_drive = v.round() as i32;
        }
    }
    popups(ui, connected, popup_id, ex, changed);
}

/// Import / Export file pickers (in-app file list, since there is no native file dialog on the touch panel).
fn popups(ui: &mut egui::Ui, connected: &mut ConnectedState, popup_id: egui::Id, ex: &mut TxExtra, changed: &mut bool) {
    let which: u8 = ui.ctx().data(|d| d.get_temp(popup_id)).unwrap_or(0);
    if which == 0 {
        return;
    }
    let name_id = egui::Id::new("tx_window_export_name");
    let mut name: String = ui.ctx().data(|d| d.get_temp(name_id)).unwrap_or_else(|| "audio_profile".to_string());
    let mut close = false;
    egui::Window::new("tx_profile_files")
        .id(egui::Id::new("tx_profile_files"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.label(egui::RichText::new(if which == 1 { "Import deskHPSDR Audio Profile" } else { "Export deskHPSDR Audio Profile" }).strong());
            ui.add_space(6.0);
            if which == 2 {
                ui.horizontal(|ui| {
                    ui.label("File name:");
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
                    ui.label(".prop");
                    if ui.add(egui::Button::new("Export").min_size(egui::vec2(90.0, 36.0))).clicked() {
                        if let Some(tx) = connected.tx_handle.as_ref() {
                            let mut p = ep::MicProfile::capture(tx, connected.spectrum.eq());
                            p.extra = *ex;
                            let file = if name.ends_with(".prop") { name.clone() } else { format!("{name}.prop") };
                            if let Some(dir) = crate::config::settings_dir() {
                                let _ = p.export_to(&dir.join(file));
                            }
                        }
                        close = true;
                    }
                });
            }
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for path in ep::list_prop_files() {
                    let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.add(egui::Button::new(&fname).min_size(egui::vec2(500.0, 40.0))).clicked() {
                        if which == 1 {
                            if let (Ok(p), Some(tx)) = (ep::MicProfile::import_from(&path), connected.tx_handle.as_ref()) {
                                let rx = p.apply(tx);
                                connected.spectrum.set_eq(rx);
                                *ex = tx.tx_extra();
                                *changed = true;
                            }
                            close = true;
                        } else {
                            name = fname.trim_end_matches(".prop").to_string();
                        }
                    }
                }
            });
            ui.add_space(6.0);
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(110.0, 40.0))).clicked() {
                close = true;
            }
        });
    ui.ctx().data_mut(|d| {
        d.insert_temp(name_id, name);
        d.insert_temp(popup_id, if close { 0u8 } else { which });
    });
}

// ======================================================================= WDSP TX Audio Tools

fn audio_tools(ui: &mut egui::Ui, w: f32, connected: &mut ConnectedState, ex: &mut TxExtra) {
    let tx = connected.tx_handle.as_ref().unwrap();
    // Row 0: Phase rotator
    let row = new_row(ui, w, ROW_H + 4.0);
    check(ui, row, 0.0, 300.0, &mut ex.phrot_enable, "Phase Rotator");
    let mut v = ex.phrot_stage as f64;
    if spin(ui, row, 310.0, 500.0, "tx_phst", &mut v, 1.0, 15.0, 1.0, 0) {
        ex.phrot_stage = v.round() as u8;
    }
    let mut v = ex.phrot_freq_hz as f64;
    if spin(ui, row, 520.0, 710.0, "tx_phfr", &mut v, 1.0, 500.0, 1.0, 0) {
        ex.phrot_freq_hz = v.round() as i32;
    }
    // Row 1: Leveler
    let row = new_row(ui, w, ROW_H + 4.0);
    let mut on = tx.leveler_enabled();
    if check(ui, row, 0.0, 300.0, &mut on, "Leveler") {
        tx.set_leveler_enabled(on);
    }
    let mut v = tx.leveler_gain_db() as f64;
    if spin(ui, row, 310.0, 500.0, "tx_levg", &mut v, 0.0, 15.0, 1.0, 0) {
        tx.set_leveler_gain_db(v as f32);
    }
    let mut v = tx.leveler_decay_ms() as f64;
    if spin(ui, row, 520.0, 710.0, "tx_levd", &mut v, 0.0, 500.0, 1.0, 0) {
        tx.set_leveler_decay_ms(v.round() as i32);
    }
    // Row 2: Speech Proc + CESSB
    let row = new_row(ui, w, ROW_H + 4.0);
    let mut on = tx.compressor_enabled();
    if check(ui, row, 0.0, 300.0, &mut on, "Speech Proc") {
        tx.set_compressor_enabled(on);
    }
    let mut v = tx.compressor_gain_db() as f64;
    if spin(ui, row, 310.0, 500.0, "tx_comp", &mut v, 0.0, 20.0, 1.0, 0) {
        tx.set_compressor_gain_db(v.round() as f32);
    }
    check(ui, row, 520.0, 800.0, &mut ex.cessb_enable, "Auto CESSB");
    // Row 3: EQ Ctfmode
    let row = new_row(ui, w, ROW_H + 4.0);
    check(ui, row, 0.0, 400.0, &mut ex.eq_ctfmode, "WDSP TX EQ Ctfmode");
    sep(ui);
    // DEXP
    let row = new_row(ui, w, ROW_H);
    check(ui, row, 0.0, 300.0, &mut ex.dexp, "Use DEXP");
    check(ui, row, 480.0, 800.0, &mut ex.dexp_filter, "Use Side Channel Filter");
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Expansion Factor (dB)");
    let mut v = ex.dexp_exp_db as f64;
    if spin(ui, row, 236.0, 440.0, "tx_dexp", &mut v, 0.0, 30.0, 1.0, 0) {
        ex.dexp_exp_db = v.round() as i32;
    }
    lbl(ui, row, 450.0, 690.0, "Filter Low-Cut (Hz)");
    let mut v = ex.dexp_filter_low_hz as f64;
    if spin(ui, row, 696.0, 900.0, "tx_dlow", &mut v, 0.0, 1200.0, 25.0, 0) {
        ex.dexp_filter_low_hz = v.round() as i32;
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Hysteresis Ratio");
    let mut v = ex.dexp_hyst;
    if spin(ui, row, 236.0, 440.0, "tx_dhyst", &mut v, 0.05, 0.95, 0.01, 2) {
        ex.dexp_hyst = (v * 100.0).round() / 100.0;
    }
    lbl(ui, row, 450.0, 690.0, "Filter High-Cut (Hz)");
    let mut v = ex.dexp_filter_high_hz as f64;
    if spin(ui, row, 696.0, 900.0, "tx_dhigh", &mut v, 500.0, 10000.0, 25.0, 0) {
        ex.dexp_filter_high_hz = v.round() as i32;
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Trigger Level (dB)");
    let mut v = ex.dexp_trigger_db as f64;
    if spin(ui, row, 236.0, 440.0, "tx_dtrig", &mut v, -40.0, -10.0, 1.0, 0) {
        ex.dexp_trigger_db = v.round() as i32;
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Trigger Attack tau (ms)");
    let mut v = ex.dexp_tau_ms;
    if spin(ui, row, 236.0, 440.0, "tx_dtau", &mut v, 1.0, 250.0, 1.0, 0) {
        ex.dexp_tau_ms = v.round();
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Trigger Attack Time (ms)");
    let mut v = ex.dexp_attack_ms;
    if spin(ui, row, 236.0, 440.0, "tx_datt", &mut v, 1.0, 250.0, 1.0, 0) {
        ex.dexp_attack_ms = v.round();
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Trigger Release Time (ms)");
    let mut v = ex.dexp_release_ms;
    if spin(ui, row, 236.0, 440.0, "tx_drel", &mut v, 1.0, 500.0, 1.0, 0) {
        ex.dexp_release_ms = v.round();
    }
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 0.0, 230.0, "Trigger Hold Time (ms)");
    let mut v = ex.dexp_hold_ms;
    if spin(ui, row, 236.0, 440.0, "tx_dhold", &mut v, 10.0, 1500.0, 10.0, 0) {
        ex.dexp_hold_ms = v.round();
    }
}

// ======================================================================= WDSP CFC

fn sorted12(ex: &TxExtra) -> ([i32; 12], [i32; 12], [i32; 12]) {
    let mut f = [0i32; 12];
    let mut l = [0i32; 12];
    let mut p = [0i32; 12];
    for i in 0..12 {
        f[i] = ex.cfc_freq_hz[i + 1];
        l[i] = ex.cfc_lvl_db[i + 1];
        p[i] = ex.cfc_post_db[i + 1];
    }
    (f, l, p)
}

fn cfc_section(ui: &mut egui::Ui, w: f32, connected: &mut ConnectedState, ex: &mut TxExtra) {
    let tx = connected.tx_handle.as_ref().unwrap();
    // Row 0: global controls.
    let row = new_row(ui, w, ROW_H + 2.0);
    let mut pre = tx.cfc_enabled();
    if check(ui, row, 0.0, 190.0, &mut pre, "Use Pre-CFC") {
        tx.set_cfc_enabled(pre);
    }
    check(ui, row, 195.0, 400.0, &mut ex.cfc_post_enabled, "Use Post-CFC");
    lbl(ui, row, 405.0, 505.0, "Pre Comp:");
    let mut v = ex.cfc_lvl_db[0] as f64;
    if spin(ui, row, 510.0, 680.0, "cfc_pre", &mut v, 0.0, 20.0, 1.0, 0) {
        ex.cfc_lvl_db[0] = v.round() as i32;
    }
    lbl(ui, row, 690.0, 790.0, "Post Gain:");
    let mut v = ex.cfc_post_db[0] as f64;
    if spin(ui, row, 796.0, 966.0, "cfc_postg", &mut v, -20.0, 20.0, 1.0, 0) {
        ex.cfc_post_db[0] = v.round() as i32;
    }

    // Graph: pre-compression (orange) and post-EQ (blue) curves, draggable points.
    let plot_h = 190.0f32;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, plot_h), egui::Sense::click_and_drag());
    let (pw, ph) = ec::plot_size(w as f64, plot_h as f64);
    let origin = rect.min + egui::vec2(ec::PAD_LEFT as f32, 6.0);
    let ph = ph + (ec::PAD_TOP as f64 - 6.0);
    let to_screen = |x: f64, y: f64| egui::pos2(origin.x + x as f32, origin.y + y as f32);
    let (freqs, lvl, post) = sorted12(ex);
    let pts_pre: Vec<(f64, f64)> = (0..12).map(|i| (ec::x_for_freq(freqs[i] as f64, pw), ec::y_for_gain(lvl[i] as f64, ph))).collect();
    let pts_post: Vec<(f64, f64)> = (0..12).map(|i| (ec::x_for_freq(freqs[i] as f64, pw), ec::y_for_gain(post[i] as f64, ph))).collect();
    let drag_id = egui::Id::new("cfc_drag");
    let mut drag: Option<(bool, usize)> = ui.ctx().data(|d| d.get_temp(drag_id));
    if resp.drag_started() {
        if let Some(p) = resp.interact_pointer_pos() {
            let (px, py) = (p.x as f64 - origin.x as f64, p.y as f64 - origin.y as f64);
            let a = ec::nearest_point(&pts_pre, px, py, ec::HIT_RADIUS);
            let b = ec::nearest_point(&pts_post, px, py, ec::HIT_RADIUS);
            drag = match (a, b) {
                (Some(i), Some(j)) => {
                    let da = (pts_pre[i].0 - px).powi(2) + (pts_pre[i].1 - py).powi(2);
                    let db = (pts_post[j].0 - px).powi(2) + (pts_post[j].1 - py).powi(2);
                    Some(if da <= db { (true, i) } else { (false, j) })
                }
                (Some(i), None) => Some((true, i)),
                (None, Some(j)) => Some((false, j)),
                _ => None,
            };
        }
    }
    if let Some((is_pre, i)) = drag {
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let f = ec::freq_for_x(p.x as f64 - origin.x as f64, pw);
                let g = ec::gain_for_y(p.y as f64 - origin.y as f64, ph);
                let (nf, ng) = ec::drag_clamp(&freqs, i, f, g);
                ex.cfc_freq_hz[i + 1] = nf;
                if is_pre {
                    ex.cfc_lvl_db[i + 1] = ng.clamp(0, 20);
                } else {
                    ex.cfc_post_db[i + 1] = ng.clamp(-20, 20);
                }
            }
        } else {
            drag = None;
        }
    }
    ui.ctx().data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(drag_id, v);
        }
        None => d.remove::<(bool, usize)>(drag_id),
    });
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(20));
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(55));
    let font = egui::FontId::proportional(10.0);
    let col = egui::Color32::from_gray(170);
    for (g, lab) in ec::GAIN_LINES.iter().zip(ec::GAIN_LABELS.iter()) {
        let y = ec::y_for_gain(*g as f64, ph);
        painter.line_segment([to_screen(0.0, y), to_screen(pw, y)], grid);
        painter.text(to_screen(-4.0, y), egui::Align2::RIGHT_CENTER, *lab, font.clone(), col);
    }
    for (f, lab) in ec::FREQ_LINES.iter().zip(ec::FREQ_LABELS.iter()) {
        let x = ec::x_for_freq(*f as f64, pw);
        painter.line_segment([to_screen(x, 0.0), to_screen(x, ph)], grid);
        painter.text(to_screen(x, ph + 1.0), egui::Align2::CENTER_TOP, *lab, font.clone(), col);
    }
    let (freqs, lvl, post) = sorted12(ex);
    let draw_curve = |gains: &[i32; 12], deg: u8, r: bool, wx10: &[u16; 12], color: egui::Color32| {
        let wts: [f64; 12] = std::array::from_fn(|i| wx10[i].max(1) as f64 / 10.0);
        let curve = if deg == 0 { ec::legacy_polyline(&freqs, gains) } else { ec::nurbs_curve(&freqs, gains, deg, r, &wts, 48000.0) };
        let line: Vec<egui::Pos2> = curve.iter().map(|(hz, db)| to_screen(ec::x_for_freq(*hz, pw), ec::y_for_gain(*db, ph))).collect();
        if line.len() >= 2 {
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
        }
    };
    let orange = egui::Color32::from_rgb(232, 150, 46);
    let blue = egui::Color32::from_rgb(120, 190, 255);
    draw_curve(&lvl, ex.cfc_comp_deg, ex.cfc_comp_r, &ex.cfc_comp_w_x10, orange);
    draw_curve(&post, ex.cfc_post_deg, ex.cfc_post_r, &ex.cfc_post_w_x10, blue);
    for i in 0..12 {
        painter.circle_filled(to_screen(ec::x_for_freq(freqs[i] as f64, pw), ec::y_for_gain(lvl[i] as f64, ph)), 4.0, orange);
        painter.circle_filled(to_screen(ec::x_for_freq(freqs[i] as f64, pw), ec::y_for_gain(post[i] as f64, ph)), 4.0, blue);
    }
    ui.add_space(3.0);

    // Point table: Freq / Pre / Post, bands 1-6 left, 7-12 right.
    let row = new_row(ui, w, 18.0);
    for half in 0..2 {
        let x = half as f32 * 490.0;
        for (k, (t, off, ww)) in [("Freq.", 0.0, 154.0), ("Pre", 154.0, 150.0), ("Post", 304.0, 150.0)].iter().enumerate() {
            let _ = k;
            let cx = x + off + ww / 2.0;
            ui.painter().text(egui::pos2(row.left() + cx, row.center().y), egui::Align2::CENTER_CENTER, *t, egui::FontId::proportional(14.0), egui::Color32::from_gray(225));
        }
    }
    for r in 0..6 {
        let row = new_row(ui, w, 36.0);
        for half in 0..2 {
            let b = r + 6 * half + 1;
            let x = half as f32 * 490.0;
            let mut v = ex.cfc_freq_hz[b] as f64;
            if spin(ui, row, x, x + 154.0, &format!("cfc_f{b}"), &mut v, 10.0, 16000.0, 10.0, 0) {
                let lo = if b == 1 { 10 } else { ex.cfc_freq_hz[b - 1] + 10 };
                let hi = if b == 12 { 16000 } else { ex.cfc_freq_hz[b + 1] - 10 };
                ex.cfc_freq_hz[b] = (v.round() as i32).clamp(lo, hi.max(lo));
            }
            let mut v = ex.cfc_lvl_db[b] as f64;
            if spin(ui, row, x + 154.0, x + 304.0, &format!("cfc_l{b}"), &mut v, 0.0, 20.0, 1.0, 0) {
                ex.cfc_lvl_db[b] = v.round() as i32;
            }
            let mut v = ex.cfc_post_db[b] as f64;
            if spin(ui, row, x + 304.0, x + 454.0, &format!("cfc_p{b}"), &mut v, -20.0, 20.0, 1.0, 0) {
                ex.cfc_post_db[b] = v.round() as i32;
            }
        }
    }
}

// ======================================================================= Peak Labels

fn peaks_section(ui: &mut egui::Ui, w: f32, tui: &mut TxUiExtra) {
    let row = new_row(ui, w, ROW_H + 4.0);
    check(ui, row, 0.0, 600.0, &mut tui.peaks_on, "Show Peak Numbers on Panadapter");
    let row = new_row(ui, w, ROW_H + 4.0);
    check(ui, row, 0.0, 600.0, &mut tui.peaks_in_passband, "Show Peaks in Passband Only");
    let row = new_row(ui, w, ROW_H + 4.0);
    check(ui, row, 0.0, 600.0, &mut tui.peaks_hide_noise, "Hide Peaks Below Noise Floor");
    let row = new_row(ui, w, ROW_H + 4.0);
    lbl(ui, row, 0.0, 480.0, "Number of Peaks to label:");
    let mut v = tui.peaks_num as f64;
    if spin(ui, row, 490.0, 700.0, "pk_num", &mut v, 1.0, 10.0, 1.0, 0) {
        tui.peaks_num = v.round() as i32;
    }
    let row = new_row(ui, w, ROW_H + 4.0);
    lbl(ui, row, 0.0, 480.0, "Panadapter Ignore Adjacent Peaks:");
    let mut v = tui.peaks_ignore_divider as f64;
    if spin(ui, row, 490.0, 700.0, "pk_ign", &mut v, 1.0, 150.0, 1.0, 0) {
        tui.peaks_ignore_divider = v.round() as i32;
    }
    let row = new_row(ui, w, ROW_H + 4.0);
    lbl(ui, row, 0.0, 480.0, "Panadapter Noise Floor Percentile:");
    let mut v = tui.peaks_noise_percentile as f64;
    if spin(ui, row, 490.0, 700.0, "pk_pct", &mut v, 1.0, 100.0, 1.0, 0) {
        tui.peaks_noise_percentile = v.round() as i32;
    }
}
