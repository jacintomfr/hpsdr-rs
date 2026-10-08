//! Band stacks (deskHPSDR bandstack.h / band.c / vfo.c / bandstack_menu.c): every band keeps a short list of frequency + mode entries and
//! remembers which one is current. Pressing the button of the band you are already on steps to the next entry (vfo_band_changed), and the
//! BandStack window lists the entries of the current band to pick one (bandstack_select_cb: the entry you leave keeps the frequency and mode you
//! had, then the chosen one is applied).
//!
//! Differences from deskHPSDR: the current entry is kept up to date every frame (deskHPSDR copies the VFO into it when it leaves), and an entry has
//! no filter / deviation / CTCSS (this program keeps the filter width per mode, not per entry). The default entries are band.c's (60 m as its "VFO"
//! list; Gen as its general-coverage AM list).

use crate::spectrum::Mode;
use crate::ConnectedState;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BandStackEntry {
    pub frequency_hz: u32,
    pub mode: Mode,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BandStack {
    pub current: usize,
    pub entries: Vec<BandStackEntry>,
}

fn e(frequency_hz: u32, mode: Mode) -> BandStackEntry {
    BandStackEntry { frequency_hz, mode }
}

/// deskHPSDR band.c's default entries for a band by its name here.
pub fn default_stack(name: &str) -> BandStack {
    use Mode::{Am, Cwl, Cwu, Lsb, Usb};
    let entries = match name {
        "160m" => vec![e(1_810_000, Cwl), e(1_835_000, Cwu), e(1_845_000, Usb)],
        "80m" => vec![e(3_501_000, Cwl), e(3_751_000, Lsb), e(3_850_000, Lsb)],
        "60m" => vec![e(5_352_750, Cwu), e(5_354_000, Usb), e(5_357_000, Usb), e(5_360_000, Usb), e(5_363_000, Usb)],
        "40m" => vec![e(7_001_000, Cwl), e(7_152_000, Lsb), e(7_255_000, Lsb)],
        "30m" => vec![e(10_120_000, Cwu), e(10_130_000, Cwu), e(10_140_000, Cwu)],
        "20m" => vec![e(14_010_000, Cwu), e(14_150_000, Usb), e(14_230_000, Usb), e(14_336_000, Usb)],
        "17m" => vec![e(18_068_600, Cwu), e(18_125_000, Usb), e(18_140_000, Usb)],
        "15m" => vec![e(21_001_000, Cwu), e(21_255_000, Usb), e(21_300_000, Usb)],
        "12m" => vec![e(24_895_000, Cwu), e(24_900_000, Usb), e(24_910_000, Usb)],
        "10m" => vec![e(28_010_000, Cwu), e(28_300_000, Usb), e(28_400_000, Usb)],
        "6m" => vec![e(50_010_000, Cwu), e(50_125_000, Usb), e(50_200_000, Usb)],
        // Gen: band.c's bandstack_entriesGEN.
        _ => vec![e(909_000, Am), e(5_975_000, Am), e(13_845_000, Am)],
    };
    BandStack { current: 0, entries }
}

/// The name of the band the dial is on: a ham band, else "Gen".
fn current_name(connected: &ConnectedState) -> String {
    let hz = dial_hz(connected);
    crate::band_for_frequency(hz).map(|b| b.name).unwrap_or("Gen").to_string()
}

fn dial_hz(connected: &ConnectedState) -> u32 {
    if connected.ctun {
        connected.ctun_frequency_hz
    } else {
        connected.session.frequency_hz.load(Ordering::Relaxed)
    }
}

/// Every frame (not on a transverter): the current entry of the current band follows the dial frequency and mode. Returns true when
/// something changed (the configuration must then be saved).
pub(crate) fn remember(connected: &mut ConnectedState) -> bool {
    if connected.active_xvtr.is_some() {
        return false;
    }
    let name = current_name(connected);
    let now = e(dial_hz(connected), connected.spectrum.mode());
    let stack = connected.bandstacks.entry(name.clone()).or_insert_with(|| default_stack(&name));
    let cur = stack.current.min(stack.entries.len().saturating_sub(1));
    if stack.entries.is_empty() {
        stack.entries.push(now);
        return true;
    }
    if stack.entries[cur] == now {
        return false;
    }
    stack.entries[cur] = now;
    true
}

/// The entry a band opens with the first time it is visited (its current one).
pub(crate) fn current_entry(connected: &mut ConnectedState, name: &str) -> BandStackEntry {
    let stack = connected.bandstacks.entry(name.to_string()).or_insert_with(|| default_stack(name));
    stack.entries[stack.current.min(stack.entries.len() - 1)]
}

/// Makes entry `idx` of band `name` current and tunes to it (deskHPSDR vfo_bandstack_changed).
pub(crate) fn select(connected: &mut ConnectedState, name: &str, idx: usize) {
    let Some(stack) = connected.bandstacks.get_mut(name) else { return };
    if idx >= stack.entries.len() {
        return;
    }
    stack.current = idx;
    let entry = stack.entries[idx];
    connected.active_xvtr = None;
    connected.session.set_frequency(entry.frequency_hz);
    connected.ctun_frequency_hz = entry.frequency_hz;
    connected.spectrum.set_mode(entry.mode);
    let width = crate::width_for_mode(&connected.width_memory, entry.mode);
    connected.spectrum.set_width_hz(width);
    if let Some(tx) = &connected.tx_handle {
        tx.set_mode(entry.mode);
        tx.set_width_hz(width);
    }
    crate::remember_band_settings(
        &mut connected.band_memory,
        entry.frequency_hz,
        connected.db_low,
        connected.db_high,
        connected.waterfall_db_low,
        connected.waterfall_db_high,
        entry.mode,
    );
}

/// The band button of the band you are on: step to the next entry (deskHPSDR vfo_band_changed). Returns false when `band_name` is not the
/// current band (the caller then does a normal band change).
pub(crate) fn step_if_same_band(connected: &mut ConnectedState, band_name: &str) -> bool {
    if connected.active_xvtr.is_some() || current_name(connected) != band_name {
        return false;
    }
    let stack = connected.bandstacks.entry(band_name.to_string()).or_insert_with(|| default_stack(band_name));
    if stack.entries.len() < 2 {
        return false;
    }
    let next = (stack.current + 1) % stack.entries.len();
    select(connected, band_name, next);
    true
}

fn label(entry: &BandStackEntry) -> String {
    format!("{:8.3} MHz {}", entry.frequency_hz as f64 * 1e-6, entry.mode.label())
}

/// The BandStack window (deskHPSDR bandstack_menu.c): a compact overlay at the bottom with rounded corners, the entries of the current band as
/// toggle buttons, 4 per row, the current one lit. Returns (close, changed).
pub fn bandstack_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let mut close_now = false;
    let mut changed = false;
    let name = current_name(connected);
    let stack = connected.bandstacks.entry(name.clone()).or_insert_with(|| default_stack(&name)).clone();
    const KEY_W: f32 = 170.0;
    const KEY_H: f32 = 46.0;
    const GAP: f32 = 6.0;
    egui::Window::new("BandStack")
        .id(egui::Id::new("bandstack_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        // The normal window frame (rounded corners and the theme's shadow), above the toolbar like the Band / Mode / Filter windows.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -58.0))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close_now = true;
            }
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal(|ui| {
                if crate::touch_close_button(ui, KEY_H).clicked() {
                    close_now = true;
                }
                ui.label(format!("Band Stack - {name} (VFO-A)"));
            });
            egui::Grid::new("bandstack_grid").num_columns(4).spacing([GAP, GAP]).show(ui, |ui| {
                for (i, entry) in stack.entries.iter().enumerate() {
                    let selected = i == stack.current;
                    if ui.add(crate::chip_button(&label(entry), selected).min_size(egui::vec2(KEY_W, KEY_H))).clicked() && !selected {
                        select(connected, &name, i);
                        changed = true;
                    }
                    if (i + 1) % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    (close_now, changed)
}
