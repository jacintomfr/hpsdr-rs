# Display menu (Settings -> Display)

Full-screen kiosk window (`src/display_window.rs`) mirroring deskHPSDR's Display menu (`display_menu.c`). Outside the kiosk the tab
only shows a note; the old Settings -> Spectrum tab is untouched and edits the same fields.

Pages (round radios in the header, last page remembered): General Settings, Peak Blobs & Hold, Peak Labels.

## Layout (1024x600)

Content height 590 - 8 (frame margins) = 582 px: title 22 + header 54 + gap 4 = 80, leaving 502 px. General page: 11 rows x 44 px
(34 px controls + 10 px gap) = 484 px (18 px spare). Left column x 0..470, right column x 490..966 (24 px side margins, 20 px between
columns). Peak Blobs & Hold page: 7 rows x 44 = 308 px; Peak Labels page: 7 rows x 44 = 308 px (194 px spare each); the colour rows end at x = 906 of 966. Not yet checked on the Pi screen (capture pending).

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

## Phase 2: Peak Labels, Peak Blobs & Hold (src/peaks.rs, drawn in main.rs after the trace)

Fields: all in `TxUiExtra` (config.rs, `#[serde(default)]` on the struct, so no migration; shared with the old TX Menu fields): `peaks_on`,
`peaks_in_passband`, `peaks_hide_noise`, `peaks_num` (1..10, default 4), `peaks_ignore_divider` (1..150, 24), `peaks_noise_percentile`
(1..100, default now 80 like deskHPSDR, was 50; stored values are kept), NEW `peaks_as_smeter` (false), `peak_hold_on` (false), `peak_hold_mode`
(1 hold / 2 decay, default 2), `peak_hold_sec` (0.1..5.0 step 0.1, 2.0), `peak_hold_drop_db` (1..10 step 0.5, 6.0), `peak_hold_tx` (false),
`peak_line_col` (RGBA 0.70,0.70,0.70,1.0), `tx_pan_col` (0,1,0,1). The kiosk has no colour dialog: swatch + R, G, B spin buttons (0..255,
step 5); alpha is fixed. The Peak Labels section left the TX Menu (the three remaining sections are re-spaced); the fields stay.

Where it is drawn (main.rs, RX1 panadapter block, only when the panadapter is shown; the same code path serves RX and TX, `transmitting`
selects the spectrum): trace, then Peaks & Hold line (1.5 px, line colour), then the peak labels (white, 16 px, over everything). The
extra receivers' windows (`render_extra_receiver_ui`) have their own simpler drawing and get neither. All work is done on the displayed
(smoothed) row of 1024 bins: ignore range = ceil(bins / divider) bins, labels are laid out in plot pixels.

### Peak labels (rx_panadapter.c 2092-2287)

Local maxima `s > both neighbours` (first/last sample forced to -200 like the C code), `num_peaks` slots, a new maximum within the ignore range
replaces the slot only if stronger, otherwise the lowest slot is replaced when stronger; exchange sort descending. "In passband only" =
only between the filter edges (x of the passband overlay converted to bins). "Hide below noise floor": `noise_level` = percentile of the
visible row + 3 dB, recomputed at most once per second (own cache, also when the percentile or RX/TX changes). Label y =
`floor((high-peak)*h/(high-low)) - 5`, clamped to the text height at the top; overlap with an earlier label (dy < height and dx < width)
moves it down by height+5, else up, else right/left by width+5. Text `"%d dBm"` (integer truncation), or with "as S-Meter values"
deskHPSDR's `dbm2smeter[get_SWert(f, dBm)]` table (meter.c: "S1".."S9", "S9+5db".."S9+60db", "no signal", "out of range"; S9 = -73 dBm up to
30 MHz, -93 dBm above). Not the app's own `s_meter_label` (that one appends dBm and uses another rounding).

Differences between the old peaks.rs and deskHPSDR, now fixed: default noise percentile 50 vs 80; label text `{:.1}` instead of `%d dBm`;
first/last sample not forced to -200; noise level recomputed on every call instead of the 1 s cache. Intentional deviation: deskHPSDR compares
`s` (with the display offset) against neighbours without the offset; here the row already carries the correction, so both sides match.

### Peaks & Hold (rx_panadapter.c 1674-1768, 1881-1906; tx_panadapter.c 425-475, 526-556)

