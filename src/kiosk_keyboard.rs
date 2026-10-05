/*!
On-screen keyboard for the touch kiosk (1024x600 panel, `lcd_kiosk_mode()`).

It appears by itself, at the bottom of the window being drawn, whenever a text field has the keyboard focus, and
moves to the top of the window if the field would otherwise be under it. The keys are not widgets of the field:
they are painted by `begin()` at the start of every pass of a window and turned into ordinary egui events (`Text`,
`Key` for Backspace/Enter) that are pushed into the input of that same pass, so every `TextEdit` works unchanged.
Pressing a key must not take the focus away from the field: while the pointer is on the keyboard the "surrender
focus on press" option is switched off, and the keys use a non-focusable sense.

Call `install()` once at start and `begin()` at the top of the UI pass of each window that can have text fields
(the main window and the Discovery, Settings and Digital windows).
*/

use std::sync::Arc;

const KEY_H: f32 = 42.0;
const GAP: f32 = 6.0;
const PAD: f32 = 6.0;
const ROWS: usize = 5;

#[derive(Clone, Default)]
struct KbState {
    shift: bool,
    symbols: bool,
    /// Where the keyboard was drawn last pass (to recognise presses on it).
    kb_rect: Option<egui::Rect>,
    /// Where the focused text field was last pass (to keep the keyboard off it).
    field_rect: Option<egui::Rect>,
}

fn state_id(ctx: &egui::Context) -> egui::Id {
    egui::Id::new(("kiosk_keyboard", ctx.viewport_id()))
}

/// Remembers, at the end of every pass, where the focused text field is (egui reports it for the IME).
pub fn install(ctx: &egui::Context) {
    if !crate::lcd_kiosk_mode() {
        return;
    }
    ctx.on_end_pass(
        "kiosk_keyboard_field",
        Arc::new(|ui: &mut egui::Ui| {
            let ctx = ui.ctx().clone();
            let rect = ctx.output(|o| o.ime.map(|i| i.rect));
            let id = state_id(&ctx);
            let mut st: KbState = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
            st.field_rect = rect;
            ctx.data_mut(|d| d.insert_temp(id, st));
        }),
    );
}

enum Press {
    Char(char),
    Backspace,
    Enter,
    Shift,
    Symbols,
    Hide,
}

