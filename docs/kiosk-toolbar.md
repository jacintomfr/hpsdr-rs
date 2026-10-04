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

## Functions that only the toolbar has

The toolbar list can grow beyond the MIDI list (that is why they are two lists). Today these exist
only in the toolbar: Two Tone, zoom in/out/1x, pan left/right, and **RADE On/Off**. RADE On/Off does
in one press what Digital -> RADE -> Hide does with the mouse (DIGU/DIGL for the band, voice processing
off, RADE's passband, Digital window hidden, so the "RADE v1 mode" side panel shows the status) and the
box stays lit while RADE runs; pressing it again does what closing the Digital window does (mode,
filters, zoom/pan and passband back as before). Code: `toggle_rade_direct` in `src/main.rs`.
To add a function, add a `ToolbarFn` variant in `src/toolbar.rs` (key, labels), run it in
`run_toolbar_fn` and, if it has an on/off state, report it in `toolbar_fn_active`.

## Kiosk layout notes (1024x600 panel)

Everything below applies **only in kiosk mode** (`lcd_kiosk_mode()`); the desktop layout is unchanged.

* **Check the real screen first.** The Pi's MPI7002 panel is natively 1024x600, but a `kanshi` profile can
  put it in 1280x720 (the panel then up-scales and loses contrast and brightness). `~/.config/kanshi/config`
  must say `mode 1024x600@60.044`; `wlr-randr` and `xrandr` show the current mode. Do not size anything
  from a screenshot without knowing the mode.
* **Secondary windows are sized from the real main window**, not from fixed numbers
  (`kiosk_window_geometry`, Settings and Digital): nearly the full width, 60 px shorter than the screen and
  at the top, because the window manager shifts an undecorated window down by about 28 px (measured: asked
  for y=10, got y=38), which used to cut the CLOSE button in half.
* **The UI scale is read once at start** (Settings -> Screen); restart the app after changing it.
* **Meter size:** `KIOSK_METER_SCALE` (now 1.3). The ADC/FIFO warning rows, the unreserved rows under the
  meter and the inert hit area only exist while it is above 1.0 (`kiosk_meter_scaled()`).
* **Hidden in the kiosk main window:** the MOX/TUNE/TWO TONE chips before the spectrum (the toolbar has them).
  REC/PLAY/Record now live on the spectrum, see [kiosk-main-window.md](kiosk-main-window.md).
* **Moved:** rigctl/TCI/CAT are chips on the band row (the standard chip: grey, orange when on); RIT/XIT sit
  beside BIN on the mode row, centred on the TCI TX gain value box and the NB chip (their x is recorded each
  frame in `ConnectedState::align_x`); the top row is tighter and LEV/PROC/CFC are centred between PK/MIC/ALC
  and the meter; VFO-A shows `step: <wheel step>` in yellow where the RX badge was, VFO-B shows nothing there.
* **VFO window (the VFO button):** no VFO A/B chips, as wide as the keypad plus the step pickers, a thin
  outline, and a Close button the size of the number keys.

More kiosk main-window details (gain grid, Squelch, LOCK/DUP, menus, Diagnostic line): [kiosk-main-window.md](kiosk-main-window.md).