One buffer + age array per panadapter (RX and TX separate, `PeakHold`). The buffer is cleared when the mode changes, the feature is toggled
(reset while off), the bin count or the Hz per bin (zoom/span/sample rate) changes; when the left edge frequency (view centre - half span)
moves by whole bins the buffer is shifted like `rx_panadapter_peak_hold_shift`. Updated once per new analyzer row (10 rows/s, not per UI
frame, so `fps` = 10 as in the averaging conversion): mode 1 per-bin maximum, never decays; mode 2 new maximum sets age 0, after
`hold_frames = (int)(hold_sec*fps+0.5)` frames the bin drops by `drop/fps` per frame. TX (only with "Enable PEAKS & HOLD for TX", never in
duplex because the main panadapter then shows the RX): tx_panadapter.c's fast attack / slow release at the Drop rate, whatever the type.
TX line/fill colour: the main panadapter while transmitting (outline always, fill at the existing alpha 110) and the duplex "TX" window's line.

## Open items

* Phase 3: Worldmap, 3D waterfall, info bar / solar data, clock & UDP broadcast, ADC OVF alarm.
* Verify on the Pi: layout capture of the three pages, Panadapter Automatic behaviour on real signals, panadapter-off layout, and the
  Phase 2 drawing (labels, hold line, decay timing, shift while retuning/zooming, TX colour), none of which has been run on hardware.
* Peak labels / hold on the extra receivers' panadapters (not drawn there).

## Decisions recorded with the owner (CU2ED)

