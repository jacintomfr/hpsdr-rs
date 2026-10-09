//! Digital modes in the kiosk without the full-screen Digital window, so the spectrum and the waterfall stay in view while tuning (SSTV first).
//!
//! The spectrum and waterfall keep the top left (zoomed to x16 and shifted to show 0..6 kHz of audio like MMSSTV, restored when the window closes);
//! the top right is the picture window (the received picture, or the one to send on the TX tab, with its status); under both, the full width, is a
//! control strip: CLOSE, the mode chips RTTY | SSTV | RADE, the RX | TX tabs and the controls of the open tab (`render_sstv_panel`, drawn in two
//! parts: `SstvPart::Rx` / `SstvPart::Tx`) -- RX or TX, never both, so everything fits.
//!
//! The desktop and the other digital modes keep the Digital window (an OS viewport). Closing, Fit Filter, Quick Tune and Send / Abort do exactly what
//! the window's own handlers do.

use crate::{chip_button, lcd_kiosk_mode, touch_close_button, ConnectedState, DigitalMode, SstvPart};
use std::time::Duration;

const TAB_ID: &str = "sstv_inapp_tab";
const ZOOM_ID: &str = "digital_inapp_prev_zoom";
/// Spectrum zoom while the panel is open (x16 is the limit): 3 kHz of audio at the 48 kHz RX rate used meanwhile, shifted to start at the dial (below it for LSB)
/// MMSSTV's frequency display -- SSTV's 1.2..2.3 kHz is about a fifth of it.
const SSTV_ZOOM: i32 = 16;
/// Where the view is centred, in Hz from the dial: the middle of the SSTV tone band (1200..2300 Hz), so the signal sits mid-screen, clear of the dB ruler and the AGC lines at the left edge.
const VIEW_CENTER_HZ: f64 = 1500.0;
/// RX sample rate while the panel is open: at 48 kHz the zoom x16 shows exactly 3 kHz of audio (the MMSSTV view); the rate in use before is put back on close.
const SSTV_RATE: u32 = 48_000;
/// The sample rate to restore (0 = the rate was not changed). Also read when the configuration is saved, so a crash or exit with the panel open does not
/// leave 48 kHz as the saved rate.
static PREV_RATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The sample rate to write to the configuration: the one before the panel changed it, if it did.
pub(crate) fn persisted_rate(current: u32) -> u32 {
    match PREV_RATE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => current,
        _ => 1, // leaving the panel always goes back to zoom x1, so that is what the configuration keeps
    }
}

/// The zoom / pan before the panel changed them (0 / NaN-free sentinel: zoom 0 = not changed), so the configuration keeps the user's own view.
static PREV_ZOOM: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static PREV_PAN_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn persisted_zoom(current: i32) -> i32 {
    match PREV_ZOOM.load(std::sync::atomic::Ordering::Relaxed) {
        0 => current,
        _ => 1, // leaving the panel always goes back to zoom x1, so that is what the configuration keeps
    }
}

pub(crate) fn persisted_pan(current: f32) -> f32 {
    if PREV_ZOOM.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        current
    } else {
        0.0
    }
}
/// Height of the control strip under the spectrum / waterfall and the picture window.
/// The control strip (RX / TX tabs, Quick Tune, Decode, Lead, Slant, Load Picture ...) is kept but hidden for now: the layout is a bare picture window
/// next to a full-height spectrum / waterfall, and the controls get their place later. CLOSE is not needed: the DIGITAL button and the MIDI function toggle.
const SHOW_STRIP: bool = false;
const STRIP_HEIGHT_RX: f32 = 100.0;
const STRIP_HEIGHT_TX: f32 = 100.0;
/// Width of the picture (RX) window at the right; the spectrum and the waterfall take what is left.
pub(crate) const PANEL_WIDTH: f32 = 496.0;
const PAN_ID: &str = "digital_inapp_prev_pan";

/// True while the SSTV tab of the Digital window is open in the kiosk: the in-app layout is used instead of the viewport.
pub(crate) fn active(connected: &ConnectedState) -> bool {
    lcd_kiosk_mode() && connected.show_digital_window && connected.digital_mode == DigitalMode::Sstv
}

