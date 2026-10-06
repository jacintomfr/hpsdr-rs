//! The "Receive" window (deskHPSDR rx_menu.c): full-screen, tab buttons RX1 / Options. The RX1 page mirrors deskHPSDR's rows:
//! Sample Rate, Select ADC, Dither/Random bit, Preamp, Mute when not active, DIGU/DIGL offsets with the RTTY / OFF buttons on
//! the left; output device, Test Audio, channel, Use Computer Audio Output and Mute Audio to Radio on the right.
//! The Options page: Alex attenuator, RX filter bypass, HL2 ADC auto gain, and the Protocol 2 network / audio reserve block.

use crate::spectrum::RxExtra;
use crate::tx_window::{check, child, lbl, new_row, sep, spin};
use crate::noise_window::choice_combo_h;
use crate::{kiosk_accent_button, ConnectedState};
use std::sync::atomic::Ordering;

const ROW_H: f32 = 44.0;

/// Returns (close, changed).
pub fn rx_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let tab_id = egui::Id::new("rx_window_tab");
    let mut tab: u8 = ui.ctx().data(|d| d.get_temp(tab_id)).unwrap_or(0);
    let mut ex: RxExtra = connected.spectrum.rx_extra();
    let ex0 = ex;
    let mut rui = connected.rx_ui;
    let rui0 = rui;
    let adc = connected.session.adc.load(Ordering::Relaxed);

    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0);
    egui::Window::new("Receive")
        .id(egui::Id::new("rx_window"))
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
            let title = if tab == 0 { format!("hpsdr-rs - Receive - RX1 - ADC{adc}") } else { "hpsdr-rs - Receive - Options".to_string() };
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(16.0), egui::Color32::from_gray(225));

            // Tab buttons (like deskHPSDR's toggle buttons): the active one is blue.
            let row = new_row(ui, w, 54.0);
            for (i, (x0, label)) in [(0.0, "RX1"), (130.0, "Options")].iter().enumerate() {
                let mut c = child(ui, row, *x0, *x0 + 120.0, false, 0.0);
                let active = tab == i as u8;
                let btn = egui::Button::new(egui::RichText::new(*label).color(if active { egui::Color32::WHITE } else { egui::Color32::from_gray(210) }))
                    .fill(if active { egui::Color32::from_rgb(70, 150, 245) } else { egui::Color32::from_gray(48) })
                    .min_size(egui::vec2(120.0, 40.0))
                    .corner_radius(5.0);
                if c.add(btn).clicked() {
                    tab = i as u8;
                }
            }
            egui::Area::new(egui::Id::new("rx_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(4.0);
            if tab == 0 {
                rx_page(ui, w, connected, &mut ex, &mut rui, &mut changed);
            } else {
                options_page(ui, w, connected, &mut rui, &mut changed);
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    if ex != ex0 {
        connected.spectrum.set_rx_extra(ex);
        changed = true;
    }
    if rui != rui0 {
        connected.rx_ui = rui;
        // The flags that live in the radio / audio layers take effect at once.
        connected.session.adc_dither.store(rui.adc_dither, Ordering::Relaxed);
        connected.session.adc_random.store(rui.adc_random, Ordering::Relaxed);
        connected.session.adc0_filter_bypass.store(rui.adc0_filter_bypass, Ordering::Relaxed);
        connected.session.adc1_filter_bypass.store(rui.adc1_filter_bypass, Ordering::Relaxed);
        crate::audio::set_rx_reserve(rui.rx_reserve_enabled, rui.rx_reserve_ms.clamp(5, 500) as u32);
        crate::audio::set_latency_correction(rui.latency_correction);
        changed = true;
    }
    (close_now, changed)
}

#[allow(clippy::too_many_arguments)]
fn rx_page(ui: &mut egui::Ui, w: f32, connected: &mut ConnectedState, ex: &mut RxExtra, rui: &mut crate::config::RxUiExtra, changed: &mut bool) {
    let hl2 = matches!(connected.device.board, crate::Boards::HermesLite | crate::Boards::HermesLite2);
    let p12 = connected.device.protocol == 1 || connected.device.protocol == 2;
    // Left block: rows are built top-down; the right block uses fixed rows 0..3 (as in deskHPSDR).
    let mut left: Vec<&str> = Vec::new();
    if p12 {
        left.push("rate");
        if connected.device.adcs > 1 {
            left.push("adc");
        }
        left.push("dither");
        if connected.device.board == crate::Boards::Metis {
            left.push("preamp");
        }
    }
    left.push("mute_na");
    let rows = left.len().max(4);
    for r in 0..rows {
        let row = new_row(ui, w, ROW_H);
        // ---- left
        match left.get(r).copied() {
            Some("rate") => {
                lbl(ui, row, 0.0, 220.0, "Sample Rate RX1");
                let rates: &[u32] = if connected.device.board == crate::Boards::Rx888 {
                    &[96_000, 192_000, 384_000]
                } else if connected.device.protocol == 2 {
                    &[48_000, 96_000, 192_000, 384_000, 768_000, 1_536_000]
                } else {
                    &[48_000, 96_000, 192_000, 384_000]
                };
                let labels: Vec<String> = rates.iter().map(|r| r.to_string()).collect();
                let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
                let sel = rates.iter().position(|r| *r == connected.sample_rate).unwrap_or(0);
                let mut c = child(ui, row, 226.0, 390.0, false, 0.0);
                if let Some(i) = choice_combo_h(&mut c, "rx_rate", 150.0, sel, &items, 34.0) {
                    if rates[i] != connected.sample_rate {
                        crate::change_sample_rate(connected, rates[i]);
                        *changed = true;
                    }
                }
            }
            Some("adc") => {
                lbl(ui, row, 0.0, 220.0, "Select ADC");
                let n = connected.device.adcs as usize;
                let labels: Vec<String> = (0..n).map(|i| format!("ADC-{i}")).collect();
                let items: Vec<(&str, bool)> = labels.iter().map(|s| (s.as_str(), true)).collect();
                let cur = connected.session.adc.load(Ordering::Relaxed) as usize;
                let mut c = child(ui, row, 226.0, 390.0, false, 0.0);
                if let Some(i) = choice_combo_h(&mut c, "rx_adc", 150.0, cur.min(n.saturating_sub(1)), &items, 34.0) {
                    connected.session.adc.store(i as u32, Ordering::Relaxed);
                    *changed = true;
                }
            }
            Some("dither") => {
                let label = if connected.device.board == crate::Boards::HermesLite2 { "Dither Bit (HL2 Band Volts)" } else { "Dither Bit" };
                check(ui, row, 0.0, 262.0, &mut rui.adc_dither, label);
                check(ui, row, 268.0, 392.0, &mut rui.adc_random, "Random Bit");
            }
            Some("preamp") => {
                let mut on = connected.session.preamp_enabled.load(Ordering::Relaxed);
                if check(ui, row, 0.0, 300.0, &mut on, "Preamp") {
                    connected.session.preamp_enabled.store(on, Ordering::Relaxed);
                    *changed = true;
                }
            }
            Some("mute_na") => {
                check(ui, row, 0.0, 330.0, &mut ex.mute_when_not_active, "Mute when not active");
            }
            _ => {}
        }
        // ---- right (fixed rows)
        match r {
            0 => {
                // Output device + Test Audio.
                let devices = crate::audio::list_output_devices();
                let mut owned: Vec<String> = vec!["(System Default)".to_string()];
                owned.extend(devices.iter().cloned());
                let items: Vec<(&str, bool)> = owned.iter().map(|s| (s.as_str(), true)).collect();
                let sel = connected.audio_output_device.as_ref().and_then(|n| devices.iter().position(|d| d == n)).map(|i| i + 1).unwrap_or(0);
                let mut c = child(ui, row, 400.0, 800.0, false, 0.0);
                if let Some(i) = choice_combo_h(&mut c, "rx_out_dev", 392.0, sel, &items, 34.0) {
                    let name = if i == 0 { None } else { Some(devices[i - 1].clone()) };
                    connected.audio_output_device = name.clone();
                    connected.audio_output = crate::audio::AudioOutput::start(
                        std::sync::Arc::clone(&connected.spectrum.audio_out),
                        name.as_deref(),
                        Some(std::sync::Arc::clone(&connected.session.mox)),
                    )
                    .ok();
                    *changed = true;
                }
                let mut c = child(ui, row, 812.0, 966.0, false, 0.0);
                if c.add_enabled(ex.local_audio, egui::Button::new("Test Audio").min_size(egui::vec2(150.0, 34.0))).clicked() {
                    crate::audio::play_test_tones(connected.audio_output_device.as_deref());
                }
            }
            1 => {
                let items = [("Stereo / Mono Downmix (L+R)", true), ("Left Channel only", true), ("Right Channel only", true)];
                let mut c = child(ui, row, 400.0, 800.0, false, 0.0);
                if let Some(i) = choice_combo_h(&mut c, "rx_channel", 392.0, ex.audio_channel as usize, &items, 34.0) {
                    ex.audio_channel = i as u8;
                }
            }
            2 => {
                check(ui, row, 400.0, 800.0, &mut ex.local_audio, "Use Computer Audio Output");
            }
            3 => {
                if p12 {
                    let mut mute = !connected.session.send_rx_audio_to_radio.load(Ordering::Relaxed);
                    if check(ui, row, 400.0, 800.0, &mut mute, "Mute Audio to Radio") {
                        connected.session.send_rx_audio_to_radio.store(!mute, Ordering::Relaxed);
                        *changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = hl2;
    // DIGU / DIGL offsets.
    let row = new_row(ui, w, ROW_H);
    lbl(ui, row, 100.0, 250.0, "DIGU Offset");
    let mut v = ex.digi_offset_u_hz as f64;
    if spin(ui, row, 256.0, 430.0, "rx_digu", &mut v, 0.0, 4000.0, 10.0, 0) {
        ex.digi_offset_u_hz = v.round() as i32;
    }
    lbl(ui, row, 430.0, 470.0, "Hz");
    lbl(ui, row, 520.0, 660.0, "DIGL Offset");
    let mut v = ex.digi_offset_l_hz as f64;
    if spin(ui, row, 666.0, 840.0, "rx_digl", &mut v, 0.0, 4000.0, 10.0, 0) {
        ex.digi_offset_l_hz = v.round() as i32;
    }
    lbl(ui, row, 840.0, 880.0, "Hz");
    // Offset RTTY / Offset OFF.
    let row = new_row(ui, w, ROW_H);
    let mut c = child(ui, row, 300.0, 490.0, false, 0.0);
    if c.add(egui::Button::new("Offset RTTY").min_size(egui::vec2(180.0, 34.0))).on_hover_text("Set DIGU/DIGL offsets for RTTY").clicked() {
        ex.digi_offset_u_hz = 1500;
        ex.digi_offset_l_hz = 2210;
    }
    let mut c = child(ui, row, 510.0, 700.0, false, 0.0);
    if c.add(egui::Button::new("Offset OFF").min_size(egui::vec2(180.0, 34.0))).on_hover_text("Disable DIGU/DIGL offsets. Recommended for all FT modes").clicked() {
        ex.digi_offset_u_hz = 0;
        ex.digi_offset_l_hz = 0;
    }
}

fn options_page(ui: &mut egui::Ui, w: f32, connected: &mut ConnectedState, rui: &mut crate::config::RxUiExtra, changed: &mut bool) {
    let hl2 = matches!(connected.device.board, crate::Boards::HermesLite | crate::Boards::HermesLite2);
    let has_alex = connected.device.protocol == 1 && !hl2 && connected.device.board != crate::Boards::Orion2;
    let orion2 = matches!(connected.device.board, crate::Boards::Orion2 | crate::Boards::Saturn);
    let frame = |ui: &mut egui::Ui, title: &str, content: &mut dyn FnMut(&mut egui::Ui)| {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(title).strong().color(egui::Color32::from_rgb(90, 160, 255)));
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95))).corner_radius(4.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(w - 20.0);
            content(ui);
        });
    };
    frame(ui, "Hardware", &mut |ui| {
        let mut any = false;
        if has_alex {
            any = true;
            let row = new_row(ui, w - 20.0, ROW_H);
            lbl(ui, row, 0.0, 220.0, "Alex Attenuator");
            let items = [("0 dB", true), ("10 dB", true), ("20 dB", true), ("30 dB", true)];
            let cur = connected.session.alex_attenuation.load(Ordering::Relaxed).min(3) as usize;
            let mut c = child(ui, row, 226.0, 390.0, false, 0.0);
            if let Some(i) = choice_combo_h(&mut c, "rx_alex_att", 150.0, cur, &items, 34.0) {
                connected.session.alex_attenuation.store(i as u32, Ordering::Relaxed);
                *changed = true;
            }
            let row = new_row(ui, w - 20.0, ROW_H);
            check(ui, row, 0.0, 330.0, &mut rui.adc0_filter_bypass, "Bypass ADC0 RX filters");
            if orion2 {
                check(ui, row, 400.0, 760.0, &mut rui.adc1_filter_bypass, "Bypass ADC1 RX filters");
            }
        }
        if connected.device.board == crate::Boards::HermesLite2 {
            any = true;
            let row = new_row(ui, w - 20.0, ROW_H);
            let mut on = connected.autogain_enabled;
            if check(ui, row, 0.0, 400.0, &mut on, "HL2 ADC Auto Gain RxPGA") {
                connected.autogain_enabled = on;
                *changed = true;
            }
            let row = new_row(ui, w - 20.0, ROW_H);
            let mut t = connected.autogain_time_enabled;
            let mut c = child(ui, row, 0.0, 420.0, false, 0.0);
            c.add_enabled_ui(connected.autogain_enabled, |ui| {
                if crate::std_checkbox(ui, &mut t, "HL2 Auto Gain time-regulated").changed() {
                    connected.autogain_time_enabled = t;
                    *changed = true;
                }
            });
        }
        if !any {
            ui.label("No global hardware controls available");
        }
    });
    if connected.device.protocol == 2 {
        frame(ui, "Protocol 2 Network", &mut |ui| {
            let ww = w - 20.0;
            let row = new_row(ui, ww, ROW_H);
            // The P2 network jitter buffer of deskHPSDR has no counterpart here yet.
            let mut c = child(ui, row, 0.0, 420.0, false, 0.0);
            c.add_enabled_ui(false, |ui| {
                let mut v = rui.p2_jitter_enabled;
                crate::std_checkbox(ui, &mut v, "P2 Network Jitter Buffer");
            });
            let row = new_row(ui, ww, ROW_H);
            lbl(ui, row, 0.0, 220.0, "Buffer depth");
            let mut v = rui.p2_jitter_depth_ms as f64;
            let mut c = child(ui, row, 226.0, 400.0, false, -6.0);
            c.add_enabled_ui(false, |ui| {
                crate::spin_buttons_full(ui, "rx_p2depth", &mut v, 5.0, 200.0, 5.0, 0, None, 76.0);
            });
            lbl(ui, row, 400.0, 440.0, "ms");
            let row = new_row(ui, ww, ROW_H);
            check(ui, row, 0.0, 420.0, &mut rui.rx_reserve_enabled, "RX Audio Network Reserve");
            let row = new_row(ui, ww, ROW_H);
            lbl(ui, row, 0.0, 220.0, "Audio reserve");
            let mut v = rui.rx_reserve_ms as f64;
            let mut c = child(ui, row, 226.0, 400.0, false, -6.0);
            c.add_enabled_ui(rui.rx_reserve_enabled, |ui| {
                if crate::spin_buttons_full(ui, "rx_reserve", &mut v, 5.0, 500.0, 5.0, 0, None, 76.0).changed() {
                    rui.rx_reserve_ms = v.round() as i32;
                }
            });
            lbl(ui, row, 400.0, 440.0, "ms");
            let row = new_row(ui, ww, ROW_H);
            check(ui, row, 0.0, 420.0, &mut rui.latency_correction, "RX Latency Correction");
        });
    }
    let _ = sep;
}