* **AGC Automatic is not in the Display menu** (neither in deskHPSDR's): it lives in the AGC menu (`agc_window.rs`: "AGC Automatic RX1" and "AGC Auto Offset" -35..-15).
  In deskHPSDR the panadapter noise-floor measurement also feeds the AGC auto level. Here it was deliberately **left on the old tracker**
  (smoothed minimum, own block in main.rs, only while `agc_auto` is on), because deskHPSDR's 60th-percentile floor sits well above the minimum and
  would shift the AGC reference. **Decision: stay as it is for now.** Switching AGC Auto to the new measurement is an open option and should be
  tested with real signals first.
* Phase 1 scope approved: General Settings page with real effect; Peak Labels page bound to the existing fields (labels not yet drawn in Phase 1, done in Phase 2);
  Blobs & Hold (done in Phase 2) and the large features (worldmap, 3D waterfall, info bar, solar data, clock/UDP) visible but inactive.
* Ranges follow deskHPSDR (Panadapter/Waterfall levels -175..50, grid step 1..20, FPS 5..60), the Auto algorithms were replaced by deskHPSDR's
  (see above), Peak Labels left the TX Menu in Phase 2, the old Spectrum tab stays in sync until it is retired,
  and the Display window edits the primary receiver and mirrors the changed fields to the extra receivers.
* Known deviation: the averaging-time conversion uses the analyzer's fixed 10 frames/s, not the FPS setting (here FPS only paces UI redraws).

## Open investigation: S-meter vs spectrum (Peak Labels "S4" vs S-meter "S9")

Observed by the owner (Pi, 20 m, USB, 14.099 MHz): a narrow spike showed **S4** (about -103 dBm) as a Peak Label and in the spectrum axis,
while the S-meter showed **S9 / -73 dBm** (S-meter mode "PK" in the meter). **Deferred: needs more study.**

What is known:
* The Peak Label is faithful to the displayed spectrum (the spike sits at about -103 dBm on the axis). The disagreement is **spectrum vs
  S-meter**, and it is older than the labels.
* It is **not a missing calibration offset**: the S-meter (`GetRXAMeter(RXA_S_AV | RXA_S_PK)`, spectrum.rs) and the spectrum pixels (`GetPixels`)
  receive the same `meter_calibration_db` / `rx_display_correction_db` (RX Gain Cal, the HL2 attenuator term, Alex, XVTR gain), in main.rs.
* The structure matches deskHPSDR: `receiver.c` does `GetRXAMeter(S_PK|S_AV) + (rx_gain_calibration - band->gain) + attenuation - adc gain`
  (+ Alex/Charly25) and its panadapter uses the same offset. The HL2 term here is `rx_gain_calibration - (stored_rx_atten - 12)`; the exact
  equivalence with deskHPSDR's `adc.attenuation - adc.gain` for the HL2 was not checked.
* The obvious explanations do **not** add up to 30 dB for a narrow signal: Detector=Average lowers a narrow tone by about 8-12 dB; integrating
  the noise over a 2.7 kHz filter adds about 26 dB to a per-bin noise floor (about -104 dBm); peak vs average meter mode is another few dB.
  For a wide signal (speech, noise, RADE) a single bin holds only a fraction of the channel power (10*log10(BW/RBW), about 26 dB for 2.4 kHz
  at about 6 Hz per bin), so the label is *meant* to read lower than the meter there (same in deskHPSDR).

Diagnostic (temporary, main.rs, next to the Peaks & Hold code): while `/tmp/hpsdr_diag.enable` exists, one line per second (receive only) is
appended to `/tmp/hpsdr_perf.log`:

    smeter_diag: meter=<dBm> smeter_mode=<peak|avg> freq=<Hz> mode=<..> pass_bins=<lo>..<hi> (<Hz>) pass_max=<dBm>@bin<n>
                 pass_sum=<dBm> row_median=<dBm> row_max=<dBm> hz_per_bin=<..> zoom=<..> sr=<..>

Use: `touch /tmp/hpsdr_diag.enable`, wait about 20 s with the signal in the filter, `rm /tmp/hpsdr_diag.enable` (the same file also enables the older
per-frame CSV diagnostics, so do not leave it), then `grep smeter_diag /tmp/hpsdr_perf.log`. Reading it: if `pass_sum` is close to `meter` and
`pass_max` far below, the difference is bandwidth; if neither gets close, the meter is seeing something else (other signal, noise, spur) or the two
paths differ in some other way. Next steps when this is picked up: capture with the diagnostic on the same signal, tune the signal out of the
filter and see whether the meter follows, try Detector=Peak and meter mode Average, and ideally repeat the author's test with a signal generator
(50 uV = -73 dBm = S9).

### RESOLVED: the S4 vs S9 discrepancy was the spectrum smoothing (owner confirmed after build 0.8.7-379)

Root cause: `smooth_spectrum_values` (5-tap kernel [0.06, 0.24, 0.40, 0.24, 0.06]) averages the **dB values** of neighbouring bins, not their
power. A narrow carrier occupying a single bin at -73 dBm over a -130 dBm floor comes out as
`0.40*(-73) + 0.24*2*(-130) + 0.06*2*(-130) = -107 dBm`: about **34 dB lower**, i.e. S9 became S4. The Peak Labels were computed on that
smoothed row, so they inherited it. The S-meter and the raw spectrum were right all along. The explanations written above ("bandwidth",
detector, noise integration) were wrong for this case. With "Smooth trace" OFF (the new default, which draws the raw row like deskHPSDR) the label
and the S-meter agree (both S9 / -73 dBm in the owner's capture).

Consequence to keep in mind: with "Smooth trace" ON the Peak Labels and the Peaks & Hold line still use the smoothed row and will read narrow
signals low again. deskHPSDR uses the raw row for labels. Suggested follow-up (not done): always compute labels and hold on the raw row, whatever
the trace option. The temporary `smeter_diag` block in main.rs is no longer needed and can be removed.

**Follow-up done:** the Peak Labels and the Peaks & Hold line now ALWAYS use the raw (corrected) spectrum row, whatever the "Smooth trace" option
(like deskHPSDR), so narrow signals read correctly even with the smooth trace on. The temporary `smeter_diag` block was removed from main.rs
(the `/tmp/hpsdr_diag.enable` file no longer produces `smeter_diag` lines).

## Audit corrections, part 1

Result of the audit of this port against deskHPSDR (rx_panadapter.c, waterfall.c, receiver.c, transmitter.c, tx_panadapter.c). Part 1 changes:

1. **Panadapter High is no longer forced every 5 s.** deskHPSDR (rx_panadapter.c:2067) sets High to -50 at every automatic calculation when it is -50 or
   lower, so a manual -60 snapped back while Panadapter Automatic was on. *Deliberate deviation:* the rule is applied only ONCE, at the first
   calculation after Panadapter Automatic is switched on (`AutoState::high_rule_pending`); afterwards the user's High stays. Help text updated.
2. **Waterfall Automatic no longer overwrites the manual Waterfall High/Low.** The automatic limits live in runtime-only
   `ConnectedState::wf_auto_low/high` (not saved, not in band memory). While Automatic is on the texture build, the greyed Display spins and the old
   Settings -> Spectrum sliders use/show them; switching Automatic off uses the untouched manual values again. Band memory, TX waterfall values and
   extra receivers still use the manual values.
3. **Noisefloor Margin applies at once** (`AutoState::force()`, like rx_panadapter_force_noisefloor_update()): the next tick measures and calculates
   immediately, and the first calculation always applies (the "moves only if |diff| > 10 or below" gate no longer swallows it).
4. **Defaults like deskHPSDR** (only for configs without a stored value; stored values untouched, FPS clamped to 5..60 on load).
5. **Peak labels RX/TX parameter sets.** RX defaults 3 / 20 / 80 (unset values only). A separate TX set (`peaks_tx_num` 4, `peaks_tx_divider` 24,
   `peaks_tx_percentile` 50, `peaks_tx_hide_noise` true, `peaks_tx_in_passband` false) is used while transmitting, labels formatted `%.1f dBm`
   (never S-meter). One enable (`peaks_on`) for both. The Peak Labels page has an RX and a TX column. The noise-level cache is keyed by
   (percentile, tx) so it is recomputed when the set switches.

| Setting | Old default | New default |
|---|---|---|
| Panadapter High | -40 | -55 |
| Waterfall High | -60 | -55 |
| Panadapter Step | 10 | 20 |
| Panadapter Automatic | on | off |
| Waterfall Automatic | off | on |
| Fill / Gradient Panadapter | off / off | on / on |
| Relation Pan<->Waterfall | 150/350 (43 %) | 70 % |
| Frames per second | 30 | 10 |
| Peak labels RX number / divider / percentile | 4 / 24 / 80 | 3 / 20 / 80 |
| Peak labels TX number / divider / percentile / hide / passband | (shared with RX) | 4 / 24 / 50 / on / off |

**Left for part 2:** y clamp, edge columns, flat-mode stroke colour, waterfall 1:1 and per-row colours, extra receivers path, hold line width, hold
clear on span change.

## Audit corrections, part 2

Drawing code of the spectrum/waterfall (all in `src/main.rs`):

1. **y is not clamped** (RX and TX share the code, "Smooth trace" OFF). Values below Low / above High land outside the plot like in cairo; the trace,
   fill and hold line are painted with a clip rect equal to the plot area, so the fill keeps its shape and the trace no longer flattens along the
   bottom when Low is above the noise floor. y is only limited to plot -10000..+10000 px (overflow guard); non-finite values count as -200 dBm.
   The gradient colour is a function of the vertex height (saturates), the geometry is the unclamped y. Smooth trace ON keeps the old clamped path.
2. **Edge columns.** With Smooth trace OFF the first and last sample of the DRAWN trace are forced to -200 dBm (local copy; peaks, hold and the noise
   measurement use the real row), so the fill closes at the bottom with steep edges.
3. **Flat-mode stroke colour (RX, Gradient off):** white alpha 0.50 (128/255, COLOUR_PAN_FILL2) when filled, white alpha 0.75 (191/255,
   COLOUR_PAN_FILL3) when not filled; it replaces the light green. TX keeps its configured colour. Widths unchanged: 0.5 filled / 1.0 unfilled
   (raw trace).
4. **Exact gradient stops.** The fill mesh now uses horizontal rows at absolute heights: 13 uniform levels plus one exactly at each RX stop
   (0.20 / 0.55 / 0.80 / 1.0 x S9n), sorted (at most 17 rows per column; a column only gets the levels below its own top; the flat fill uses 2
   rows). TX gradient: uniform rows only. The vertex count per column is about the same or lower than before (26).
5. **Waterfall colours are baked per row** (like deskHPSDR's pixbuf): `WaterfallBake` keeps a rolling colour image (row length x display rows); a new
   row is coloured once with the limits current at that moment and the older rows scroll down, so with Waterfall Automatic the history no longer
   re-shades. New rows are found by identity (the newest baked row is kept as an `Arc` and searched in the history; its position = number of new
   rows), not by revision (revision also counts spectrum-only updates). Full rebuild from the dB rows with the current limits: palette, display
   height, row length, TX/RX switch, Waterfall Automatic toggled, manual limits changed (or the RX gain correction that shifts them), history empty,
   or the baked row is gone (lost rows, or as many new rows as the image is high). The upload (`tex.set`, LINEAR) only happens when the image changed.
   Extra receivers still rebuild the whole texture.
6. **Extra receivers** follow the "Smooth trace" option (a global atomic written by the main window): OFF = raw polyline, y floored, not clamped,
   clipped, 1.0 px; ON = the old smoothing + spline. No fill/gradient there.
7. **Hold line:** 1.0 px (PAN_LINE_THICK) with the same floored, unclamped, clipped y as the raw trace; still cleared on span change as before.

Left: pixel count = widget width (deskHPSDR has one sample per pixel), bilinear stretch of the waterfall texture, HiDPI floor (y is floored to
physical pixels), the "weak" (inactive receiver) gradient/fill colours.
