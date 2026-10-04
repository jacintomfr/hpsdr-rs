# UI conventions for contributors

Rules the main window follows; keep to them when adding or changing controls.

## Look

* **Chips**: toggles and buttons use `chip_button` / `toggle_chip` (rounded
  corners, thin outline, grey = off, orange = on). Slider names use
  `framed_label`; action buttons with a fixed colour use `solid_chip` /
  `solid_chip_text`. Exceptions: STOP and MOX/TUNE/TWO TONE are red (white
  text) while active (`alert_chip_button`); SETTINGS is yellow and DIGITAL
  light blue.
* **Switches** (RxPGA, AGC Gain) use `switch_button`: orange box with black
  text when off, inverted when on.
* **No hover growth**: `with_orange_selection` zeroes egui's hover/press
  `expansion`, so nothing trembles under the mouse.

## Fixed-width readouts

Any value that changes at runtime (dB, Hz, offsets, counts, status fields)
must be drawn in a slot sized for its widest possible text and clamped to
it, so the layout never jumps when a number goes from 1 to 3 digits.
`stable_value_box` allocates one exact size for every slider value;
`chip_width` / `label_box_width` measure the widest label; the PK/MIC/ALC
column and the RIT/XIT chips are sized the same way. Measure with
`painter().layout_no_wrap` and allocate with `allocate_exact_size`, rather
than relying on a parent layout.

## Persistence

Every control the user can change must be saved to the per-radio `Config`
(`src/config.rs`, `Option<..>` fields with `#[serde(default)]`), written where
`settings_changed` is handled in `main.rs`, and applied in
`connect_to_device`. `Config::save` writes a temporary file and renames it,
keeps a `.json.bak` copy of the file as it was at start-up, and `Config::load`
falls back to that copy if the main file cannot be read. When the TX chain is
not armed, the TX-processing fields are kept from disk instead of being
overwritten with "unset", and the Digital window's forced settings are not
saved (the values from before it opened are).

## Layout: shared code, gated geometry

Desktop and the 1024x600 kiosk panel share one code base and one look. Only
geometry is gated, with `lcd_kiosk_mode()`: the larger meter (`meter_left_x`
accounts for it), the UI-scale presets, the reserved height below the
waterfall, the small ADC/FIFO warning rows, and returning focus to
secondary windows. Do not fork the branch for layout; add a condition.

## Gestures

Short click vs long press is handled by `press_gesture` (RIT/XIT: click
toggles, hold ~0.6 s clears). Brief orange confirmation flashes
(`flash_on` / `flash_start`) are for buttons without a state (A>B, B>A,
A<>B).

## Side panel (CW decoder, RADE)

The CW decoder and the RADE status share one side panel to the right of the spectrum and
waterfall (`render_cw_decoder_panel_beside`, `render_rade_side_panel_beside`; fixed
`CW_PANEL_WIDTH`). Opening it shrinks the plot instead of covering it, and anything anchored to
the plot's right edge must use the full width (`cw_panel_reserved_width` is added back), as the
audio scope does, so it does not move when the panel opens.

RADE ("RADE v1 mode") shows it while the mode is DIGU/DIGL, RADE is the active digital mode and the
Digital window is hidden: SYNC, SNR, offset, Fit Filter, a Quick Tune popup, and the received
callsigns stacked at the bottom, newest last. Its clicks are recorded in
`ConnectedState::rade_side_fit` / `rade_side_tune` and applied once per frame outside the Digital
window's own handlers, so they work while that window is closed.
