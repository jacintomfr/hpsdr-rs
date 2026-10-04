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

* Band: the bands the radio reaches, Gen, and the XVTR slots. Mode: all modes. RX Filter: the 10 fixed presets
  of the current mode (`rx_filter_presets`, converted to this app's width model `passband_for`); no Var1/Var2,
  ESSB, RTTY/FreeDV presets or FM deviation.
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

* The new MIDI actions (`ReportRec`, `ReportPlay`, `RecordWav`, `BandMenu`, `ModeMenu`, `FilterMenu`, `Rade`,
  `Squelch`, `SquelchToggle`) appear in the desktop MIDI lists; `Rade` is not offered twice in toolbar lists.
* `render_cw_text_send` / `cw_text_may_start`: the desktop SEND CW code moved into functions, same logic.
* `resolve_tune_main` replaces `resolve_tune` at the main-window call sites; with `VFO_LOCKED` false it is the same.
* `DemodParams` has new fields (`muted`, `duplex`, `squelch`, `squelch_enable`), the config has `squelch_memory`,
  `duplex`, `diag_items`; the squelch is applied for every mode change on the desktop as well (off by default).
* The spectrum waterfall switch uses `mox && !duplex` (same while DUP is off).
