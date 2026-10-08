# Spectrum and waterfall: hpsdr-rs vs deskHPSDR (code comparison)

Why: the owner noticed that the top line of the panadapter trace looks different (hpsdr-rs: thin and smooth; deskHPSDR: jagged,
"sawtooth"). Four read-only analyses were made (spectrum and waterfall of each program), all values below were read from the code
(hpsdr-rs: `src/spectrum.rs`, `src/main.rs`, `src/display_window.rs`; deskHPSDR: `src/receiver.c`, `src/rx_panadapter.c`,
`src/waterfall.c`, `wdsp-2.10/analyzer.c`). Line numbers drift: re-check before editing.

**Result in one sentence:** both programs get the same pixel row from WDSP with the same analyzer settings; deskHPSDR draws it
untouched (one independent value per pixel, straight segments), hpsdr-rs smooths it (5-tap filter + Catmull-Rom spline) before drawing.

## 1. Spectrum: acquisition (same in both)

| Item | deskHPSDR | hpsdr-rs |
|---|---|---|
| Analyzer | `XCreateAnalyzer(id,&rc,262144,1,1,NULL)` | `XCreateAnalyzer(ch,&ok,262144,1,1,wisdom_dir)` |
| Data type / stitches / clip | complex I/Q, 1, 0 | same |
| Window | type `pan_window_type` = 5, `kaiser_pi` 14.0 | `WIN_TYPE` 5, 14.0 |
| FFT size | `want = max(width*zoom, ceil(sr/fps))`, rounded up to 16384/32768/65536/131072/262144 | `required = max(1024*zoom, ceil(sr/10))`, same tiers, minimum 16384 |
| Overlap | `max(0, ceil(afft - sr/fps))` | `max(0, ceil(afft - sr/10))` |
| `max_w` | `afft + min(0.1*sr, 0.1*afft*fps)` | `afft + min(KEEP_TIME*FPS, KEEP_TIME*afft*FPS)` |
| FPS of the analyzer | `rx->fps`, default 10 (30 on macOS) | `SPECTRUM_FPS` = 10, fixed |
| Detector / averaging (defaults) | `DET_AVERAGE`, `AVG_LOGRECURSIVE`, 250 ms | `AVERAGE`, `LOG_RECURSIVE`, 250 ms (Display menu, phase 1) |
| Averaging maths | `backmult = exp(-1/(fps*t))`, `num = max(2, min(60, fps*t))`: 0.670 and 2 at 10 fps | same formulas with fps fixed at 10 |
| Mode change | NONE, sleep 50 ms, target | same |
| Normalisation | `SetDisplayNormOneHz(id,0,1)`, `SetDisplaySampleRate(id, width*zoom)` | `SetDisplayNormOneHz` on both pixouts, `SetDisplaySampleRate(ch, pixels)` |
| Correction | `soffset = rx_gain_calibration - band->gain + attenuation - adc gain` (+ Alex / Charly25) added to every pixel | `meter_calibration_db` in the analyzer thread + `rx_display_correction_db` in the UI (RX Gain Cal, HL2 attenuator term, Alex, XVTR gain); same quantity |

Differences in acquisition:

* **Pixels.** deskHPSDR asks WDSP for `width * zoom` pixels (and pan is a window into that array). hpsdr-rs always asks for 1024
  pixels and implements zoom/pan by growing the FFT and clipping (`fscLin/fscHin`), then stretches the 1024 values to the plot.
  At the owner's screens (about 1000 px wide) the pixel counts are nearly equal, so this is not what he sees; on a wide window
  hpsdr-rs has fewer pixels than screen columns.
* **Pixouts.** deskHPSDR uses 1 pixout shared by panadapter and waterfall. hpsdr-rs uses 2: pixout 0 (spectrum, with detector/averaging)
  and pixout 1 (waterfall, raw). See section 4.
* **Pixel formation (WDSP).** With bins > pixels (normal) the Average detector takes the linear mean of the bins of each pixel
  (about 10 bins per pixel at 48 kHz / 1600 px, about 20 at 192 kHz), corrected by the window's ENBW; then the log-recursive average
  across frames. So every pixel is an independent noisy value: this is where the fine detail comes from, in both programs.

## 2. Spectrum: between `GetPixels` and the drawing (the real difference)