/// Every frame: zoom (and shift) the spectrum when the panel opens and put the previous zoom / pan back when it closes.
pub(crate) fn sync_zoom(connected: &mut ConnectedState, ctx: &egui::Context) {
    let id = egui::Id::new(ZOOM_ID);
    let pan_id = egui::Id::new(PAN_ID);
    let prev: Option<i32> = ctx.data(|d| d.get_temp(id));
    if active(connected) {
        if prev.is_none() {
            ctx.data_mut(|d| {
                d.insert_temp(id, connected.spectrum_zoom);
                d.insert_temp(pan_id, connected.spectrum_pan);
            });
            // Opening the Digital window already saved the user's own view (and put its own zoom 6 in): that is the one to keep in the configuration.
            let (orig_zoom, orig_pan) = match &connected.pre_digital_filters {
                Some(f) => (f.zoom, f.pan),
                None => (connected.spectrum_zoom, connected.spectrum_pan),
            };
            PREV_PAN_BITS.store(orig_pan.to_bits(), std::sync::atomic::Ordering::Relaxed);
            PREV_ZOOM.store(orig_zoom.max(1), std::sync::atomic::Ordering::Relaxed);
            connected.spectrum_zoom = SSTV_ZOOM;
            if connected.sample_rate > SSTV_RATE && connected.device.board != crate::Boards::Rx888 && PREV_RATE.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                PREV_RATE.store(connected.sample_rate, std::sync::atomic::Ordering::Relaxed);
                crate::change_sample_rate(connected, SSTV_RATE);
            }
            // Narrow the RX and TX passband to the SSTV tone band, like the Digital window did on opening. After the rate change: that rebuilds the
            // receiver (a new SpectrumHandle), which would drop an explicit passband set before it.
            fit_filter(connected);
        }
        // Keep the 0..6 kHz audio range in view (the sign follows the sideband).
        let half = connected.sample_rate as f64 / 2.0;
        let max_pan = half - half / SSTV_ZOOM as f64;
        let lower = matches!(connected.spectrum.mode(), crate::spectrum::Mode::Lsb | crate::spectrum::Mode::Digl);
        let center = if lower { -VIEW_CENTER_HZ } else { VIEW_CENTER_HZ };
        connected.spectrum_pan = if max_pan > 0.0 { (center / max_pan).clamp(-1.0, 1.0) as f32 } else { 0.0 };
    } else if let Some(z) = prev {
        // Closing the Digital window has put the user's own zoom and pan back already (restore_pre_digital_filters): leave them alone. Only when the
        // window stays open (another digital mode took over) the Digital window's own zoom is restored.
        let digital_still_open = connected.pre_digital_filters.is_some();
        if digital_still_open {
            connected.spectrum_zoom = z;
        }
        PREV_ZOOM.store(0, std::sync::atomic::Ordering::Relaxed);
        let prev_rate = PREV_RATE.swap(0, std::sync::atomic::Ordering::Relaxed);
        if prev_rate != 0 && connected.sample_rate != prev_rate {
            crate::change_sample_rate(connected, prev_rate);
        }
        if digital_still_open {
            if let Some(p) = ctx.data(|d| d.get_temp::<f32>(pan_id)) {
                connected.spectrum_pan = p;
            }
        }
        ctx.data_mut(|d| {
            d.remove_temp::<i32>(id);
            d.remove_temp::<f32>(pan_id);
        });
    }
}

fn tab(ctx: &egui::Context) -> u8 {
    ctx.data(|d| d.get_temp::<u8>(egui::Id::new(TAB_ID))).unwrap_or(0)
}

/// Narrow the RX / TX passband to the SSTV tone band (sync 1200 Hz .. white 2300 Hz, plus a margin), as the window's Fit Filter.
fn fit_filter(connected: &mut ConnectedState) {
    let mode = connected.spectrum.mode();
    let (low, high) = (1200.0 - 100.0, 2300.0 + 100.0);
    let passband = if matches!(mode, crate::spectrum::Mode::Lsb | crate::spectrum::Mode::Digl) { (-high, -low) } else { (low, high) };
    connected.spectrum.set_explicit_passband(Some(passband));
    if let Some(tx) = &connected.tx_handle {
        tx.set_explicit_passband(Some(passband));
    }
}

