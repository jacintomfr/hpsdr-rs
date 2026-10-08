//! Full-screen Toolbar window (Menu -> Toolbar): the editor of the 8 layers x 8 boxes of the bottom toolbar (FNC(1)..FNC(8)).
//! It was the Settings -> Toolbar tab; in the kiosk the tab is gone and this window is the only way in. The grid itself
//! (`render_toolbar_config`) and the function chooser are the same code as before.

use crate::{kiosk_accent_button, render_toolbar_config, ConnectedState};

/// Returns (close, changed). The function chooser saves the assignments itself, so `changed` is always false.
pub fn toolbar_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    // Like the OC Output / PA windows: it stops above the bottom toolbar, which stays visible and usable (no shadow over it).
    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("Toolbar")
        .id(egui::Id::new("toolbar_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .fixed_pos(screen.min)
        .constrain_to(screen)
        .fixed_size(egui::vec2(screen.width() - 58.0, win_h))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && connected.toolbar_choose.is_none() {
                close_now = true;
            }
            let w = screen.width() - 58.0;
            ui.set_width(w);
            ui.set_min_height(win_h);
            crate::rounded_boxes(ui);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - Toolbar", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            egui::Area::new(egui::Id::new("toolbar_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            ui.add_space(8.0);
            // Left of the CLOSE button: 96 px + 24 px margin are kept free on the right of the help text.
            ui.allocate_ui(egui::vec2(w - 130.0, 40.0), |ui| {
                ui.label(
                    "Each row is one layer, FNC(1) to FNC(8); the eight boxes at the bottom of the screen run the functions of the current layer. Tap a box to change its function.\nAssign FNC (next layer) or FNC- to a box to step through the layers, or FNC's to open the list and jump to any layer.",
                );
            });
            ui.add_space(6.0);
            render_toolbar_config(ui, connected);
        });
    (close_now, false)
}
