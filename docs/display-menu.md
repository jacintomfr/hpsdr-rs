# Display menu (Settings -> Display)

Full-screen kiosk window (`src/display_window.rs`) mirroring deskHPSDR's Display menu (`display_menu.c`). Outside the kiosk the tab
only shows a note; the old Settings -> Spectrum tab is untouched and edits the same fields.

Pages (round radios in the header, last page remembered): General Settings, Peak Blobs & Hold, Peak Labels.

## Layout (1024x600)

Content height 590 - 8 (frame margins) = 582 px: title 22 + header 54 + gap 4 = 80, leaving 502 px. General page: 11 rows x 44 px
(34 px controls + 10 px gap) = 484 px (18 px spare). Left column x 0..470, right column x 490..966 (24 px side margins, 20 px between
columns). Not yet checked on the Pi screen (capture pending).

## General Settings: control -> field -> effect

| deskHPSDR control | Field | Effect |
|---|---|---|
| Frames Per Second 5..60 step 5 | `spectrum_fps` + `tx_spectrum_fps` | UI redraw rate (the analyzer itself runs at the fixed `SPECTRUM_FPS` = 10) |
| Relation Pan<->Waterfall 30..80 step 5 | `spectrum_waterfall_ratio` = pct/100 | divider position; divider drag still works |
| Panadapter High / Low -175..50 | `db_high` / `db_low` (Low greyed while Automatic) | trace range |
| Panadapter Step 1..20 | `panadapter_step_db` | grid; with steps under ~5 dB only every n-th line is labelled |
| Waterfall High / Low -175..50 | `waterfall_db_high/low` (greyed while Waterfall Automatic) | colour mapping |
| Waterfall Automatic | `waterfall_db_low_auto` | see algorithms |
| Panadapter Automatic | `db_low_auto` | see algorithms |
| Noisefloor Margin -20..10 (default -5) | NEW `panadapter_noise_margin` | margin of Panadapter Automatic |
| Detector Peak/Rosenfell/Average/Sample | NEW `display_detector` (default Average) | WDSP `SetDisplayDetectorMode` on pixout 0 |
| Averaging None/Recursive/Time Window/Log Recursive | NEW `display_average_mode` (default Log Recursive) | `SetDisplayAverageMode` |
| Av. Time (ms) 1..9999 step 10 (default 250) | NEW `display_average_time_ms` | `SetDisplayAvBackmult` / `SetDisplayNumAverage` |
| Display Panadapter | NEW `display_panadapter` (default on) | off: only a 44 px strip (frequency axis, dial marker) stays above the waterfall, trace/grid hidden, divider hidden |
| Display Waterfall | `waterfall_enabled` | never both off: the other one is switched on again |
| Fill / Gradient Panadapter | `spectrum_filled` / `spectrum_gradient` | trace style |
| Palette (extra) | `waterfall_palette` | waterfall colours |

Config keys (all `Option`, serde default): `panadapter_noise_margin`, `display_detector`, `display_average_mode`,
`display_average_time_ms`, `display_panadapter`; restored in `connect_to_device`, saved in the Config literal.

Extra receivers: changes made in this window to db_low/db_high/waterfall low/high/palette/waterfall_enabled/ratio are also written into
every `ExtraReceiver` (only the fields that changed). Values moved by the Automatic algorithms are NOT mirrored (each extra receiver
keeps its own Auto Low). Also stored via `remember_band_settings` like the Spectrum tab does.

## Detector / averaging (spectrum.rs)

`DemodParams::display_avg: Option<DisplayAvg>` (None for the TX analyzer = unchanged behaviour). The analyzer thread edge-detects it
(`set_display_avg`, like zoom/pan). Mode or time change: time constants written, averaging set to NONE, 50 ms sleep (spectrum thread),
then the target mode (deskHPSDR artifact workaround). `SetAnalyzer` (zoom/pan reconfigure) re-applies the choice. Conversion:
`t = 0.001 * time_ms; backmult = exp(-1/(fps*t)); num = max(2, min(60, fps*t))` with `fps` = the analyzer's fixed 10 frames/s (not the
UI "Frames Per Second", which does not pace the analyzer here); so the UI fps change does not recompute it.