| Step | deskHPSDR | hpsdr-rs |
|---|---|---|
| Edge handling | first and last visible column forced to -200 dBm (the fill closes at the bottom) | hard clamp of every value to [db_low, db_high] |
| Spatial smoothing | **none** (optional `pan_peak_preserve`, default off: max of 3 neighbours) | **5-tap kernel [0.06, 0.24, 0.40, 0.24, 0.06]** (`smooth_spectrum_values`), passes = 1 + (zoom-1)/4 (1 up to zoom 4, 2 at 5..8, ...) |
| Effect on noise | none | weight sum of squares = 0.28: noise power between neighbours drops about 3.6x; more with zoom |
| Resampling / curve | none: `y = floor((high - (v + soffset)) * h / (high - low))`, integer y | **Catmull-Rom spline** (`smooth_trace`), 4 sub-segments per bin span, about 4000 points |
| Temporal smoothing | only WDSP's log-recursive | only WDSP's (same) |

This is the cause of the visual difference. The raw row is also what deskHPSDR draws, so the sawtooth is real signal statistics, not a
rendering artefact.

## 3. Spectrum: drawing

| Item | deskHPSDR (cairo) | hpsdr-rs (egui) |
|---|---|---|
| Primitive | polyline, one `move_to/line_to` per widget pixel column, no curve | one `Shape::line` over the spline points |
| Stroke | 0.5 px when filled, 1.0 px when not; stroked with the same gradient as the fill; cairo default anti-aliasing | 1.5 px (1.0 over the fill), solid LIGHT_GREEN; egui anti-aliasing |
| Fill | `close_path; fill_preserve` with a vertical linear gradient | mesh of quad strips (12 vertex rows per column), flat `rgba(0,200,0,110)` or gradient with alpha 190 |
| Gradient stops | 0 GREEN (0,1,0); S9*0.20 YELLOW (1,1,0); S9*0.55 ORANGE (1,0.66,0); S9*0.80 RED (1,0,0); S9 PURPLE (0.75,0.25,1); S9 = -73 dBm (-93 above 30 MHz) with a +10 dB shift, normalised to (S9-low)/(high-low), clamped 0..1 | t=0 (0,255,0); 1/3 (255,168,0); 2/3 (255,255,0); 1 (255,0,0); t = (dB-low)/(s9-low), S9 = -73 / -93 dBm |
| No gradient | white fill, alpha 0.25 inactive / 0.50 active filled / 0.75 active unfilled | flat green fill alpha 110 |
| Grid | cached surface, cyan (0,1,1) 0.5 px lines, labels "%d dBm" FreeSans Bold 12 with a 2 px black halo, MHz.kHz frequency labels | gray 55 1 px lines, labels "{db} dB" monospace 14 gray |
| Filter passband | gray rectangle (0.40, 0.40, 0.40, alpha 0.75) | blue translucent `rgba(70,150,230,50)` with 1 px edge lines, full height of the rect |
| Overlays | AGC knee / hang lines (coral), noise-floor text, info bar, worldmap | peak hold, peak labels (phase 2), RTTY/SSTV/RADE cursors |
| Update | GTK timer 1000/fps (100 ms); redraw only when `GetPixels` returns a new frame | UI repaint every 1000/spectrum_fps (default 30 fps) while data changes 10 times per second: the same frame is drawn up to 3 times |

## 4. Waterfall

