# MIDI window (src/midi_window.rs)

The Settings -> MIDI page in the new layout (NEW MENU -> MIDI, or Settings -> MIDI in the kiosk), laid out like deskHPSDR's midi_menu.c and like the OC Output window: it stops above the toolbar, square corners, no shadow, the explanation behind the round "!".

* Header: Enable MIDI control and the connection status. Devices: one touch chip per detected controller (a configured one not seen right now stays, marked "not detected").
* Learn row (deskHPSDR's "new event" row): Learn, the control just touched (Note / CC / Pitch Bend, number, channel), Type (Key for a note; Knob / Wheel for a value control), Action (opens the Choose Function list), Any channel, Momentary (Mox / Tune only), Add / Update, Cancel.
* Wheel parameters (deskHPSDR "Configure WHEEL parameters"): sensitivity, rate limit, acceleration (Fixed / Value-based), shown for a Wheel binding, explained behind a "!".
* Bindings: Delete All, Import Thetis XML, Backup, Restore, and the list (Event, Channel, Number, Type, Action, Mom., Sens., Rate) with Edit and Delete per row, in a touch scroll area.

Only the presentation is new: it edits the same `midi`, `midi_learn` and `midi_bindings` state as the old page, with the same rules (a control does one thing; Add drops older bindings on the same control; Update replaces in place). Not ported: "Ignore Controller Pairs".