## Automatic algorithms (ports)

* Panadapter Automatic (rx_panadapter.c 1996-2090, 793-797): every >= 1 s (not while transmitting) the visible spectrum (+ display
  correction) is sorted, 60th percentile + 3 dB, EMA alpha 0.25. At the first run after switching Automatic on and then every 5 s:
  `low = (trunc(nf/10) - (nf%10 != 0)) * 10 + margin`, clamped -220..-95, minus 5 (`panadapter_scale_corr`); applied if
  |diff| > 10 or current low < new low; a High <= -50 is set to -50. Replaces the old per-frame "smoothed minimum" tracker (alpha 0.01).
* Waterfall Automatic (waterfall.c 1362-1369): with every new waterfall row `low = mean(row) + correction - 5`, `high = low + 55`
  (correction = the RX display correction main.rs already applies to the waterfall levels). Replaces "smoothed minimum + 10 dB".
* Kept as it was: "AGC Auto" still runs the old smoothed-minimum tracker (own block in main.rs, only when agc_auto is on), so its
  behaviour is unchanged. Extra receivers keep their own per-window Auto Low.

## Inactive controls (greyed, "!" help, no storage)

Show Worldmap, 3D Waterfall History, Display Info Bar, Show Solardata in Info Bar, Show clock & UDP broadcast. "Show ADC OVF Alarm"
and "Show AH4 state" of deskHPSDR are not shown.

## Peak Blobs & Hold page

All deskHPSDR controls shown greyed (Enable PEAKS & HOLD, Type, Decay hold time, Drop, Enable for TX, line colour); not implemented.

## Peak Labels page

Bound to `TxUiExtra` (shared with the TX Menu's Peak Labels page): `peaks_on`, `peaks_in_passband`, `peaks_hide_noise`, `peaks_num`
(1..10), `peaks_ignore_divider` (1..150), `peaks_noise_percentile` (1..100). "Peak Labels as S-Meter values" is skipped (no such
field). The "!" says the labels are not drawn yet. The TX Menu page is unchanged.

## Open items

* Phase 2: draw the peak labels (peaks.rs exists); Peak Blobs & Hold; "Peak Labels as S-Meter values".
* Phase 3: Worldmap, 3D waterfall, info bar / solar data, clock & UDP broadcast, ADC OVF alarm.
* Verify on the Pi: layout capture, Panadapter Automatic behaviour on real signals, panadapter-off layout.

## Decisions recorded with the owner (CU2ED)

* **AGC Automatic is not in the Display menu** (neither in deskHPSDR's): it lives in the AGC menu (`agc_window.rs`: "AGC Automatic RX1" and "AGC Auto Offset" -35..-15).
  In deskHPSDR the panadapter noise-floor measurement also feeds the AGC auto level. Here it was deliberately **left on the old tracker**
  (smoothed minimum, own block in main.rs, only while `agc_auto` is on), because deskHPSDR's 60th-percentile floor sits well above the minimum and
  would shift the AGC reference. **Decision: stay as it is for now.** Switching AGC Auto to the new measurement is an open option and should be
  tested with real signals first.
* Phase 1 scope approved: General Settings page with real effect; Peak Labels page bound to the existing fields (labels not yet drawn);
  Blobs & Hold and the large features (worldmap, 3D waterfall, info bar, solar data, clock/UDP) visible but inactive.
* Ranges follow deskHPSDR (Panadapter/Waterfall levels -175..50, grid step 1..20, FPS 5..60), the Auto algorithms were replaced by deskHPSDR's
  (see above), Peak Labels will leave the TX Menu when the labels are drawn (Phase 2), the old Spectrum tab stays in sync until it is retired,
  and the Display window edits the primary receiver and mirrors the changed fields to the extra receivers.
* Known deviation: the averaging-time conversion uses the analyzer's fixed 10 frames/s, not the FPS setting (here FPS only paces UI redraws).
