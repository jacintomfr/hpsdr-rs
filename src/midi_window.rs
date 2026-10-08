//! The "MIDI" window (deskHPSDR midi_menu.c layout): enable + devices on top, the learn row (the control just touched: event, channel, number, type,
//! action, channel any / momentary, Add), the wheel parameters (sensitivity, rate limit, acceleration -- deskHPSDR's "Configure WHEEL parameters") and the list
//! of bindings with Edit / Delete, plus Delete All, Import and Backup / Restore. Same layout family as the OC Output window: it stops above the toolbar,
//! square corners, no shadow, the explanation behind the round "!".
//!
//! Only the presentation is new: everything it edits is the state the old Settings -> MIDI page used (`connected.midi`, `midi_learn`, `midi_bindings`),
//! with the same rules (a control does one thing: Add drops older bindings on the same control; Update replaces in place; Learn captures the next
//! event in the frame loop). Not ported from deskHPSDR: "Ignore Controller Pairs".

use crate::discovery_ui::touch_scroll;
use crate::midi::{self, MidiAction, MidiBinding, MidiBindingKind, MidiEventKind, MidiStatus, RawMidiEvent, WheelAccelMode, KEY_ACTIONS, KNOB_ACTIONS, WHEEL_ACTIONS};
use crate::tx_window::{child, new_row, spin};
use crate::{chip_button, help_button, kiosk_accent_button, midi_action_label, touch_checkbox_sized, ConnectedState, MidiLearnState};
use std::sync::atomic::Ordering;

const ROW_H: f32 = 38.0;
const MIDI_HELP: &str = "MIDI control surface: bind the buttons, knobs and wheels of a MIDI controller to actions.\n\nEnable MIDI, tick your controller(s), press Learn, move a control and pick what it does (Action), then Add. A button is a Key; a knob or wheel sends a value (Knob = absolute position, Wheel = relative steps: pick whichever your controller sends). Wheels have a sensitivity, a rate limit and an acceleration mode. Add replaces an older binding on the same control; Edit loads a binding back into the learn row.";
const WHEEL_HELP: &str = "Sensitivity scales how far one movement moves the value (lower it if a light touch moves too far, raise it if it feels sluggish). Rate limit is the minimum time between two applied steps (raise it if the control is still jumpy; 0 = no limit). Acceleration: Fixed = every message moves by the same amount; Value-based = a bigger or faster turn moves further (like piHPSDR / deskHPSDR).";

fn event_desc(ev: &RawMidiEvent) -> String {
    match ev.kind {
        MidiEventKind::NoteKey => format!("Note {} / Ch {}", ev.number, ev.channel + 1),
        MidiEventKind::ControlChange => format!("CC {} / Ch {}", ev.number, ev.channel + 1),
        MidiEventKind::PitchBend => format!("Pitch Bend / Ch {}", ev.channel + 1),
    }
}

fn backup(connected: &mut ConnectedState) {
    if let Some(path) = rfd::FileDialog::new().add_filter("MIDI bindings", &["json"]).set_file_name("hpsdr-rs-midi-bindings.json").save_file() {
        connected.midi_backup_message = Some(match serde_json::to_string_pretty(&connected.midi_bindings) {
            Ok(json) => match std::fs::write(&path, json) {
                Ok(()) => format!("Saved {} binding(s) to {}", connected.midi_bindings.len(), path.display()),
                Err(e) => format!("Couldn't write {}: {e}", path.display()),
            },
            Err(e) => format!("Couldn't encode bindings: {e}"),
        });
    }
}

fn restore(connected: &mut ConnectedState) -> bool {
    let Some(path) = rfd::FileDialog::new().add_filter("MIDI bindings", &["json"]).pick_file() else { return false };
    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<Vec<MidiBinding>>(&json) {
            Ok(bindings) => {
                connected.midi_backup_message = Some(format!("Restored {} binding(s) from {}", bindings.len(), path.display()));
                connected.midi_bindings = bindings;
                return true;
            }
            Err(e) => connected.midi_backup_message = Some(format!("Couldn't parse {}: {e}", path.display())),
        },
        Err(e) => connected.midi_backup_message = Some(format!("Couldn't read {}: {e}", path.display())),
    }
    false
}

