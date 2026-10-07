# Kiosk main window (1024x600 panel)

This describes what the **kiosk** build (`HPSDR_LCD_1024X600=1`, `lcd_kiosk_mode()`) does differently from the
desktop window. The desktop layout is unchanged: every layout or behaviour change below is gated by
`lcd_kiosk_mode()`. The few places where code is shared with the desktop are listed at the end.
The toolbar and its MIDI keys are in [kiosk-toolbar.md](kiosk-toolbar.md).

## Top rows

* **Band row:** active band and mode as indicators (not buttons), then SNB, ANF, BIN, NB, NR, RIT, XIT, LOCK
  and DUP. The AGC mode chip is not here any more, it sits beside the AGC Gain value box.
* **LOCK** freezes tuning (click, wheel, MIDI wheel, remote): while on, every tuning action resolves to the dial
  frequency captured at the start of the frame (`resolve_tune_main`, `VFO_LOCKED`). Not saved, the app always
  starts unlocked. Like piHPSDR/deskHPSDR, band changes and typing a frequency are not blocked.
* **DUP** (only with TX): modelled on piHPSDR's duplex. The receiver keeps running while keyed (its audio,
  spectrum, waterfall and CTUN view stay, no TX/RX silence window) and the TX panadapter goes into a small "TX"
  window at the top right. Saved. **Open item:** the user does not like the behaviour yet; ask what to change
  before touching it. Not done: piHPSDR's "Mute RX on TX" option, and silencing the local TX monitor in duplex.