fn quick_tune(connected: &mut ConnectedState, hz: u32) {
    connected.active_xvtr = None;
    connected.session.set_frequency(hz);
    connected.ctun_frequency_hz = hz;
    let new_mode = crate::digi_mode_for_band(hz);
    connected.spectrum.set_mode(new_mode);
    if let Some(tx) = &connected.tx_handle {
        tx.set_mode(new_mode);
    }
    // The band's sign (LSB / USB) may have changed.
    fit_filter(connected);
}

fn close_window(connected: &mut ConnectedState) {
    // Restore normal mode-based filtering and the mode / filters from before the Digital window was opened.
    connected.spectrum.set_explicit_passband(None);
    if let Some(tx) = &connected.tx_handle {
        tx.set_explicit_passband(None);
    }
    connected.show_digital_window = false;
    if let Some(prev) = connected.pre_digital_mode.take() {
        connected.spectrum.set_mode(prev);
    }
    crate::restore_pre_digital_filters(connected);
}

/// The panel: the picture window pinned to `win` (right of the spectrum and the waterfall) and the control strip pinned to `strip` (under both, the
/// full width). One tab at a time: RX shows the received picture and the receive controls, TX the picture to send and the transmit controls.
pub(crate) fn panel(ui: &mut egui::Ui, connected: &mut ConnectedState, win: egui::Rect, strip: egui::Rect) {
    let mut close_now = false;
    let mut switch_to: Option<DigitalMode> = None;
    let mut tab_now = tab(ui.ctx());
    let sstv = connected.sstv.clone();
    let tx_available = connected.tx_enabled && connected.tx_handle.is_some();
    let mox = connected.session.mox.load(std::sync::atomic::Ordering::Relaxed);
    let snap = sstv.snapshot();
    let mut fit = false;
    let mut quick: Option<u32> = None;
    let mut mox_request: Option<bool> = None;
    let amber = egui::Color32::from_rgb(230, 150, 50);
    let green = egui::Color32::from_rgb(40, 190, 70);

    // ---- The picture window: the picture on the left, its status on the right.
    egui::Area::new(egui::Id::new("digital_inapp_window")).fixed_pos(win.min).constrain(false).show(ui, |ui| {
        let frame = egui::Frame::group(ui.style());
        let margin = frame.total_margin().sum();
        let (inner_w, inner_h) = (win.width() - margin.x, win.height() - margin.y - 2.0);
        frame.show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_height(inner_h);
            crate::apply_kiosk_touch_style(ui);
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
            // The picture takes everything but two thin vertical bars (Sync, Level) at its right and the status line under it.
            let bars_w = 104.0f32;
            // TX: one line at the very bottom (Load Picture, Mode, Callsign banner) and Send / Abort in the right column, so the picture gets the rest.
            let bottom_h = if tab_now == 1 { 50.0f32 } else { 0.0f32 };
            let pic_w = inner_w - bars_w - 8.0;
            let pic_h = inner_h - bottom_h;
            let sync_color = if snap.sync_quality > 0.7 {
                green
            } else if snap.sync_quality > 0.3 {
                amber
            } else {
                egui::Color32::from_rgb(140, 140, 140)
            };
            let y0 = ui.cursor().top();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(pic_w, pic_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(pic_w);
                    ui.set_max_width(pic_w);
                    if tab_now == 1 {
                        match (&connected.sstv_tx_texture, &connected.sstv_tx_prepared) {
                            (Some(tex), Some((w, h, _))) => {
                                let scale = (pic_w / *w as f32).min(pic_h / *h as f32);
                                ui.image((tex.id(), egui::vec2(*w as f32 * scale, *h as f32 * scale)));
                            }
                            _ => {
                                ui.add(egui::Label::new(egui::RichText::new("No TX picture: Load Picture...").weak()).wrap());
                            }
                        }
                    } else if snap.w > 0 && snap.h > 0 && snap.rgb.len() == snap.w as usize * snap.h as usize * 3 {
                        let size = [snap.w as usize, snap.h as usize];
                        let pixels: Vec<egui::Color32> = snap.rgb.chunks_exact(3).map(|p| egui::Color32::from_rgb(p[0], p[1], p[2])).collect();
                        let image = egui::ColorImage::new(size, pixels);
                        match &mut connected.sstv_texture {
                            Some(tex) if connected.sstv_texture_image_id == snap.image_id => tex.set(image, egui::TextureOptions::LINEAR),
                            slot => {
                                *slot = Some(ui.ctx().load_texture("sstv_image", image, egui::TextureOptions::LINEAR));
                                connected.sstv_texture_image_id = snap.image_id;
                            }
                        }
                        if let Some(tex) = &connected.sstv_texture {
                            let scale = (pic_w / snap.w as f32).min(pic_h / snap.h as f32).max(0.05);
                            ui.image((tex.id(), egui::vec2(snap.w as f32 * scale, snap.h as f32 * scale)));
                        }
                    } else {
                        ui.add(egui::Label::new(egui::RichText::new("No picture yet -- waiting for a VIS header.").weak()).wrap());
                    }
                });
                // Three vertical bars (Sync coloured by quality, the input Level, the picture progress) and under them what the decoder is doing.
                ui.allocate_ui_with_layout(egui::vec2(bars_w, pic_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_max_width(bars_w);
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                    let bar_h = (pic_h - 22.0 - 64.0 - if tab_now == 1 { 104.0 } else { 0.0 }).max(40.0);
                    let (area, _) = ui.allocate_exact_size(egui::vec2(bars_w, if tab_now == 1 { 0.0 } else { bar_h + 22.0 }), egui::Sense::hover());
                    let pct = format!("{:.0}%", snap.progress * 100.0);
                    // The receiver is idle while sending, so on the TX tab the bars are the transmission's: its progress, and whether it is on air.
                    let tx_tab = tab_now == 1;
                                        let bars: Vec<(&str, f32, egui::Color32)> = if tx_tab {
                        Vec::new()
                    } else {
                        vec![
                            ("Sync", snap.sync_quality, sync_color),
                            ("Lvl", snap.level, egui::Color32::from_rgb(90, 160, 230)),
                            (pct.as_str(), snap.progress, egui::Color32::from_rgb(230, 150, 50)),
                        ]
                    };
                    for (i, (label, value, color)) in bars.iter().enumerate() {
                        let x = area.left() + i as f32 * 36.0;
                        let track = egui::Rect::from_min_size(egui::pos2(x, area.top()), egui::vec2(28.0, bar_h));
                        ui.painter().rect_filled(track, 4.0, egui::Color32::from_gray(20));
                        let fill_h = bar_h * value.clamp(0.0, 1.0);
                        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(track.left(), track.bottom() - fill_h), track.right_bottom()), 4.0, *color);
                        ui.painter().rect_stroke(track, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(70)), egui::StrokeKind::Inside);
                        ui.painter().text(egui::pos2(track.center().x, track.bottom() + 11.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(13.0), egui::Color32::from_gray(190));
                    }
                    if tx_tab {
                        // The transmission's progress, horizontal, at the top of the right column.
                        ui.add(egui::ProgressBar::new(sstv.tx_progress().clamp(0.0, 1.0)).desired_width(bars_w - 4.0).desired_height(24.0).fill(egui::Color32::from_rgb(230, 70, 70)).text(format!("{:.0}%", sstv.tx_progress() * 100.0)));
                    }
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        if tx_tab {
                            let on_air = sstv.tx_active();
                            ui.painter().circle_filled(r.center(), 5.0, if on_air { egui::Color32::from_rgb(230, 70, 70) } else { amber });
                            ui.add(egui::Label::new(if on_air { "ON AIR" } else { "ready" }).wrap());
                        } else {
                            ui.painter().circle_filled(r.center(), 5.0, if snap.receiving { green } else { amber });
                            ui.add(egui::Label::new(match snap.detected {
                                Some(m) => format!("{}\n{}", m.label(), if snap.receiving { "(receiving)" } else { "(last)" }),
                                None => "hunting...".to_string(),
                            }).wrap());
                        }
                    });
                    if let Some(id) = &snap.rx_id {
                        ui.add(egui::Label::new(format!("Station: {id}")).wrap());
                    }
                    if let Some(u) = &snap.unsupported {
                        ui.add(egui::Label::new(egui::RichText::new(format!("{u}: not decoded")).color(amber)).wrap());
                    }
                    if tx_tab {
                        // Send keys MOX and plays the prepared picture; Abort stops it. The receiver tab never needs them.
                        let ready = connected.sstv_tx_prepared.is_some();
                        let size = egui::vec2(bars_w - 4.0, 40.0);
                        if ui.add_enabled(ready && tx_available && !sstv.tx_active(), egui::Button::new(egui::RichText::new("Send").size(22.0).strong()).min_size(egui::vec2(size.x, 46.0))).clicked() {
                            if let Some((w, h, rgb)) = connected.sstv_tx_prepared.as_ref() {
                                sstv.set_image(connected.sstv_tx_mode, rgb, *w, *h, connected.own_callsign.as_str());
                                sstv.set_tx_armed(true);
                                mox_request = Some(true);
                            }
                        }
                        ui.add_space(40.0);
                        if ui.add_enabled(sstv.tx_active(), egui::Button::new(egui::RichText::new("Abort").size(22.0).strong()).min_size(egui::vec2(size.x, 46.0))).clicked() {
                            sstv.abort_tx();
                            mox_request = Some(false);
                        }
                    }
                });
            });
            if tab_now == 1 {
                // TX: the picture-file line (Load Picture, Mode, banner) at the very bottom of the window; Send / Abort are in the right column above.
                let used = ui.cursor().top() - y0;
                ui.add_space((inner_h - used - 46.0).max(0.0));
                ui.allocate_ui_with_layout(egui::vec2(inner_w, 44.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                let (f, q, m) = crate::render_sstv_panel(
                    ui,
                    &sstv,
                    &mut connected.sstv_texture,
                    &mut connected.sstv_texture_image_id,
                    &mut connected.sstv_tx_source,
                    &mut connected.sstv_tx_mode,
                    &mut connected.sstv_tx_banner,
                    &mut connected.sstv_tx_prepared,
                    &mut connected.sstv_tx_texture,
                    &mut connected.own_callsign,
                    tx_available,
                    mox,
                    SstvPart::TxFile,
                );
                fit |= f;
                quick = quick.or(q);
                mox_request = mox_request.or(m);
                });
            } else {
            }
        });
    });

    // ---- The control strip: the header (CLOSE, mode chips, RX | TX) and the controls of the open tab.
    // ---- OPTIONS: a button at the bottom left of the waterfall; open, the controls fill the waterfall (the spectrum stays in view for tuning).
    let opt_id = egui::Id::new("digital_inapp_options_open");
    let mut options_open: bool = ui.ctx().data(|d| d.get_temp(opt_id)).unwrap_or(false);
    if !options_open {
        egui::Area::new(egui::Id::new("digital_inapp_options_button"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(strip.left() + 6.0, strip.bottom() - 46.0 + 2.0))
            .constrain(false)
            .show(ui, |ui| {
                crate::apply_kiosk_touch_style(ui);
                // A centred-and-justified cell, so the label sits in the middle of the box.
                let clicked = ui
                    .allocate_ui_with_layout(egui::vec2(120.0, 40.0), egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                        ui.add(chip_button("OPTIONS", false).min_size(egui::vec2(120.0, 40.0))).clicked()
                    })
                    .inner;
                if clicked {
                    options_open = true;
                }
            });
    }
    if options_open {
    egui::Area::new(egui::Id::new("digital_inapp_strip")).fixed_pos(egui::pos2(strip.left(), strip.bottom() - 2.0)).pivot(egui::Align2::LEFT_BOTTOM).order(egui::Order::Foreground).constrain(false).show(ui, |ui| {
        let frame = egui::Frame::group(ui.style()).fill(ui.visuals().panel_fill);
        let margin = frame.total_margin().sum();
        let inner_w = strip.width() - margin.x;
        frame.show(ui, |ui| {
            ui.set_width(inner_w);
            crate::apply_kiosk_touch_style(ui);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            ui.horizontal(|ui| {
                if ui.add(chip_button("OPTIONS", true).min_size(egui::vec2(120.0, 40.0))).clicked() {
                    options_open = false;
                }
                ui.add_space(6.0);
                for (i, label) in ["RX", "TX"].iter().enumerate() {
                    if ui.add(chip_button(label, tab_now == i as u8).min_size(egui::vec2(52.0, 40.0))).clicked() {
                        tab_now = i as u8;
                    }
                }
                ui.add_space(6.0);
                if tab_now == 0 {
                    // RX: the calling frequencies (popup above) and Auto-save.
                    crate::sstv_quick_popup(ui, &mut quick);
                    let mut auto_save = sstv.rx_auto_save();
                    if crate::std_checkbox(ui, &mut auto_save, "Auto-save").changed() {
                        sstv.set_rx_auto_save(auto_save);
                    }
                } else {
                    // TX: arm the picture transmission, and the FSK ID.
                    let armed = sstv.tx_armed();
                    if ui
                        .add_enabled(tx_available, egui::Button::selectable(armed, egui::RichText::new("TX").strong()).min_size(egui::vec2(52.0, 40.0)))
                        .clicked()
                    {
                        sstv.set_tx_armed(!armed);
                    }
                    if armed && mox && sstv.tx_active() {
                        ui.colored_label(egui::Color32::from_rgb(230, 70, 70), "ON AIR");
                    }
                    let mut fsk_id = sstv.tx_fsk_id_enabled();
                    if crate::std_checkbox(ui, &mut fsk_id, "FSK ID").changed() {
                        sstv.set_tx_fsk_id_enabled(fsk_id);
                    }
                }
            });
            let _ = touch_close_button;
            let part = if tab_now == 0 { SstvPart::Rx } else { SstvPart::Tx };
            let (f, q, m) = crate::render_sstv_panel(
                ui,
                &sstv,
                &mut connected.sstv_texture,
                &mut connected.sstv_texture_image_id,
                &mut connected.sstv_tx_source,
                &mut connected.sstv_tx_mode,
                &mut connected.sstv_tx_banner,
                &mut connected.sstv_tx_prepared,
                &mut connected.sstv_tx_texture,
                &mut connected.own_callsign,
                tx_available,
                mox,
                part,
            );
            fit |= f;
            quick = quick.or(q);
            mox_request = mox_request.or(m);
        });
    });
    }
    ui.ctx().data_mut(|d| d.insert_temp(opt_id, options_open));
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(TAB_ID), tab_now));
    ui.ctx().request_repaint_after(Duration::from_millis(300));

    let mut changed = false;
    if fit {
        fit_filter(connected);
        changed = true;
    }
    if let Some(hz) = quick {
        quick_tune(connected, hz);
        changed = true;
    }
    if let Some(want) = mox_request {
        // Send / Abort key MOX themselves (see ConnectedState::sstv_tx_sending); the auto-drop once the picture is sent is polled elsewhere.
        connected.session.set_mox(want);
        connected.sstv_tx_sending = want;
    }
    if let Some(m) = switch_to {
        // The Digital window takes over for the other modes (it refits the passband on its first frame).
        connected.digital_mode = m;
        connected.digital_refit = true;
        changed = true;
    }
    if close_now {
        close_window(connected);
        changed = true;
    }
    if changed {
        connected.settings_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Height of the control strip: the receive tab needs two lines, the transmit tab three.
pub(crate) fn strip_height(ctx: &egui::Context) -> f32 {
    if !SHOW_STRIP {
        0.0
    } else if tab(ctx) == 1 {
        STRIP_HEIGHT_TX
    } else {
        STRIP_HEIGHT_RX
    }
}