fn import_thetis(connected: &mut ConnectedState) -> bool {
    let Some(path) = rfd::FileDialog::new().add_filter("XML", &["xml"]).pick_file() else { return false };
    match std::fs::read_to_string(&path) {
        Ok(xml) => match crate::midi_import::import_thetis_midi2cat(&xml) {
            Ok(result) => {
                // Replace a binding on the exact same raw control instead of adding a duplicate that would never fire.
                for imported in result.imported() {
                    if let Some(existing) =
                        connected.midi_bindings.iter_mut().find(|b| b.event == imported.event && b.channel == imported.channel && b.number == imported.number)
                    {
                        *existing = *imported;
                    } else {
                        connected.midi_bindings.push(*imported);
                    }
                }
                let skipped: Vec<String> =
                    result.outcomes.iter().filter_map(|o| o.result.as_ref().err().map(|reason| format!("{}: {reason}", o.control_name))).collect();
                let mut msg = format!("Imported {} binding(s), skipped {}", result.imported_count(), result.skipped_count());
                if !skipped.is_empty() {
                    msg.push_str(&format!(" ({})", skipped.join("; ")));
                }
                connected.midi_import_message = Some(msg);
                return true;
            }
            Err(e) => connected.midi_import_message = Some(format!("Import failed: {e}")),
        },
        Err(e) => connected.midi_import_message = Some(format!("Couldn't read {}: {e}", path.display())),
    }
    false
}

fn kind_label(k: MidiBindingKind) -> &'static str {
    match k {
        MidiBindingKind::Key => "Key",
        MidiBindingKind::Knob => "Knob",
        MidiBindingKind::Wheel => "Wheel",
    }
}

