# Kiosk toolbar and its MIDI keys

The 1024x600 kiosk layout has a toolbar of eight boxes along the bottom of the screen, modelled on
piHPSDR's (`toolbar.c`, `toolbar_menu.c`, `action_dialog.c`, `midi_menu.c`, `actions.c`). It exists
only in kiosk mode; the desktop layout keeps its zoom/pan row and status bar. The code is in
`src/toolbar.rs` (data) and `src/main.rs` (`render_toolbar`, `render_toolbar_config`,
`run_toolbar_fn`, `dispatch_midi_binding`).

## The scheme: two steps, two menus, two lists

Physical keys under the screen (sent as MIDI notes) never know what a box does. That is split in two:

| Step | Where | List | Result |
|------|-------|------|--------|
| 1. Which key is which box | Settings -> MIDI -> Learn, then **Action** | the MIDI action list (alphabetical) with `ToolBar1`..`ToolBar7`, `Function`, `FuncRev` | the key is bound to **`ToolBar1`**, not to a function |
| 2. What each box does | Settings -> Toolbar (opens straight on the configuration), click a box -> **Choose Function** | the toolbar function list (alphabetical), without the `ToolBar`/`Function` entries | box 1 of layer 0 is, say, `12m` |

Pressing the key runs whatever box *n* holds **in the current layer**; `Function` steps to the next
layer (`FuncRev` to the previous one). So changing a box's function in step 2 never needs the MIDI
to be learned again.

The toolbar list is the MIDI key-action list plus Two Tone and the zoom/pan controls (which used to
be a slider row), minus the `ToolBar`/`Function`/`FuncRev` entries (a box that presses a box would
loop). More functions can be added in `src/toolbar.rs` (`ToolbarFn`); the box, the configuration and
the chooser pick them up automatically.

## On screen

* Seven function boxes (F1-F7) and `FNC(n)`. Grey box with a thin outline; lighter while pressed;
  lighter with whiter text while the function is on (MOX, TUNE, 2TONE, NR, CTUN, SPLIT, RIT...).
* Click runs the function. Holding a function box ~0.6 s opens the function chooser for that box.
  Clicking `FNC` goes to the next layer; holding it goes back.
* Six layers, as in piHPSDR. The assignment and the current layer are saved in the radio's config
  (`toolbar_layers`, `toolbar_layer`).
* The audio scope above the spectrum is anchored to the full window width, so it does not move when a
  side panel (CW decoder, RADE) opens.

## Pitfall: a key bound twice

The first MIDI binding that matches a note wins. If a key still has an old direct binding (for example
note 29 -> `Mox`) next to the new `ToolBar1` one, the old one fires and the toolbar never gets asked.
That looked like "changing the function in Toolbar configuration does nothing". Adding a binding in
Settings -> MIDI now replaces any older binding on the same control, but bindings made earlier have to
be deleted (Settings -> MIDI, bindings table) or learned again.
