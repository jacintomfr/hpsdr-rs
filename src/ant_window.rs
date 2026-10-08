//! The "Ant" window (deskHPSDR ant_menu.c): the per-band RX and TX antenna of the Alex front end, HF bands (with Gen) on one page and the
//! transverter bands on the other, two band groups side by side (Band | RX Ant | TX Ant, twice) like the original. Same layout family as the OC
//! Output window: it stops above the toolbar (which stays visible), square corners, no shadow; the long description that headed the old Settings
//! page is behind the round "!". Values are the same `ConnectedState::antenna_settings` the old Settings -> Antenna tab edited (by band name), and
//! "ANAN 100/200 new PA board" the same `session.new_pa_board`.

use crate::noise_window::choice_combo_h;
use crate::tx_window::{child, new_row};
use crate::{chip_button, help_button, kiosk_accent_button, touch_checkbox_sized, AntennaMask, Boards, ConnectedState};
use std::sync::atomic::Ordering;

const ROW_H: f32 = 48.0;
const COMBO_H: f32 = 40.0;
/// One group: band label, RX combo, TX combo.
const LABEL_W: f32 = 78.0;
const RX_W: f32 = 120.0;
const TX_W: f32 = 120.0;
const GROUP_W: f32 = LABEL_W + RX_W + 12.0 + TX_W;
const GROUP_GAP: f32 = 60.0;

/// (port number, label) -- RX gets all six ports, TX only ANT1-3 (Ext / XVTR are RX-only, as in the reference).
const RX_PORTS: [(u32, &str); 6] = [(0, "Ant1"), (1, "Ant2"), (2, "Ant3"), (3, "Ext1"), (4, "Ext2"), (5, "Xvtr")];
const TX_PORTS: [(u32, &str); 3] = [(0, "Ant1"), (1, "Ant2"), (2, "Ant3")];

const ANT_HELP: &str = "Alex's RX antenna ports (Ant1-3, Ext1/Ext2, Xvtr) and TX antenna ports (Ant1-3 only -- Ext / Xvtr are RX-only, as in the reference), configured per band -- e.g. to receive on a separate listening antenna or a transverter's IF port, or to transmit into a dummy load or a different antenna than you receive on.\n\nDriven by the primary front end's band, shared across every receiver -- not a per-extra-receiver setting.";
const NEW_PA_HELP: &str = "ANAN-100/200: there is an \"old\" (Rev. 15/16) and a \"new\" (Rev. 24) PA board, which differ in the relay settings for using Ext1 / Ext2 and in how PureSignal feedback is done. There is no way to detect which one is installed. Only matters if you use Ext1 / Ext2 / Xvtr as an RX antenna: if their reception does not work, try toggling this.";

fn hf_names(connected: &ConnectedState) -> Vec<String> {
    crate::BANDS
        .iter()
        .filter(|band| (band.low() as u64) >= connected.device.frequency_min && (band.high() as u64) <= connected.device.frequency_max)
        .map(|band| band.name.to_string())
        .chain(std::iter::once("Gen".to_string()))
        .collect()
}

fn xvtr_names(connected: &ConnectedState) -> Vec<String> {
    connected.xvtrs.iter().filter(|x| !x.name.is_empty()).map(|x| x.name.clone()).collect()
}