/// Returns (close, changed).
pub fn midi_window(ui: &mut egui::Ui, connected: &mut ConnectedState) -> (bool, bool) {
    let screen = ui.ctx().content_rect();
    let mut close_now = false;
    let mut changed = false;
    let pick_id = egui::Id::new("midi_window_action_pick");

    // The action chooser (deskHPSDR: the Action button opens the Choose Function list).
    if let Some((mut sel, mut scroll)) = ui.ctx().data(|d| d.get_temp::<(usize, f32)>(pick_id)) {
        let binding_kind = match connected.midi_learn.captured {
            Some(ev) if ev.kind == MidiEventKind::NoteKey => MidiBindingKind::Key,
            _ => connected.midi_learn.captured_kind.unwrap_or(MidiBindingKind::Knob),
        };
        let actions: &[MidiAction] = match binding_kind {
            MidiBindingKind::Key => KEY_ACTIONS,
            MidiBindingKind::Knob => KNOB_ACTIONS,
            MidiBindingKind::Wheel => WHEEL_ACTIONS,
        };
        let mut actions: Vec<MidiAction> = actions.to_vec();
        actions.sort_by(|a, b| crate::toolbar::natural_cmp(midi_action_label(*a, connected), midi_action_label(*b, connected)));
        let labels: Vec<String> = actions.iter().map(|a| midi_action_label(*a, connected).replace('\n', " ")).collect();
        match crate::choose_function_dialog(ui.ctx(), &labels, &mut sel, &mut scroll) {
            Some(true) => {
                connected.midi_learn.selected_action = actions.get(sel).copied();
                ui.ctx().data_mut(|d| d.remove_temp::<(usize, f32)>(pick_id));
            }
            Some(false) => {
                ui.ctx().data_mut(|d| d.remove_temp::<(usize, f32)>(pick_id));
            }
            None => {
                ui.ctx().data_mut(|d| d.insert_temp(pick_id, (sel, scroll)));
            }
        }
    }

    let win_h = screen.height() - (crate::TOOLBAR_HEIGHT + crate::TOOLBAR_MARGIN) - 11.0;
    let frame = egui::Frame::window(ui.style()).inner_margin(egui::Margin::symmetric(24, 4)).corner_radius(0.0).shadow(egui::Shadow::NONE);
    egui::Window::new("MIDI")
        .id(egui::Id::new("midi_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .fixed_pos(screen.min)
        .constrain_to(screen)
        .fixed_size(egui::vec2(screen.width() - 58.0, win_h))
        .show(ui.ctx(), |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.ctx().data(|d| d.get_temp::<(usize, f32)>(pick_id)).is_none() {
                close_now = true;
            }
            let w = screen.width() - 58.0;
            ui.set_width(w);
            ui.set_min_height(win_h);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
            let (tr, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), egui::Sense::hover());
            ui.painter().text(tr.center(), egui::Align2::CENTER_CENTER, "hpsdr-rs - MIDI", egui::FontId::proportional(16.0), egui::Color32::from_gray(225));
            egui::Area::new(egui::Id::new("midi_window_close"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 26.0))
                .show(ui.ctx(), |ui| {
                    if kiosk_accent_button(ui, "CLOSE").clicked() {
                        close_now = true;
                    }
                });
            let strong = egui::FontId::proportional(egui::TextStyle::Body.resolve(ui.style()).size);
            let color = ui.visuals().text_color();

            // ---- Header: "!", Enable, status.
            let row = new_row(ui, w, 46.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 12.0;
                help_button(&mut c, "midi_help", MIDI_HELP);
                let mut on = connected.midi.enabled.load(Ordering::Relaxed);
                if touch_checkbox_sized(&mut c, &mut on, "Enable MIDI control", 30.0).changed() {
                    connected.midi.enabled.store(on, Ordering::Relaxed);
                    changed = true;
                }
                let status = match &*connected.midi.status.lock().unwrap() {
                    MidiStatus::Disabled => "Disabled".to_string(),
                    MidiStatus::Searching => "Searching...".to_string(),
                    MidiStatus::Connected { connected: names, missing } => {
                        let mut s = format!("Connected: {}", names.join(", "));
                        if !missing.is_empty() {
                            s.push_str(&format!(" -- searching: {}", missing.join(", ")));
                        }
                        s
                    }
                    MidiStatus::Error(e) => format!("Error: {e}"),
                };
                c.label(egui::RichText::new(status).color(egui::Color32::from_gray(170)));
            }

            // ---- Devices: one chip per detected port; a configured one not seen right now stays, marked.
            let row = new_row(ui, w, 48.0);
            {
                let mut c = child(ui, row, 0.0, 866.0, false, 0.0);
                c.spacing_mut().item_spacing.x = 10.0;
                c.label("Devices:");
                let ports = midi::list_port_names();
                let mut wanted = connected.midi.device_names.lock().unwrap().clone();
                let keys: Vec<String> = ports.iter().map(|p| midi::port_key(p)).collect();
                if ports.is_empty() {
                    c.label(egui::RichText::new("none detected").color(egui::Color32::from_gray(150)));
                }
                for (name, key) in ports.iter().zip(keys.iter()) {
                    let on = wanted.contains(key);
                    let short = key.split(':').next_back().unwrap_or(key).to_string();
                    if c.add(chip_button(&short, on).min_size(egui::vec2(0.0, 40.0))).on_hover_text(name).clicked() {
                        if on {
                            wanted.retain(|n| n != key);
                        } else {
                            wanted.push(key.clone());
                        }
                        *connected.midi.device_names.lock().unwrap() = wanted.clone();
                        changed = true;
                    }
                }
                for name in wanted.iter().filter(|n| !keys.contains(n)).cloned().collect::<Vec<_>>() {
                    let short = name.split(':').next_back().unwrap_or(&name).to_string();
                    if c.add(chip_button(&format!("{short} (not detected)"), true).min_size(egui::vec2(0.0, 40.0))).clicked() {
                        connected.midi.device_names.lock().unwrap().retain(|n| n != &name);
                        changed = true;
                    }
                }
            }

            // ---- Learn row: the control just touched.
            let row = new_row(ui, w, 54.0);
            {
                let mut c = child(ui, row, 0.0, 120.0, false, 0.0);
                if c.add(chip_button("Learn", connected.midi_learn.listening).min_size(egui::vec2(110.0, 40.0))).clicked() {
                    connected.midi_learn.listening = !connected.midi_learn.listening;
                    if connected.midi_learn.listening {
                        connected.midi_learn.captured = None;
                    }
                }
                let text = if connected.midi_learn.listening {
                    "Move a control on your MIDI device...".to_string()
                } else if let Some(ev) = connected.midi_learn.captured {
                    event_desc(&ev)
                } else {
                    "Press Learn, then move a control".to_string()
                };
                ui.painter().text(egui::pos2(row.left() + 130.0, row.center().y), egui::Align2::LEFT_CENTER, text, strong.clone(), color);
            }
            if let Some(ev) = connected.midi_learn.captured {
                let binding_kind = if ev.kind == MidiEventKind::NoteKey { MidiBindingKind::Key } else { connected.midi_learn.captured_kind.unwrap_or(MidiBindingKind::Knob) };
                {
                    // Type (Knob / Wheel for a value control; a note is always a Key) and the Action button.
                    let mut c = child(ui, row, 420.0, 866.0, false, 0.0);
                    c.spacing_mut().item_spacing.x = 8.0;
                    if ev.kind == MidiEventKind::NoteKey {
                        c.label("Key");
                    } else {
                        for (k, label) in [(MidiBindingKind::Knob, "Knob"), (MidiBindingKind::Wheel, "Wheel")] {
                            if c.add(chip_button(label, binding_kind == k).min_size(egui::vec2(80.0, 40.0))).clicked() {
                                connected.midi_learn.captured_kind = Some(k);
                                connected.midi_learn.selected_action = None;
                            }
                        }
                    }
                    let current = connected.midi_learn.selected_action.map(|a| midi_action_label(a, connected).replace('\n', " ")).unwrap_or_else(|| "(choose action)".to_string());
                    if c.add(egui::Button::new(current).min_size(egui::vec2(230.0, 40.0)).corner_radius(5.0)).clicked() {
                        let actions: &[MidiAction] = match binding_kind {
                            MidiBindingKind::Key => KEY_ACTIONS,
                            MidiBindingKind::Knob => KNOB_ACTIONS,
                            MidiBindingKind::Wheel => WHEEL_ACTIONS,
                        };
                        let mut actions: Vec<MidiAction> = actions.to_vec();
                        actions.sort_by(|a, b| crate::toolbar::natural_cmp(midi_action_label(*a, connected), midi_action_label(*b, connected)));
                        let sel = connected.midi_learn.selected_action.and_then(|a| actions.iter().position(|x| *x == a)).unwrap_or(0);
                        c.ctx().data_mut(|d| d.insert_temp(pick_id, (sel, 0.0f32)));
                    }
                }

                // ---- Options row: any channel, momentary, Add / Update, Cancel.
                let row = new_row(ui, w, 48.0);
                {
                    let mut c = child(ui, row, 0.0, 700.0, false, 0.0);
                    c.spacing_mut().item_spacing.x = 18.0;
                    touch_checkbox_sized(&mut c, &mut connected.midi_learn.channel_any, "Any channel", 30.0);
                    if binding_kind == MidiBindingKind::Key && matches!(connected.midi_learn.selected_action, Some(MidiAction::Mox) | Some(MidiAction::Tune)) {
                        touch_checkbox_sized(&mut c, &mut connected.midi_learn.momentary, "Momentary (press AND release)", 30.0);
                    }
                }
                {
                    let mut c = child(ui, row, 560.0, w - 130.0, true, 0.0);
                    c.spacing_mut().item_spacing.x = 10.0;
                    if c.add(egui::Button::new("Cancel").min_size(egui::vec2(110.0, 40.0)).corner_radius(5.0)).clicked() {
                        connected.midi_learn = MidiLearnState::default();
                    }
                    let add_label = if connected.midi_learn.edit_index.is_some() { "Update" } else { "Add" };
                    if c
                        .add_enabled(connected.midi_learn.selected_action.is_some(), egui::Button::new(add_label).min_size(egui::vec2(110.0, 40.0)).corner_radius(5.0))
                        .clicked()
                    {
                        if let Some(action) = connected.midi_learn.selected_action {
                            let binding = MidiBinding {
                                event: ev.kind,
                                channel: if connected.midi_learn.channel_any { None } else { Some(ev.channel) },
                                number: ev.number,
                                kind: binding_kind,
                                action,
                                momentary: connected.midi_learn.momentary,
                                sensitivity: connected.midi_learn.sensitivity,
                                debounce_ms: connected.midi_learn.debounce_ms,
                                accel_mode: connected.midi_learn.accel_mode,
                            };
                            if let Some(i) = connected.midi_learn.edit_index {
                                connected.midi_bindings[i] = binding;
                            } else {
                                // A control can only do one thing, and the first matching binding wins: drop older ones on the same control.
                                connected.midi_bindings.retain(|b| {
                                    !(b.event == binding.event && b.number == binding.number && (b.channel.is_none() || binding.channel.is_none() || b.channel == binding.channel))
                                });
                                connected.midi_bindings.push(binding);
                            }
                            changed = true;
                            connected.midi_learn = MidiLearnState::default();
                        }
                    }
                }

                // ---- Wheel parameters (deskHPSDR "Configure WHEEL parameters").
                if binding_kind == MidiBindingKind::Wheel && connected.midi_learn.captured.is_some() {
                    let row = new_row(ui, w, 52.0);
                    {
                        let mut c = child(ui, row, 0.0, 40.0, false, 0.0);
                        help_button(&mut c, "midi_wheel_help", WHEEL_HELP);
                    }
                    ui.painter().text(egui::pos2(row.left() + 46.0, row.center().y), egui::Align2::LEFT_CENTER, "Sensitivity", strong.clone(), color);
                    let mut sens = connected.midi_learn.sensitivity as f64;
                    if spin(ui, row, 150.0, 150.0 + 98.0 + 90.0, "midi_sens", &mut sens, 0.05, 10.0, 0.05, 2) {
                        connected.midi_learn.sensitivity = sens as f32;
                    }
                    ui.painter().text(egui::pos2(row.left() + 410.0, row.center().y), egui::Align2::LEFT_CENTER, "Rate limit (ms)", strong.clone(), color);
                    let mut rate = connected.midi_learn.debounce_ms as f64;
                    if spin(ui, row, 560.0, 560.0 + 98.0 + 80.0, "midi_rate", &mut rate, 0.0, 500.0, 5.0, 0) {
                        connected.midi_learn.debounce_ms = rate as u32;
                    }
                    let mut c = child(ui, row, 760.0, w, false, 0.0);
                    c.spacing_mut().item_spacing.x = 8.0;
                    for (m, label) in [(WheelAccelMode::Fixed, "Fixed"), (WheelAccelMode::ValueBased, "Value")] {
                        if c.add(chip_button(label, connected.midi_learn.accel_mode == m).min_size(egui::vec2(88.0, 40.0))).clicked() {
                            connected.midi_learn.accel_mode = m;
                        }
                    }
                }
            }

            // ---- Bindings: the bar, the table header and the scrolling list.
            let row = new_row(ui, w, 46.0);
            {
                let mut c = child(ui, row, 0.0, w, false, 0.0);
                c.spacing_mut().item_spacing.x = 10.0;
                c.label(egui::RichText::new(format!("Bindings ({})", connected.midi_bindings.len())).strong());
                if c.add(egui::Button::new("Delete All").min_size(egui::vec2(110.0, 38.0)).corner_radius(5.0)).clicked() {
                    connected.midi_bindings.clear();
                    changed = true;
                }
                if c.add(egui::Button::new("Import Thetis XML").min_size(egui::vec2(160.0, 38.0)).corner_radius(5.0)).clicked() && import_thetis(connected) {
                    changed = true;
                }
                if c.add(egui::Button::new("Backup").min_size(egui::vec2(90.0, 38.0)).corner_radius(5.0)).clicked() {
                    backup(connected);
                }
                if c.add(egui::Button::new("Restore").min_size(egui::vec2(90.0, 38.0)).corner_radius(5.0)).clicked() && restore(connected) {
                    changed = true;
                }
                if let Some(msg) = connected.midi_import_message.clone().or_else(|| connected.midi_backup_message.clone()) {
                    let short: String = msg.chars().take(60).collect();
                    c.label(egui::RichText::new(short).color(egui::Color32::from_gray(150)));
                }
            }
            const X: [f32; 9] = [4.0, 84.0, 154.0, 234.0, 304.0, 544.0, 614.0, 684.0, 780.0];
            let r = new_row(ui, w, 24.0);
            for (x, t) in X.iter().zip(["Event", "Channel", "Number", "Type", "Action", "Mom.", "Sens.", "Rate", ""]) {
                ui.painter().text(egui::pos2(r.left() + x, r.center().y), egui::Align2::LEFT_CENTER, t, strong.clone(), egui::Color32::from_gray(170));
            }
            let mut delete_index = None;
            let mut edit_request = None;
            let bindings = connected.midi_bindings.clone();
            let labels: Vec<&'static str> = bindings.iter().map(|b| midi_action_label(b.action, connected)).collect();
            let h = ui.available_height().max(120.0);
            touch_scroll(ui, "midi_bindings", Some(h), false, &mut |ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                let inner_w = ui.available_width();
                if bindings.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No bindings yet -- use Learn to add one.").color(egui::Color32::from_gray(150)));
                }
                for (i, b) in bindings.iter().enumerate() {
                    let row = new_row(ui, inner_w, ROW_H);
                    let t = |x: f32, text: String| {
                        ui.painter().text(egui::pos2(row.left() + x, row.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(15.0), color);
                    };
                    t(X[0], match b.event {
                        MidiEventKind::NoteKey => "Note",
                        MidiEventKind::ControlChange => "CC",
                        MidiEventKind::PitchBend => "Pitch Bend",
                    }
                    .to_string());
                    t(X[1], b.channel.map(|c| (c + 1).to_string()).unwrap_or_else(|| "Any".to_string()));
                    t(X[2], b.number.to_string());
                    t(X[3], kind_label(b.kind).to_string());
                    let mut action = labels[i].replace('\n', " ");
                    if action.chars().count() > 26 {
                        action = format!("{}...", action.chars().take(25).collect::<String>());
                    }
                    t(X[4], action);
                    t(X[5], if b.momentary { "Yes".to_string() } else { String::new() });
                    t(X[6], if b.kind == MidiBindingKind::Wheel { format!("{:.2}", b.sensitivity) } else { String::new() });
                    t(X[7], if b.kind == MidiBindingKind::Wheel { format!("{} ms", b.debounce_ms) } else { String::new() });
                    let mut cell = child(ui, row, X[8], inner_w, false, 0.0);
                    cell.spacing_mut().item_spacing.x = 6.0;
                    if cell.add(egui::Button::new("Edit").min_size(egui::vec2(60.0, 32.0)).corner_radius(5.0)).clicked() {
                        edit_request = Some((i, *b));
                    }
                    if cell.add(egui::Button::new("Delete").min_size(egui::vec2(70.0, 32.0)).corner_radius(5.0)).clicked() {
                        delete_index = Some(i);
                    }
                }
            });
            if let Some((i, binding)) = edit_request {
                connected.midi_learn = MidiLearnState {
                    listening: false,
                    captured: Some(RawMidiEvent { kind: binding.event, channel: binding.channel.unwrap_or(0), number: binding.number, value: 0, off: false }),
                    captured_kind: Some(binding.kind),
                    channel_any: binding.channel.is_none(),
                    selected_action: Some(binding.action),
                    momentary: binding.momentary,
                    sensitivity: binding.sensitivity,
                    debounce_ms: binding.debounce_ms,
                    accel_mode: binding.accel_mode,
                    edit_index: Some(i),
                };
            }
            if let Some(i) = delete_index {
                connected.midi_bindings.remove(i);
                changed = true;
            }
        });

    (close_now, changed)
}