* **MENU / DIGITAL / EXIT:** a vertical column at the right edge, after the meter (`ui.new_child` with an
  explicit top-down layout; `scope_builder` inherits the row's layout and reserves space, do not use it).
* rigctl/TCI/CAT are a vertical column of standard chips right after the VFO-B box.

## Gain grid (`gain_filter_grid`)

Three rows, columns lined up exactly (each name, slider and value box is one set of grid cells; a widget inside
`ui.horizontal` is drawn about 6 px lower than a plain cell, so every cell here is a plain cell):

| | col 1-3 | col 4-6 | col 7-9 |
|---|---|---|---|
| row 1 | Audio gain | TCI TX gain | Mic gain |
| row 2 | RxPGA / RX gain | TX Power | AGC Gain + AGC mode chip |
| row 3 | Squelch | Filter width | |

* **Audio gain:** clicking the name mutes/unmutes (grey name while muted). The mute lives in
  `DemodParams::muted`; the gain value is kept. Not saved (the app starts with sound).
* **Squelch:** piHPSDR's mapping (`rx_set_squelch`): slider 0..100, AM/SAM/CW use the AM squelch (-160..0 dB),
  LSB/USB/DSB the voice squelch (SSQL, 0..0.75), FMN the FM squelch (1..0.01), digital modes none. Moving the
  slider above 0.5 switches it on, below it off; clicking the name switches it on/off independently (grey = off).
  Stored **per mode** (`squelch_memory`, saved in the config) and loaded when the mode changes, like piHPSDR's
  per-mode profile. MIDI: `Squelch` (knob or encoder) and `SquelchToggle`.
* The label column widths follow the swap (`col4_w`, `col6_w`).
* The audio-scope box at the right must not touch the AGC value box (about 6 px free).

## REC / PLAY / Record on the spectrum

Top right **inside** the spectrum (drawn after it), one at a time in the same place, plus a fixed 80 px slot for
the progress bar so nothing moves:

* **REC** (signal-report recorder): 1st press shows it and records (up to 60 s), 2nd stops, 3rd hides it.
* **PLAY**: 1st press shows it and arms the playback (the recording goes out in place of the microphone while
  MOX is on), 2nd stops, 3rd hides it. Needs TX and a recording.
* **Record** (RX audio to a WAV file): 1st press records, 2nd stops and it disappears 1 s later.
* Pressing another of the three replaces the one on screen (and stops what it was doing).
* MIDI/toolbar functions: `ReportRec`, `ReportPlay`, `RecordWav`; the toolbar boxes stay lit while active
  because these have no other indicator. Code: `report_rec_press`, `report_play_press`, `report_wav_press`.

## Menus: Band, Mode, RX Filter

Toolbar functions and MIDI actions `BandMenu`, `ModeMenu`, `FilterMenu` (modelled on deskHPSDR's band_menu.c,
mode_menu.c and filter_menu.c). A popup below the start of the spectrum: Close at the top left, then a grid of
5 buttons per row with the current one lit; it stays open until Close/Escape; opening one closes the others.

* Band: the bands the radio reaches, Gen, and the XVTR slots. Mode: all modes. RX Filter: see "Filter menu" below
  (the old `rx_filter_presets` list is no longer used by the kiosk menu); FM deviation is not editable.
* **They must stay inside the main window** (`choice_window` uses an `egui::Window`). As separate OS windows
  they made the main window run at about 1 fps: windows hidden behind each other do not get frame callbacks,
  so every frame of the whole app waited about 1 s. Measured: `ui_fps 1.0, ui_ms 1000` while the DSP was fine
  (188 blocks/s, 0.1 ms per block). The VFO, Settings and Digital windows are still OS windows, so the same
  thing can happen if several of them are open and covered.

## CW Decoder / RADE side panels

The message picker and SEND CW are the first line of the CW Decoder panel (`render_cw_text_send`). The panels
take their frame margins off their size so they end above the toolbar.

## Diagnostic line

Settings -> Diagnostic (kiosk only) lists the values (`DIAG_ITEMS`); the ticked ones are drawn in the free line
between the gain grid and the spectrum, from the left edge up to the audio scope (the program leaves out what
does not fit). Fixed-width text; grey normal, orange alert, red critical (thresholds in `diag_text`).
The **PA** item is different, like piHPSDR's rx_panadapter: HL2 temperature and current, yellow, inside the
spectrum at the top left, only while transmitting, the maximum of each half second.
For deeper checks, while `/tmp/hpsdr_diag.enable` exists the app appends one line per second to
`/tmp/hpsdr_perf.log` (UI fps, DSP blocks/s, display revisions, gap/proc/queue, sample rate).

## Look

* Main window background grey 34 (egui's default is 27) so the black spectrum/waterfall stands out.
* Toolbar names use font size 22 (shrunk further if a name does not fit the box).

## Where kiosk code touches shared code (desktop)

Behaviour on the desktop is meant to be identical, but these are shared, so check them on a desktop build:

* The new MIDI actions (`ReportRec`, `ReportPlay`, `RecordWav`, `BandMenu`, `ModeMenu`, `FilterMenu`, `AgcMenu`,
  `NoiseMenu`, `TxMenu`, `RxMenu`, `SdrMenu`, `Rade`, `Squelch`, `SquelchToggle`) appear in the desktop MIDI lists; `Rade` is not offered twice in toolbar lists.
* `render_cw_text_send` / `cw_text_may_start`: the desktop SEND CW code moved into functions, same logic.
* `resolve_tune_main` replaces `resolve_tune` at the main-window call sites; with `VFO_LOCKED` false it is the same.
* `DemodParams` has new fields (`muted`, `duplex`, `squelch`, `squelch_enable`), the config has `squelch_memory`,
  `duplex`, `diag_items`; the squelch is applied for every mode change on the desktop as well (off by default).
* The spectrum waterfall switch uses `mox && !duplex` (same while DUP is off).
* MIDI `Mox` and the Space PTT unkey through `set_rade_aware_mox` (as the on-screen MOX button always did): with RADE
  armed the End-of-Over burst goes out and the RADE engine learns the over ended; cutting mox directly left RADE's
  receiver without sync after each transmission (confirmed fixed on the Pi).

## Other kiosk-related changes

* **Freq. Calibration (ppm factor)** (Settings -> RX, deskHPSDR's `ppm_factor`): -100.0..+100.0 in 0.1 steps, applied only to the frequencies sent to
  the radio (`apply_ppm` in `src/radio.rs`: P2 `phase_word`, P1 RX/TX frequency bytes), not to the display or band/filter decisions. It uses a value box
  with - and + buttons (`spin_buttons`, tap or hold) because dragging a value is impractical on the touch screen.
* **Radioberry Juice (Linux):** juice now writes to `~/.config/hpsdr-rs/radioberry-juice.log` (not a pipe: a pipe made juice die by SIGPIPE when hpsdr-rs
  exited and left the FPGA stuck), the console follows the file and a running juice is adopted live; the real-time priority is given only after
  "FPGA gateware activated"; 3 s of "NSTATUS and NCONF_DONE must be low..." start an automatic USB reset and restart (and a rediscovery). The root
  cause is not proven yet.
* **VFO encoder:** each tick moves the selected step, dynamic tune from the distance to 64 in the MIDI value, Settings -> MIDI "ticks per step" (default 10),
  17 steps (deskHPSDR's list) stored per mode.
* Discover: the Radioberry Juice setup help is a tooltip on its header.

* **VFO window and main screen:** tapping VFO A / VFO B opens the VFO window (keypad, RIT/VFO step, Lock VFOs, Duplex, CTUN, Split, like deskHPSDR's VFO menu). A>B, B>A, A<>B, Split, VFO and CTUN are no longer on the main screen (MIDI first); LOCK, DUP, CTUN and Split are shown as indicators (first row / after the Filter width box). VFO A is 38 pt, right-aligned in a fixed-width box (000.000.000).
* **Settings -> XVTR** is deskHPSDR's grid (Title, Min/Max/LO in MHz, LO error, Gain, Disable PA, Reset); the limits are applied by Update or when leaving the tab, never per field (they depend on the LO; the HL2 IF range is 0-30.72 MHz, so e.g. LO 404 MHz caps the maximum at 434.72 MHz).
* **Settings -> TX "PA enable"** (piHPSDR): off by default in the kiosk. With it off the HL2 keeps its RX gain while transmitting (no 30 dB drop in duplex) and the TR relay bit stays in RX.
* **Duplex TX window:** 150x200 px (central quarter of the TX spectrum, half for FM), draggable by touch; its position is saved in the config (`tx_window_pos`).
* **VFO headers:** "A:" / "B:" (bold, colour of the digits), band, mode and step in yellow, fixed slots, group right-aligned in the box. VFO A and VFO B each have their own step (`tune_step_hz`, `vfo_b_step_hz`); VFO B shows the XVTR RF frequency.
* **Audio scope:** in the kiosk it grows to the left until its left edge lines up with the AGC Gain value box (`draw_audio_waveform` takes the width); the Diagnostic line stops at its left edge.
* **MIDI / toolbar** are shared by every radio (`shared-controls.json`); saved MIDI devices are matched without the ALSA client:port id (`midi::port_key`). The Arduino Due shift key adds +64 to every CC/note number.
* **HL2 duplex:** with PA enable off the hardware TX LNA register follows the RX gain (no RX drop while transmitting).
* **VOX** (deskHPSDR's vox_menu): window with Close, level LED, VOX Enable, Mic Level bar, VOX Threshold (0..1000), Hang (ms, steps of 50), Side Channel Filter with low/high cut. The TX thread measures the mic level (`vox.rs`, `VoxDetector`, filter = two biquads, not WDSP's DEXP), keys `mox` itself and releases it after the hang + 50 ms, so the UI redraws nothing for VOX. Not active in CW, Tune, Two-tone, RADE/RTTY/SSTV. With VOX on, the idle mic buffer keeps the last 300 ms so the speech that triggered it is sent. MIDI/toolbar functions: `Vox`, `VoxMenu`, `VoxLevel`. No anti-VOX; TCI audio is not detected.
* **TX window position:** saved in the radio config when a drag ends and applied each time the window appears; `constrain` is only switched on after 3 frames (egui does not know the window size at first and pushed it up).
* **DSP gap:** the first chunk after a pause of the TX spectrum (and extra receivers) no longer counts as a gap (it showed seconds at each key-up).
* **Diagnostics log** (`/tmp/hpsdr_diag.enable`): also `mox=` and `audio_underruns=` per second, and `tx_window:` lines.
* **Open:** duplex split TX at 192 kHz cuts the audio occasionally (96 kHz is fine); DSP and output underrun counters are clean, check the TX mic path.
* **Discovery screen (model for kiosk touch screens):** text +3 pt (the Juice log keeps its size); the device list is fixed on top in a rounded box with a thin line; everything under it scrolls with `touch_scroll` (wide bar on the right with up/down buttons and a draggable thumb; the Juice log has its own); buttons and fields are rounded with a thin line. The window is requested as `size / native_pixels_per_point` (1000x550 px): winit multiplies viewport sizes by the native scale (1.33), so the old 1000x580 request was a 1333x773 px window bigger than the 1024x600 panel. The other kiosk viewports may need the same fix.
* **Discovery window size:** the window system scales viewport sizes by a factor egui does not always report (1.33 at start, 1.0 after EXIT from the radio screen), so the window measures the size it got and re-requests size and position corrected by the real factor (up to 4 tries). Touch targets are 36 px high (`interact_size.y`, `button_padding` 4x7). `f32::clamp` panics when min > max: the thumb length in `touch_scroll` is limited to the track height first (this made EXIT close the whole program).
* **Top right (kiosk):** two big touch buttons, MENU and EXIT, with a 6 px gap, together as tall as the meter (85 x `KIOSK_METER_SCALE`), label centred (`solid_chip_text_wh` adds them in a centred, justified cell). The DIGITAL button is gone there: the MIDI/toolbar function `Digital Menu` (`toggle_digital_window`) opens/closes the Digital window like it did.
* **RADE side panel (kiosk):** Fit Filter and Quick Tune are touch buttons (about 41 px high, rounded, thin line); the Quick Tune menu items are 49 px apart.
* **Digital window (kiosk):** touch look (`apply_kiosk_touch_style`: 36 px targets, 24 px checkboxes, rounded widgets with a thin line) and the panel inside `touch_scroll` (room kept for CLOSE). SSTV: the Sync meter shares the Auto-save line and the status ("hunting for a header...") the Quick Tune line; in TX the first line holds `SSTV TX`, My Call, FSK ID, TX Lead and TX Slant, the Load Picture line ends with Send / Abort TX / progress. RTTY: Clear RX, the lock dot with the confidence bar and then the AFC offset follow the baud choices on the first line. The yellow CLOSE / EXIT button is 96x46 px, label only (the old "✕" glyph is not in the font and showed as a square).
* **On-screen keyboard** (`src/kiosk_keyboard.rs`): appears at the bottom of the window (top if the field would be under it) whenever a text field has the focus; QWERTY, digits, Shift, Backspace (repeats), Enter, `?123` symbols, space, hide. `begin(ctx)` is called at the top of the UI pass of the main window and of the Discovery, Settings and Digital viewports and pushes `Text`/`Key` events into that pass' input; while the pointer is on the keyboard "surrender focus on press" is switched off so the field keeps the focus. Keys are painted with a non-focusable sense.
* **Transverter memory:** each XVTR slot remembers its last frequency (IF) and mode (`xvtr_memory`, saved as `xvtr_settings` in the radio config, updated every frame while the slot is active); selecting the slot returns there (`select_xvtr`), or to the start of its range the first time or when the saved frequency is out of the range.
* **SSTV TX first line (kiosk):** `TX` (strong), My Call, FSK ID, `Lead (ms)` and `Slant (ppm)` as value boxes with touch buttons (`spin_buttons_full`: Lead `- +` in steps of 10 ms, Slant `- 0 +` in steps of 5 ppm with the middle button resetting to 0), instead of the drag value and the slider.
* **Meter options (kiosk, like deskHPSDR's meter_menu.c):** a tap on the meter opens an in-app window (`meter_options_window`) with big buttons: Meter type (Digital / Analog), S-meter reading (Peak / Average, `RXA_S_PK` / `RXA_S_AV` via `spectrum::set_smeter_peak`) and, with TX, TX ALC reading (Peak / Average / Gain, `TXA_ALC_PK` / `_AV` / `_GAIN` via `tx::set_alc_mode`). Saved in the radio config (`smeter_mode`, `alc_mode`, `meter_style`). The meter area stays inert; the tap is tested against the drawn (enlarged) rectangle only so REC/PLAY keep working. The Close button of the in-app windows (Band/Mode/Filter, meter) is 110 px wide with a centred label (`touch_close_button`).
* **Audio device pickers (Settings -> Audio):** on Linux/ALSA cpal reports every PCM alias of a card (hw, plughw, sysdefault, front, surround*, iec958, dmix, dsnoop, usbstream and its numeric CARD=0 twins) with the same description, so each card was listed 10-20 times and opening "by name" took the first alias (the exclusive `hw:`). `audio::enumerate_devices` keeps one entry per card (sysdefault > plughw > hw, grouped by label) plus "PulseAudio / PipeWire", and `find_device` opens that entry; saved selections (labels) still work. `HPSDR_RS_AUDIO_DEBUG=1` logs the raw list. Other systems: names already unique, only de-duplicated.
* **TX meter SWR** (like piHPSDR/deskHPSDR `transmitter.c`): `smoothed_swr` is only updated while the forward power is above 0.25 W (otherwise it moves back towards 1.0), the reflection coefficient is capped at 0.95 (SWR 39) and it is lightly smoothed (0.7/0.3). Without it the raw rev/fwd ratio of two noise-level detector readings gave meaningless SWR values (up to 173) in SSB silence. The HL2 with the PA30 board keeps using the Hermes constants (`power_meter_board`). The perf log (`/tmp/hpsdr_diag.enable`) also records `fwd_raw`, `rev_raw`, `fwd_sm`, `rev_sm` and `swr` per second.
* **HL2-PA30 detector offsets:** with the transmitter keyed and no RF, this board's forward/reverse detectors read about 261 / 255-262 of 4095 (8 / 9 in RX), so the reverse offset looked like reflected power (13 W into a 1.0:1 load showed SWR 1.6, SSB silence showed 39). `power_detector_offsets` (only the HL2 with `HL2_PA30_MAC`, 258 forward / 253 reverse) is subtracted for the SWR calculation only; the displayed power keeps its calibration (it matches an external wattmeter with the offset included). Verified against an MFJ meter: 13 W / 1.0 matched, 12 W / 1.3 into a real mismatch matched.
* **WDSP EQ window (like deskHPSDR's "WDSP EQ Menu", `src/eq_window.rs`):** full-screen in-app window (no spectrum needed while adjusting): title with the active mic profile, Close, RX1 / TX / extra-receiver radios, Enable, "Added Frequency-Independent Gain" spin, the framed curve plot (log 10 Hz..16 kHz, +-20 dB, 12 draggable points, drag applies on release, wheel changes the NURBS weight), `Curve:` Legacy linear / Linear (1) / Cubic (3) / Degree 5 / Degree 7 and "NURBS weights", and the 12-point table as 4 columns of spin boxes. Opened by the Settings -> Equalizer tab (kiosk; the old panels stay on the desktop) or the toolbar/MIDI function `EQ Menu`. Curve maths in `src/eq_curve.rs` (port of WDSP nurbs.c, unit tested); `SetRXA/TXAEQCurve` + `SetRXA/TXAEQWeights` applied after the profile in `spectrum.rs`/`tx.rs`. EQ is kept per mode group (SSB / CW / DIG / ... , `eq_profiles::eq_mode_group`, maps `rx_eq_by_mode`/`tx_eq_by_mode` in the config) and swapped on mode change (`eq_window::mode_tick`). Three mic-profile slots (`mic-profile-N.json` in the settings dir: description, TX/RX EQ, leveler, compressor, CFC, TX denoiser) are loaded/saved from the top-right of the window.
* **SDR Device (Settings tab, full-screen window in the kiosk, `render_sdr_device` / `sdr_device_window`):** deskHPSDR's "SDR Device Settings" in three columns by topic with touch-sized controls (`touch_checkbox` with a big X, spin buttons scaled by the `spin_touch_scale` data, +3 pt font): RECEIVE (Filter Board + N2ADR HPF, Sample Rate, Freq. Calibration, RX Gain Cal), TRANSMIT (LNA during TX or TX ADC0 attenuation, PA enable, mic connector on Orion boards, Ext. TxInhibit, Ext. AutoTune, HL2 ATU TUNE, VFO Encoder Divisor, Audio Capture Time = max length of REC), AUDIO / HARDWARE (Send RX audio, HL2+ codec, HL2 CL1 10 MHz ref clock, Your Callsign, Your Locator, IARU Region). Moved here from the RX/TX/Open Collector/MIDI tabs; ADC selection (only with 2+ ADCs) went back to the RX tab like deskHPSDR. Your Callsign (`own_callsign`, migrated from `rade_callsign`) is the single callsign for RADE/SSTV/RTTY (their "My Call" boxes are gone, desktop too); Your Locator is new. IARU Region (1/2/3, default 2) changes the 160/80/40 m edges (`Band::low()/high()`). External inputs/clock in `radio.rs`: TxInhibit/AutoTune read IO inputs of the P1 status address 0 / P2 status byte 59 (`HwIoInputs`, `hardware_tx_inhibit`, `auto_tune_asserted`), `radio_inputs_tick` cuts TX or starts/stops TUNE (`tune_set`); HL2 CL1 sends deskHPSDR's 48-byte I2C table. HL2 ATU TUNE has no wire effect yet (the HL2 already sets the ATU bit on TUNE).
* **Help popups (rule):** explanatory text is not on the screen; a round "!" (`help_button`) at the end of the control opens a window with the text and OK. First applied on SDR Device.
* **Close buttons:** CLOSE is yellow with black text everywhere (`touch_close_button`, `kiosk_accent_button`); in the full-screen windows (EQ, SDR Device) it sits bottom right with a 24/26 px gap; the other kiosk windows use the same offset. The on-screen keyboard keeps a 28 px gap from the top/bottom edge. MIDI "VFO Step Up/Down" change the tuning step (1 Hz, 10 Hz ...), not the frequency.
* **Filter menu (`src/filter_window.rs`, tables in `src/filters.rs`):** deskHPSDR's "Set RX Filter", same non-full-screen style as BAND/MODE (anchored above the toolbar with a gap, 5 buttons per row). The 15 fixed filters of the mode (filter.c tables, empty titles hidden), the ESSB / digital specials, "Set TX = RX filter edges" (`TxExtra::use_rx_filter`), and editable Var1/Var2 with Default buttons (width/shift spins in CW, DSB, AM, SAM, SPEC, DRM; low/high cut otherwise). Chosen filter and Var edges per mode are saved (`filter_sel`, `filter_vars`); `tick()` re-applies the choice on mode change and drops the explicit passband when the width slider is moved.
* **AGC menu (`src/agc_window.rs`):** AGC mode, AGC Automatic, Auto Offset (-35..-15) and Hang Threshold (0..100, only Long/Slow, `SetRXAAGCHangThreshold`, saved as `agc_hang_threshold`). Opened by a long press / right click on the AGC chip or the MIDI/toolbar function `AgcMenu`; opening it closes Band/Mode/Filter.
* **Noise menu (`src/noise_window.rs`), TX Menu (`src/tx_window.rs`), RX Menu (`src/rx_window.rs`):** full-screen windows mirroring deskHPSDR (opened from Settings or the functions `NoiseMenu`, `TxMenu`, `RxMenu`, `SdrMenu`). Noise: NR/NR2/NB/NB2/ANF/SNB (NR3/NR4 shown but inactive), NB threshold, MNF. TX: Basic, Audio Tools (DEXP, phase rotator, CTCSS), CFC, Peaks, with import/export of profiles. RX: RX1 page and an Options page (Hardware, P2 network). Everything is saved (`TxUiExtra`, `RxUiExtra`, `noise_extra`). Not yet effective: "Use Drive levels per Band", "Tune Drive = TX drive", "Fill TX Panadapter", SDR LineIn/Mic Boost/POST monitor, P2 jitter buffer; peak labels (`peaks.rs`) are computed but not drawn.
* **Toolbar FNC layers:** 8 layers (FNC 0..7) plus the assignable `FuncRev` ("FNC-") and `FNC's` (quick jump) like deskHPSDR; layers 6 and 7 start empty (None). See toolbar-fnc.md. Toolbar/MIDI names: AGC SET, NOISE, TX SET, RX SET, SDR SET.
* **Pi desktop (not in the repo):** the launchers on the Pi desktop are drawn in rectangles baked into the wallpaper (`~/.config/pcmanfm/default/wall_boxes.png`, all 154x122 px) with positions in `desktop-items-HDMI-A-1.conf`.
