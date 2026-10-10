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

/// The plain-language translation of the SYNOP reports in the RTTY text (the DEC button): one buffer for the one RTTY panel.
static SYNOP_VIEW: std::sync::Mutex<Option<crate::synop::SynopView>> = std::sync::Mutex::new(None);
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
        prev => prev,
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

/// True while the SSTV or RTTY tab of the Digital window is open in the kiosk: the in-app layout is used instead of the viewport.
pub(crate) fn active(connected: &ConnectedState) -> bool {
    lcd_kiosk_mode() && connected.show_digital_window && matches!(connected.digital_mode, DigitalMode::Sstv | DigitalMode::Rtty)
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

/// Narrow the RX / TX passband to the tones of the open mode (SSTV: sync 1200 Hz .. white 2300 Hz; RTTY: the mark / space pair), plus a margin, as the window's Fit Filter.
fn fit_filter(connected: &mut ConnectedState) {
    let mode = connected.spectrum.mode();
    let s = connected.rtty.settings();
    let passband = crate::digital_fit_passband(connected.digital_mode, mode, s.center_hz, s.shift_hz);
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

/// The panel of the open mode: SSTV (the picture window) or RTTY (the text window), pinned to `win`, with the OPTIONS controls over the waterfall at `strip`.
pub(crate) fn panel(ui: &mut egui::Ui, connected: &mut ConnectedState, win: egui::Rect, strip: egui::Rect) {
    match connected.digital_mode {
        DigitalMode::Rtty => rtty_panel(ui, connected, win, strip),
        _ => sstv_panel(ui, connected, win, strip),
    }
}

/// The SSTV panel: the picture window pinned to `win` (right of the spectrum and the waterfall) and the control strip pinned to `strip` (under both, the
/// full width). One tab at a time: RX shows the received picture and the receive controls, TX the picture to send and the transmit controls.
fn sstv_panel(ui: &mut egui::Ui, connected: &mut ConnectedState, win: egui::Rect, strip: egui::Rect) {
    let mut close_now = false;
    let mut switch_to: Option<DigitalMode> = None;
    let mut tab_now = tab(ui.ctx()).min(1);
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

/// The DEC window of the RTTY panel: the SYNOP reports of the received text in plain language, large enough to read like a commercial decoder (not full
/// screen: the toolbar stays visible). Returns false when CLOSE was pressed.
fn weather_window(ui: &mut egui::Ui, raw_text: &str, win: egui::Rect, strip: egui::Rect) -> bool {
    let mut open = true;
    let amber = egui::Color32::from_rgb(230, 150, 50);
    let screen = ui.ctx().content_rect();
    let width = (screen.width() - 2.0 * 60.0).min(860.0);
    let height = (strip.bottom().max(win.bottom()) - 56.0).clamp(300.0, screen.height() - 130.0);
    let rect = egui::Rect::from_min_size(egui::pos2(screen.center().x - width / 2.0, 56.0), egui::vec2(width, height));
    let mut guard = SYNOP_VIEW.lock().unwrap();
    let view = guard.get_or_insert_with(Default::default);
    if view.update(raw_text) {
        ui.ctx().request_repaint_after(Duration::from_millis(120));
    }
    egui::Area::new(egui::Id::new("digital_inapp_weather_window")).order(egui::Order::Foreground).fixed_pos(rect.min).constrain(false).show(ui, |ui| {
        let frame = egui::Frame::popup(ui.style()).stroke(egui::Stroke::new(1.5, ui.visuals().widgets.active.bg_stroke.color));
        let margin = frame.total_margin().sum();
        frame.show(ui, |ui| {
            ui.set_width(rect.width() - margin.x);
            ui.set_height(rect.height() - margin.y);
            crate::apply_kiosk_touch_style(ui);
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            ui.label(egui::RichText::new("Weather reports (SYNOP) in plain language").strong().size(26.0));
            ui.separator();
            let list_h = ui.available_height() - 62.0;
            egui::ScrollArea::vertical().id_salt("rtty_weather_list").max_height(list_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if view.lines().is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new("Waiting for a weather report (AAXX / BBXX) ...").size(24.0).color(amber)).wrap());
                }
                for line in view.lines() {
                    if line.starts_with("---") {
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(egui::RichText::new(line.trim_matches('-').trim()).strong().size(26.0).color(amber)).wrap());
                    } else if let Some((head, rest)) = line.split_once(": ") {
                        ui.add(egui::Label::new(egui::RichText::new(head).strong().size(26.0)).wrap());
                        ui.add(egui::Label::new(egui::RichText::new(rest.replace(", ", "  \u{b7}  ")).size(24.0)).wrap());
                        ui.separator();
                    } else {
                        ui.add(egui::Label::new(egui::RichText::new(line).size(24.0)).wrap());
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("CLOSE").size(22.0).strong()).min_size(egui::vec2(150.0, 50.0))).clicked() {
                    open = false;
                }
            });
        });
    });
    open
}