| Item | deskHPSDR | hpsdr-rs |
|---|---|---|
| Data | same `pixel_samples` row as the panadapter (same detector and averaging); `width*zoom` values, 1 px per column | **separate pixout 1, raw** (no detector, no averaging), 1024 values per row |
| Rows per second | one per display timer = `fps` (10) | one per WDSP frame (about 10/s) |
| History | `height` rows (pixbuf scrolled with `memmove`) | up to 1000 rows (`WATERFALL_HISTORY`), the whole texture is rebuilt when a new row / palette / limits change |
| Drawing | pixbuf blitted 1:1, no scaling | `ColorImage` 1024 x rows, `TextureOptions::LINEAR`, stretched to the pane with **bilinear filtering** |
| Level -> colour | `p=(s-low)/(high-low)`; below low black; above high (255,255,0); else 7-segment gradient, truncating casts | `t = clamp(...)`, 4 palettes (Fire, Ocean, Classic, Grayscale), piecewise-linear, truncating cast |
| Palette of deskHPSDR | black, blue, cyan, green, yellow, red, magenta, pink-white (stops at 0.222222, 0.333333, 0.444444, 0.555555, 0.777777, 0.888888) | none equal; Ocean (black-blue-cyan-white) is the default |
| Automatic limits | `low = mean(row) + soffset - 5`, `high = low + 55` per row (the mean is over the first `width` samples, not the visible window) | same formula since phase 1 of the Display menu (`low = mean(newest row) + corr - 5`, `high = low + 55`), once per new row |
| Manual limits | -140 / -55 default, automatic on | -140 / -60 default |
| Layout | `percent_pan_wf` 70% panadapter / 30% waterfall (Display menu) | `spectrum_waterfall_ratio` (default 0.4286, draggable divider, 30..80% in the Display menu) |
| 3D waterfall | yes (`display_3d`, terrain 40% of the height, 80 frames at 16 Hz) | not implemented (inactive control) |
| Frequency axis | none on the waterfall when the panadapter is visible (cursor line and triangle) | shares the spectrum axis; marker lines for RTTY/SSTV/RADE |

Consequences: the deskHPSDR waterfall is smoothed in time (log-recursive 250 ms) and has a very different colour scale; the hpsdr-rs
waterfall is raw row to row (more granular), upscaled with bilinear filtering, and uses cooler palettes.

## 5. Candidate causes of the "jagged vs smooth" difference (ranked)

1. **`smooth_spectrum_values`** (5-tap spatial filter): about 3.6x less pixel-to-pixel noise power, more at zoom 5+. Main cause.
2. **`smooth_trace`** (Catmull-Rom): removes the polyline zigzag between bins; adds no detail.
3. **Stroke**: 1.5 px anti-aliased line vs 0.5 px (filled) / 1.0 px cairo line with integer-quantised y.
4. **Pixel count**: 1024 fixed vs `width*zoom` (matters on wide windows and at zoom: with zoom, deskHPSDR's pixels get fewer bins and
   the WDSP interpolating branch makes its line smoother too).
5. **UI redraw at 30 fps** of 10 fps data: no effect on the shape of the line, only on CPU use.

## 6. Decision and what was done

Owner decision: align with deskHPSDR in three points.

1. **Smooth trace** option (new, default OFF = deskHPSDR look): raw row, plain polyline, y rounded to whole pixels, deskHPSDR stroke widths;
   when ON the previous smoothing and spline are used.
2. **Gradient** identical to deskHPSDR (stops, colours, S9 anchoring) for the fill (and the stroke if egui allows it cheaply).
3. **Waterfall**: same detector/averaging as the spectrum on the waterfall pixout, and a new palette "deskHPSDR" (7 segments, below-low black,
   above-high yellow), default only for configs without a stored palette.

See the "Implementation notes" section at the end of this file for what was actually changed and verified.

## 7. Not changed (open, optional)

* Redraw the panadapter only when a new frame arrives (CPU saving).
* deskHPSDR look of the grid (cyan 0.5 px lines, "dBm" labels with black halo) and of the filter passband (gray rectangle).
* Pixel count = widget width * zoom instead of the fixed 1024, and drawing the waterfall 1:1 (no bilinear stretch).
* Forcing the first and last column to -200 dBm, the AGC knee/hang lines, noise-floor text, info bar, worldmap, 3D waterfall.
* The S-meter vs spectrum level discrepancy (see `docs/display-menu.md`) is a different question: it is not caused by anything in this
  comparison (the meter and the pixels get the same correction).

## 8. Implementation notes (what was changed; compiled for the Pi, NOT yet seen on screen)

Files: `src/main.rs`, `src/config.rs`, `src/display_window.rs`, `src/spectrum.rs` (no change to `wdsp_sys/mod.rs`).

**Point 1, Smooth trace** (`spectrum_smooth_trace`, Config `Option<bool>`, default false, persisted; checkbox "Smooth trace" + "!" in the Display
window, General Settings page, last row, left column of the palette row).
* OFF (default, deskHPSDR look): one point per bin, plain polyline, y floored to whole device pixels
  (`floor((high - v) * plot_height / range)`, via `pixels_per_point`), stroke 0.5 px with the fill on and 1.0 px with the fill off. egui fades
  sub-pixel lines, so 0.5 px may look faint: to be judged on the Pi.