/// Returns (close, changed).
pub fn ant_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let page_id = egui::Id::new("ant_window_page");
    let hf = hf_names(connected);
    let xv = xvtr_names(connected);
    let mut page: u8 = ui.ctx().data(|d| d.get_temp(page_id)).unwrap_or(0);
    if page == 1 && xv.is_empty() {
        page = 0;
    }

    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("Ant")
        .id(egui::Id::new("ant_window"))
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
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - ANT", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            egui::Area::new(egui::Id::new("ant_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });

            // Header: the "!", the HF / XVTR pages and, on ANAN-100/200, the PA board checkbox.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                help_button(&mut c, "ant_help", ANT_HELP);
                if c.add(chip_button("HF", page == 0).min_size(egui::vec2(90.0, 40.0))).clicked() {
                    page = 0;
                }
                if !xv.is_empty() && c.add(chip_button("XVTR", page == 1).min_size(egui::vec2(110.0, 40.0))).clicked() {
                    page = 1;
                }
                if matches!(connected.device.board, Boards::Hermes | Boards::Angelia | Boards::Orion) {
                    let mut new_pa_board = connected.session.new_pa_board.load(Ordering::Relaxed);
                    if touch_checkbox_sized(&mut c, &mut new_pa_board, "ANAN 100/200 new PA board", 30.0).changed() {
                        connected.session.new_pa_board.store(new_pa_board, Ordering::Relaxed);
                        changed = true;
                    }
                    help_button(&mut c, "ant_new_pa", NEW_PA_HELP);
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(page_id, page));
            ui.add_space(4.0);

            // Column titles, twice (two band groups side by side).
            let font = egui::TextStyle::Body.resolve(ui.style());
            let strong = egui::FontId::proportional(font.size);
            let color = ui.visuals().text_color();
            let r = new_row(ui, w, 26.0);
            for g in 0..2 {
                let x0 = g as f32 * (GROUP_W + GROUP_GAP);
                for (dx, text) in [(4.0, "Band"), (LABEL_W + 4.0, "RX Ant"), (LABEL_W + RX_W + 12.0 + 4.0, "TX Ant")] {
                    ui.painter().text(egui::pos2(r.left() + x0 + dx, r.center().y), egui::Align2::LEFT_CENTER, text, strong.clone(), color);
                }
            }

            let names = if page == 1 { &xv } else { &hf };
            let mut i = 0;
            while i < names.len() {
                let row = new_row(ui, w, ROW_H);
                for g in 0..2 {
                    let Some(name) = names.get(i + g) else { continue };
                    let x0 = g as f32 * (GROUP_W + GROUP_GAP);
                    ui.painter().text(egui::pos2(row.left() + x0 + 4.0, row.center().y), egui::Align2::LEFT_CENTER, name, strong.clone(), color);
                    let mut ant: AntennaMask = connected.antenna_settings.get(name.as_str()).copied().unwrap_or_default();
                    let mut edited = false;
                    {
                        let mut c = child(ui, row, x0 + LABEL_W, x0 + LABEL_W + RX_W + 6.0, false, 0.0);
                        let items: Vec<(&str, bool)> = RX_PORTS.iter().map(|(_, l)| (*l, true)).collect();
                        let sel = RX_PORTS.iter().position(|(p, _)| *p == ant.rx).unwrap_or(0);
                        if let Some(k) = choice_combo_h(&mut c, &format!("ant_rx_{name}"), RX_W - 34.0, sel, &items, COMBO_H) {
                            if RX_PORTS[k].0 != ant.rx {
                                ant.rx = RX_PORTS[k].0;
                                edited = true;
                            }
                        }
                    }
                    {
                        let tx0 = x0 + LABEL_W + RX_W + 12.0;
                        let mut c = child(ui, row, tx0, tx0 + TX_W + 6.0, false, 0.0);
                        let items: Vec<(&str, bool)> = TX_PORTS.iter().map(|(_, l)| (*l, true)).collect();
                        let sel = TX_PORTS.iter().position(|(p, _)| *p == ant.tx).unwrap_or(0);
                        if let Some(k) = choice_combo_h(&mut c, &format!("ant_tx_{name}"), TX_W - 34.0, sel, &items, COMBO_H) {
                            if TX_PORTS[k].0 != ant.tx {
                                ant.tx = TX_PORTS[k].0;
                                edited = true;
                            }
                        }
                    }
                    if edited {
                        connected.antenna_settings.insert(name.clone(), ant);
                        changed = true;
                    }
                }
                i += 2;
            }
        });

    (close_now, changed)
}