/// The RTTY panel, same pattern as SSTV: the text window at the right (the lock status and the decoded text; on the TX tab also the line to send)
/// and an OPTIONS button at the bottom left of the waterfall that opens the controls (RX tab: Center, Baud, Shift, Reverse / AFC / Squelch, Clear RX, Fit Filter;
/// TX tab: TX ON, CALL CQ, CLEAR, Send on Return) over the waterfall. The spectrum above shows 3 kHz of audio with the mark / space cursors.
fn rtty_panel(ui: &mut egui::Ui, connected: &mut ConnectedState, win: egui::Rect, strip: egui::Rect) {
    let rtty = connected.rtty.clone();
    let mut tab_now = tab(ui.ctx());
    let tx_available = connected.tx_enabled && connected.tx_handle.is_some();
    let mox = connected.session.mox.load(std::sync::atomic::Ordering::Relaxed);
    let st = rtty.rx_status();
    let mut s = rtty.settings();
    let s0 = s;
    let mode = connected.spectrum.mode();
    let mut fit = false;
    let mut mox_request: Option<bool> = None;
    let mut send_on_return = connected.rtty_send_on_return;
    // DEC: a window with the weather reports in plain language; it is not remembered, CLOSE (or leaving the panel) turns DEC off.
    let dec_id = egui::Id::new("digital_inapp_dec_open");
    let mut decode: bool = ui.ctx().data(|d| d.get_temp(dec_id)).unwrap_or(false);
    let raw_text = rtty.rx_text();
    let callsign = connected.own_callsign.clone();
    let mut new_tx_input: Option<String> = None;
    let amber = egui::Color32::from_rgb(230, 150, 50);
    let red = egui::Color32::from_rgb(220, 50, 50);
    let green = egui::Color32::from_rgb(40, 190, 70);

    // ---- The text window: status, decoded text and, on the TX tab, the line to send.
    egui::Area::new(egui::Id::new("digital_inapp_window")).fixed_pos(win.min).constrain(false).show(ui, |ui| {
        let frame = egui::Frame::group(ui.style());
        let margin = frame.total_margin().sum();
        let (inner_w, inner_h) = (win.width() - margin.x, win.height() - margin.y - 2.0);
        frame.show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_height(inner_h);
            crate::apply_kiosk_touch_style(ui);
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let color = if st.lock >= 1.0 { green } else if st.lock > 0.0 { amber } else { red };
                ui.painter().circle_filled(r.center(), 6.0, color);
                ui.label(if st.lock >= 1.0 { "LOCK" } else { "no lock" });
                ui.add(egui::ProgressBar::new(st.confidence.clamp(0.0, 1.0)).desired_width(150.0).desired_height(20.0).text(format!("conf {:.0}%", st.confidence * 100.0)));
                if s.afc {
                    ui.label(format!("AFC {:+.0} Hz", st.afc_offset_hz));
                }
                // DEC at the right end of the status row: opens the weather window (its CLOSE turns it off again).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(chip_button("DEC", decode).min_size(egui::vec2(64.0, 36.0)))
                        .on_hover_text("Translate the weather reports (SYNOP) into plain language in a larger window")
                        .clicked()
                    {
                        decode = true;
                    }
                });
            });
            if !matches!(mode, crate::spectrum::Mode::Usb | crate::spectrum::Mode::Digu | crate::spectrum::Mode::Lsb | crate::spectrum::Mode::Digl) {
                ui.colored_label(amber, "RTTY needs USB/DIGU (or LSB/DIGL with Reverse).");
            }
            let tx_tab = tab_now == 1;
            let tx_h = if tx_tab { 128.0 } else { 0.0 };
            let rx_h = (ui.available_height() - tx_h - 4.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("rtty_inapp_rx").max_height(rx_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                ui.label(egui::RichText::new(&raw_text).monospace().size(15.0));
            });
            if tx_tab {
                ui.separator();
                ui.horizontal(|ui| {
                    let (sent, total) = rtty.tx_progress();
                    ui.label(format!("Sent {sent}/{total}"));
                    if mox {
                        ui.colored_label(red, "ON AIR");
                    }
                    if !tx_available {
                        ui.weak("TX unavailable");
                    }
                });
                ui.add_enabled_ui(tx_available, |ui| {
                    ui.horizontal(|ui| {
                        // Return sends the line (Send on Return): consumed before the text box is built, or it would become a newline.
                        let tx_id = ui.id().with("rtty_inapp_tx_edit");
                        let enter = send_on_return && ui.memory(|m| m.has_focus(tx_id)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        let box_w = (ui.available_width() - 100.0).max(100.0);
                        egui::Frame::new()
                            .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color))
                            .corner_radius(5.0)
                            .inner_margin(egui::Margin::symmetric(4, 3))
                            .show(ui, |ui| {
                                ui.set_width(box_w);
                                egui::ScrollArea::vertical().id_salt("rtty_inapp_tx_scroll").max_height(64.0).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut connected.rtty_tx_input)
                                            .id(tx_id)
                                            .desired_rows(3)
                                            .desired_width(f32::INFINITY)
                                            .hint_text(if send_on_return { "Type a line, Return sends it..." } else { "Text to send" }),
                                    );
                                });
                            });
                        let send = ui.add(egui::Button::new(egui::RichText::new("Send").size(20.0).strong()).min_size(egui::vec2(88.0, 46.0))).clicked();
                        if (send || enter) && !connected.rtty_tx_input.is_empty() {
                            rtty.send_text(&connected.rtty_tx_input);
                            connected.rtty_tx_input.clear();
                            mox_request = Some(true);
                        }
                    });
                });
            }
        });
    });

    // ---- OPTIONS: a button at the bottom left of the waterfall; open, the controls fill the waterfall (the spectrum stays in view for tuning).
    let opt_id = egui::Id::new("digital_inapp_options_open");
    let mut options_open: bool = ui.ctx().data(|d| d.get_temp(opt_id)).unwrap_or(false);
    // The weather window is in front of the OPTIONS button and strip: they step aside while it is open.
    if decode {
        options_open = false;
    }
    if !options_open && !decode {
        egui::Area::new(egui::Id::new("digital_inapp_options_button"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(strip.left() + 6.0, strip.bottom() - 46.0 + 2.0))
            .constrain(false)
            .show(ui, |ui| {
                crate::apply_kiosk_touch_style(ui);
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
        egui::Area::new(egui::Id::new("digital_inapp_strip"))
            .fixed_pos(egui::pos2(strip.left(), strip.bottom() - 2.0))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .order(egui::Order::Foreground)
            .constrain(false)
            .show(ui, |ui| {
                let frame = egui::Frame::group(ui.style()).fill(ui.visuals().panel_fill);
                let margin = frame.total_margin().sum();
                let inner_w = strip.width() - margin.x;
                frame.show(ui, |ui| {
                    ui.set_width(inner_w);
                    crate::apply_kiosk_touch_style(ui);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    ui.horizontal(|ui| {
                        if ui.add(chip_button("OPTIONS", true).min_size(egui::vec2(120.0, 40.0))).clicked() {
                            options_open = false;
                        }
                        ui.add_space(6.0);
                        for (i, label) in ["RX", "TX", "SET"].iter().enumerate() {
                            if ui.add(chip_button(label, tab_now == i as u8).min_size(egui::vec2(52.0, 40.0))).clicked() {
                                tab_now = i as u8;
                            }
                        }
                        ui.add_space(6.0);
                        if tab_now == 1 {
                            let label = if mox { "TX ON" } else { "TX" };
                            if ui.add_enabled(tx_available, egui::Button::selectable(mox, egui::RichText::new(label).strong()).min_size(egui::vec2(80.0, 40.0))).on_hover_text("Click to key / unkey PTT directly").clicked() {
                                mox_request = Some(!mox);
                            }
                            if mox {
                                ui.colored_label(red, "ON AIR");
                            }
                        }
                    });
                    match tab_now {
                        0 => {
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new("Clear RX").min_size(egui::vec2(96.0, 40.0))).clicked() {
                                    rtty.clear_rx_text();
                                    *SYNOP_VIEW.lock().unwrap() = None;
                                }
                                if ui.add(egui::Button::new("Fit Filter").min_size(egui::vec2(96.0, 40.0))).on_hover_text("Fit the RX / TX filter to the mark / space pair").clicked() {
                                    fit = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                crate::std_checkbox(ui, &mut s.reverse, "Reverse");
                                crate::std_checkbox(ui, &mut s.afc, "AFC");
                                crate::std_checkbox(ui, &mut s.squelch, "Squelch").on_hover_text("Discard decoded text while the signal is too noise-dominated to trust");
                            });
                        }
                        1 => {
                            ui.horizontal(|ui| {
                                if ui.add_enabled(tx_available, egui::Button::new("CALL CQ").min_size(egui::vec2(96.0, 40.0))).on_hover_text("Queue the CQ call and key PTT").clicked() {
                                    let call = if callsign.trim().is_empty() { "NOCALL".to_string() } else { callsign.clone() };
                                    let cq = format!("CQ CQ CQ DE {call} {call} {call} PSE K\n");
                                    rtty.clear_tx();
                                    rtty.send_text(&cq);
                                    new_tx_input = Some(cq);
                                    mox_request = Some(true);
                                }
                                if ui.add(egui::Button::new("CLEAR").min_size(egui::vec2(80.0, 40.0))).on_hover_text("Drop whatever is still queued - TX itself stays as it was").clicked() {
                                    rtty.clear_tx();
                                    new_tx_input = Some(String::new());
                                }
                            });
                            ui.horizontal(|ui| {
                                crate::std_checkbox(ui, &mut send_on_return, "Send on Return");
                            });
                        }
                        _ => {
                            // SET: the tone pair. Center first (with Fit Filter to re-fit after changing it), then baud and shift on one row.
                            ui.horizontal(|ui| {
                                ui.label("Center:");
                                crate::spin_buttons_full(ui, "rtty_inapp_center", &mut s.center_hz, 300.0, 3000.0, 10.0, 0, None, 64.0);
                                ui.label("Hz");
                                ui.add_space(8.0);
                                if ui.add(egui::Button::new("Fit Filter").min_size(egui::vec2(96.0, 40.0))).on_hover_text("Fit the RX / TX filter to the mark / space pair").clicked() {
                                    fit = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.spacing_mut().button_padding = egui::vec2(5.0, 6.0);
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label("Baud");
                                for b in crate::rtty_link::BAUD_CHOICES {
                                    if ui.add(chip_button(&format!("{b}"), s.baud == b).min_size(egui::vec2(38.0, 40.0))).clicked() {
                                        s.baud = b;
                                    }
                                }
                                ui.add_space(6.0);
                                ui.label("Shift");
                                for sh in crate::rtty_link::SHIFT_CHOICES {
                                    if ui.add(chip_button(&format!("{sh:.0}"), s.shift_hz == sh).min_size(egui::vec2(38.0, 40.0))).clicked() {
                                        s.shift_hz = sh;
                                    }
                                }
                            });
                        }
                    }
                });
            });
    }
    if decode {
        decode = weather_window(ui, &raw_text, win, strip);
    } else {
        *SYNOP_VIEW.lock().unwrap() = None;
    }
    ui.ctx().data_mut(|d| d.insert_temp(dec_id, decode));
    ui.ctx().data_mut(|d| d.insert_temp(opt_id, options_open));
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(TAB_ID), tab_now));
    ui.ctx().request_repaint_after(Duration::from_millis(300));

    let mut changed = false;
    if s != s0 {
        rtty.set_settings(s);
        changed = true;
    }
    if send_on_return != connected.rtty_send_on_return {
        connected.rtty_send_on_return = send_on_return;
        changed = true;
    }
    if let Some(t) = new_tx_input {
        connected.rtty_tx_input = t;
    }
    if fit {
        fit_filter(connected);
        changed = true;
    }
    if let Some(want) = mox_request {
        // A persistent hold the operator controls directly (no auto-drop), like the Digital window's TX row.
        connected.session.set_mox(want);
    }
    if changed {
        connected.settings_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