/// Draws the keyboard (if a text field is focused) and pushes what was typed into this pass' input.
pub fn begin(ctx: &egui::Context) {
    if !crate::lcd_kiosk_mode() {
        return;
    }
    let id = state_id(ctx);
    let mut st: KbState = ctx.data(|d| d.get_temp(id)).unwrap_or_default();

    // A press on the keyboard must not make the text field give up the focus.
    let on_keyboard = match (st.kb_rect, ctx.input(|i| i.pointer.interact_pos())) {
        (Some(r), Some(p)) => r.contains(p),
        _ => false,
    };
    ctx.options_mut(|o| {
        o.input_options.surrender_focus_on =
            if on_keyboard { egui::SurrenderFocusOn::Never } else { egui::SurrenderFocusOn::Presses };
    });

    if !ctx.text_edit_focused() {
        st.kb_rect = None;
        ctx.data_mut(|d| d.insert_temp(id, st));
        return;
    }

    let screen = ctx.content_rect();
    let kb_h = ROWS as f32 * KEY_H + (ROWS as f32 - 1.0) * GAP + 2.0 * PAD;
    let at_top = st.field_rect.is_some_and(|f| f.bottom() > screen.bottom() - kb_h - 8.0);
    let top = if at_top { screen.top() } else { screen.bottom() - kb_h };
    let kb = egui::Rect::from_min_size(egui::pos2(screen.left(), top), egui::vec2(screen.width(), kb_h));

    let mut presses: Vec<Press> = Vec::new();
    egui::Area::new(egui::Id::new("kiosk_keyboard_area"))
        .order(egui::Order::Foreground)
        .fixed_pos(kb.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(kb);
            let (_, _) = ui.allocate_exact_size(kb.size(), egui::Sense::hover());
            ui.painter().rect(kb, 0.0, egui::Color32::from_gray(24), egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);

            // (label, press, weight)
            let ch = |s: &str| -> Vec<(String, Press, f32)> {
                s.chars()
                    .map(|c| {
                        let shown = if st.shift && c.is_alphabetic() { c.to_ascii_uppercase() } else { c };
                        (shown.to_string(), Press::Char(shown), 1.0)
                    })
                    .collect()
            };
            let rows: Vec<Vec<(String, Press, f32)>> = if st.symbols {
                vec![
                    { let mut r = ch("!@#$%^&*()"); r.push(("\u{232b}".into(), Press::Backspace, 1.4)); r },
                    ch("-_=+[]{}\\|"),
                    { let mut r = ch(";:'\"<>?/~"); r.push(("\u{21b5}".into(), Press::Enter, 1.4)); r },
                    ch("`.,@#_-+*/"),
                    vec![
                        ("ABC".into(), Press::Symbols, 1.6),
                        ("space".into(), Press::Char(' '), 5.0),
                        (".".into(), Press::Char('.'), 1.0),
                        ("/".into(), Press::Char('/'), 1.0),
                        (":".into(), Press::Char(':'), 1.0),
                        ("hide".into(), Press::Hide, 1.6),
                    ],
                ]
            } else {
                vec![
                    { let mut r = ch("1234567890"); r.push(("\u{232b}".into(), Press::Backspace, 1.4)); r },
                    ch("qwertyuiop"),
                    { let mut r = ch("asdfghjkl"); r.push(("\u{21b5}".into(), Press::Enter, 1.6)); r },
                    { let mut r = vec![("\u{21e7}".into(), Press::Shift, 1.4)]; r.extend(ch("zxcvbnm,.-")); r },
                    vec![
                        ("?123".into(), Press::Symbols, 1.6),
                        ("space".into(), Press::Char(' '), 5.0),
                        (".".into(), Press::Char('.'), 1.0),
                        ("/".into(), Press::Char('/'), 1.0),
                        (":".into(), Press::Char(':'), 1.0),
                        ("hide".into(), Press::Hide, 1.6),
                    ],
                ]
            };

            let now = ui.input(|i| i.time);
            let mut y = kb.top() + PAD;
            for (ri, row) in rows.into_iter().enumerate() {
                let total_w: f32 = row.iter().map(|k| k.2).sum();
                let avail = kb.width() - 2.0 * PAD - GAP * (row.len() as f32 - 1.0);
                let unit = avail / total_w;
                let mut x = kb.left() + PAD;
                for (ki, (label, press, weight)) in row.into_iter().enumerate() {
                    let w = unit * weight;
                    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, KEY_H));
                    x += w + GAP;
                    // Not focusable: pressing a key must leave the focus on the text field.
                    let resp = ui.interact(rect, egui::Id::new(("kiosk_key", ri, ki)), egui::Sense::CLICK);
                    let down = resp.is_pointer_button_down_on();
                    let active = matches!(press, Press::Shift) && st.shift || matches!(press, Press::Symbols) && st.symbols;
                    let fill = if down {
                        egui::Color32::from_gray(110)
                    } else if active {
                        egui::Color32::from_rgb(200, 110, 30)
                    } else if matches!(press, Press::Char(c) if c != ' ') || matches!(press, Press::Char(' ')) {
                        egui::Color32::from_gray(52)
                    } else {
                        egui::Color32::from_gray(40)
                    };
                    ui.painter().rect(rect, 5.0, fill, egui::Stroke::new(1.0, egui::Color32::from_gray(95)), egui::StrokeKind::Inside);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &label,
                        egui::FontId::proportional(20.0),
                        egui::Color32::from_gray(235),
                    );
                    // Backspace repeats while held (after 0.45 s, every 70 ms); every other key fires on release.
                    let fire = if matches!(press, Press::Backspace) {
                        let key = egui::Id::new("kiosk_key_bs_timer");
                        if down {
                            let (t0, last): (f64, f64) = ui.data(|d| d.get_temp(key)).unwrap_or((-1.0, -1.0));
                            let fire = if t0 < 0.0 {
                                ui.data_mut(|d| d.insert_temp(key, (now, now)));
                                true
                            } else if now - t0 > 0.45 && now - last > 0.07 {
                                ui.data_mut(|d| d.insert_temp(key, (t0, now)));
                                true
                            } else {
                                false
                            };
                            ui.ctx().request_repaint();
                            fire
                        } else {
                            ui.data_mut(|d| d.remove::<(f64, f64)>(key));
                            false
                        }
                    } else {
                        resp.clicked()
                    };
                    if fire {
                        presses.push(press);
                    }
                }
                y += KEY_H + GAP;
            }
        });
    st.kb_rect = Some(kb);

    let mut events: Vec<egui::Event> = Vec::new();
    let key_event = |key: egui::Key| {
        [true, false].map(|pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
    };
    for p in presses {
        match p {
            Press::Char(c) => {
                events.push(egui::Event::Text(c.to_string()));
                st.shift = false;
            }
            Press::Backspace => events.extend(key_event(egui::Key::Backspace)),
            Press::Enter => events.extend(key_event(egui::Key::Enter)),
            Press::Shift => st.shift = !st.shift,
            Press::Symbols => st.symbols = !st.symbols,
            Press::Hide => ctx.memory_mut(|m| m.stop_text_input()),
        }
    }
    if !events.is_empty() {
        ctx.input_mut(|i| i.events.extend(events));
    }
    ctx.data_mut(|d| d.insert_temp(id, st));
}