* ON: the previous 5-tap filter + Catmull-Rom spline and the old stroke widths (1.0 over the fill, 1.5 without).
* `smoothed_row` is the row that is actually drawn, so peak hold, peak labels and the S-meter diagnostics follow it (raw row when smoothing is off).
* The extra-receiver panel (`render_extra_receiver_ui`) is NOT changed: it still always smooths (no fill/gradient there).
* The deskHPSDR trick of forcing the first/last column to -200 dBm was not copied: the fill mesh already runs every column down to the baseline.

**Point 2, gradient** (`desk_gradient_color`): stops green 0, yellow 0.20*S9, orange 0.55*S9, red 0.80*S9, purple S9 (last colour repeated above), with
S9 = (-73 dBm, or -93 above 30 MHz) + 10 dB normalised as (S9 - low)/(high - low) clamped 0..1 (verified against `rx_panadapter.c`).
* RX gradient fill is now **opaque** (alpha 255) like deskHPSDR; the RX flat fill (gradient off) is white alpha 128 (deskHPSDR `COLOUR_PAN_FILL2`).
* The RX trace stroke uses the same gradient as the fill (`PathStroke::new_uv`, a single path).
* Side effect to judge on screen: the passband shading and the RTTY/SSTV/RADE cursors are painted before the trace, so the opaque fill hides
  them below the trace line (deskHPSDR also paints its fill over its filter rectangle). Peak hold, labels, band edges and the VFO line are drawn
  after the trace and stay visible. If those aids should stay visible, lower the alpha of the fill (a one-argument change).
* TX path unchanged (old gradient, alpha 190, solid stroke).

**Point 3, waterfall.**
* (a) Detector, averaging mode, back-multiplier and number of averages are now applied to pixout 0 **and** pixout 1 (initial setup, re-apply after
  zoom/pan, and the Display menu path, including the NONE -> 50 ms -> target sequence). WDSP keeps these per pixout, so pixout 1 supports them.
  deskHPSDR's waterfall reads the same (averaged) pixout 0, so this matches it. It replaces the old "pixout 1 left raw" comment (added once to
  remove a "memory effect" complaint): averaged rows bring back some temporal memory (250 ms log-recursive).
* (b) New palette **deskHPSDR** (`Palette::DeskHpsdr`, `desk_hpsdr_color`, 7 segments exactly as `waterfall.c`; below the low limit black, above the
  high limit (255,255,0)); the image builder passes it the unclamped fraction, the other palettes still get the clamped one. It is the default
  only for configs without a stored palette (stored choices are untouched). Five palette chips now share the right-column bottom row (about 355 px
  of 394 px: tight, needs a visual check).
* The horizontal bilinear stretch of the waterfall was not changed.

**Unverified:** the on-screen look (0.5 px stroke, gradient colours, hidden passband under the fill, five chips fitting), the waterfall with averaged
rows, and the whole-pixel flooring on the Pi's `pixels_per_point`.

## 9. Follow-up (peak labels and hold use the raw row)

Found afterwards: the 5-tap smoothing averages dB values, which lowered a narrow single-bin carrier by about 34 dB (S9 read as S4 in the Peak
Label); see `docs/display-menu.md` ("RESOLVED"). With "Smooth trace" OFF the label and the S-meter agree. To keep this true when the option is ON,
the Peak Labels and the Peaks & Hold line now always use the raw row (as deskHPSDR does); only the drawn trace follows the option.

## 10. Audit corrections, part 1

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

**Left for part 2 (done, see below):** y clamp, edge columns, flat-mode stroke colour, waterfall 1:1 and per-row colours, extra receivers path, hold line width,
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

## Meter: "Analog (deskHPSDR)" (src/meter_vintage.rs)

Third meter style (tap the meter -> Meter type): deskHPSDR's analog meter (meter.c `analog_meter`) ported drawing for drawing: cream face (vertical gradient + soft glow, rounded corners), dark scale, red alarm arc from S9, dark red needle, the S-word table (HF/VHF), the dBm / Peak-Average / RX n texts, the TX scale in W with SWR and ALC, the Mic | ALC (VOX | ALC) bar graph and the "sedated" needle ballistics (CNTMAX 5, 0.75/0.25). Drawn on meter.c's 250-unit-wide surface and scaled to the meter box; the face is a cached texture; the font is FreeSansBold when installed. The "Analog" and "Digital" styles are unchanged.
