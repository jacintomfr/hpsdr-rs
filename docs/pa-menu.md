# PA menu (Settings -> PA) and drive-level linearization

Full-screen kiosk window `src/pa_window.rs`, modelled on deskHPSDR's *PA Calibration* menu
(`deskhpsdr/src/pa_menu.c`, `band.c`, `radio.c`). The old *PA Calibration* tab is kept (it is meant to be
retired slowly); both edit the **same stored values**, so they stay in sync.

## Status

| Part | State |
|---|---|
| Header: MAX Power, Transmit out of band, Save, Import, CLOSE | done, layout checked on the Pi by the owner |
| Calibrate page (per-band calibration spins) | done, layout checked on the Pi |
| Linearization page (9 correction spins per band) | done, layout checked on the Pi |
| Measure assistant (TUNE + external wattmeter) | **written, never run on hardware** (no wattmeter connected yet) |
| Watt Meter Calibrate page (deskHPSDR `pa_trim`) | **not implemented** (later phase) |

## Header and Calibrate page

* **MAX Power** combo: 1, 5, 10, 15, 20, 25, 30, 50, 75, 100, 125, 200, 500 W, 1 KW. It writes the existing
  `max_tx_power_watts` (meter full scale and TX power slider range in this app). deskHPSDR only uses its MAX Power
  for the watt-meter scale; here it also limits the slider, because both are the same variable. The drive maths is
  not touched. A stored value that is not in the list is shown as an extra `<n>W` entry.
* **Transmit out of band** (`allow_out_of_band_tx`, `Arc<AtomicBool>` read live by every PTT path). It lives only
  here now (it was moved to SDR Device for a short time and moved back). When on, the *Gen* row appears.
* **Calibrate**: one spin (− value +, tap/hold, never drag) per band, **38.8 .. 70.0 in 0.1 steps**, the range of
  deskHPSDR (`band.c` clamps loaded values to 38.8..70.0). Stored and loaded values are clamped. HL2 (not
  Radioberry) gets a "!" with the deskHPSDR instructions (all bands 38.8, MAX Power 5 W, PA enable on in
  SDR Device).
* Bands: HL2 shows 136kHz, 472kHz, 160 .. 10 (two columns of 6); other boards go up to 6 m (and 8 m as in
  deskHPSDR); *Gen* only with out-of-band on; XVTR slots that have a title. **Only 160 m .. 6 m are read by the
  drive code** (`resolved_pa_gain_db`); 136kHz, 472kHz, 8m, Gen and XVTR values are stored, saved and imported but
  have no effect on the drive (those bands would still use 38.8).

## Save / Import

deskHPSDR `.props` format: `pa_calibration_<radio name, non-alphanumerics -> _>.props` in the settings directory,
lines `band.<n>.pa_calibration=<float>` using deskHPSDR's band index (band136=0, band472, band160, band80, band60,
band40, band30, band20, band17, band15, band12, band10, band8, band6, band70, band144, band220, band430, band902,
band1240, band2300, bandAIR, bandWWV, bandGen=23; XVTR slot *i* = 24+*i*). No `device_id` is written (this app has
no deskHPSDR device numbering) and no radio comparison is done on import; unknown keys are ignored, values are
clamped. Import is an in-app popup file list (`*.props` of the settings directory).

## Drive maths (checked, unchanged)

`drive_byte_for_watts` is numerically identical to deskHPSDR `calcLevel`:
`target_dbm = 10*log10(watts*1000) - gain`, `volts = min(sqrt(10^(dBm/10)*0.05)/0.8, 1)`, `/0.98`, clamp,
`int(x*255)`. The only difference is a 0.01 W floor to avoid `log(0)`. `hl2_drive_level_and_scale` equals
`radio_calc_drive_level` for the HL2 (all 15 thresholds, attenuator steps and scale constants). The drive gain is
`pa_gain_db = calibration(band) - linearization_correction(band, watts)` (applied every frame, `main.rs`).

## Linearization page

* 9 points per band at **10% .. 90% of MAX Power** (0 dB at 0% and 100% by definition), correction in dB,
  **-20 .. +20, step 0.1**, interpolated by `interpolate_drive_adjust`. Same storage as the old tab:
  `pa_drive_adjust[band]` (`[f32; 9]`). Reset removes the band's entry (= zeros); *Reset all* asks first.
* Band chips: the bands the drive code reads (160 m .. 6 m); default = current TX band.
* Target curve: output power proportional to the requested watts (the slider is in watts, 0..5 W on the HL2).

### Sign of the correction (UNVERIFIED on a radio)

`gain = cal - adjust` and `drive_byte_for_watts` uses `target_dbm = P_dBm - gain`. A **positive** correction lowers
the assumed PA gain, i.e. **raises the drive**. The old tab's comment says "positive reduces output", which by this
derivation is wrong. **To verify** (dummy load + wattmeter): set +1.0 dB on one point, transmit at that power and
check that the output goes up. If it goes down the sign is swapped: change it in one place
(`pa_window.rs`, the `new_adj` line of the Measure assistant and the description above).

## Measure assistant (written, not run on hardware)

Needs a **dummy load and an external wattmeter**. Flow: *Start* -> confirmation (dummy load) -> *Key* keys TUNE at
the point's full power -> the operator types the wattmeter reading -> *Next* stores the correction
`new_adjust = applied + 10*log10(target/measured)` (clamped to ±20, rounded to 0.1, written into that point
directly) and releases TX; the next point always needs an explicit *Key*. The app's own forward-power reading is
shown as a display-only hint (the HL2 PA30 detector offsets are applied).

* **Whole watts only.** The TX power setting (`tx_power_watts`) is a whole-number `u32`. A point that is not a whole
  number of watts cannot be commanded exactly (e.g. MAX 5 W: 10% = 0.5 W would be commanded as 1 W). The assistant
  therefore **skips those points** (`point_exact`, `next_exact_point`) and they are set manually with the spins. At
  MAX 5 W (HL2) the measurable points are 20% (1 W), 40% (2 W), 60% (3 W) and 80% (4 W). If a finer assistant is
  wanted later, `tx_power_watts` would have to accept tenths of a watt (a larger change that touches the TX path).
* The 90% point of a 5 W radio (4.5 W) is manual; the 100% point is fixed at 0 dB.
* **Safety (all in `pa_window.rs`)**: refuses to start if TX is not enabled / no `tx_handle`, the hardware TX
  inhibit is asserted, the radio is already transmitting (MOX, TUNE, two-tone, CW text), the frequency is not
  allowed (`tx_frequency_allowed`), or the TX band is not the selected band. It uses its own TUNE sequence (never
  touches `tune_power_percent`) and caps the power at `max_tx_power_watts`. TX is released (`unkey`, power restored
  to the saved value) on Cancel, on leaving the page/tab, on CLOSE/Esc, when the PA window is closed or not drawn
  for 1.5 s, when TX becomes disabled/inhibited, when TUNE/MOX is dropped elsewhere, when `tx_power_watts` is
  changed from outside (this is how the SWR protection, which forces 10 W, is detected), when the frequency leaves
  the band, when SWR >= `max_swr` with SWR protection on, and after a hard **20 s per point** timeout.
  `measure_tick` runs every frame (`main.rs`, after `radio_inputs_tick`); `pa_measure` lives in `ConnectedState`,
  so a disconnect drops it.

## Open items

1. Run the assistant with a dummy load and a wattmeter and **verify the sign** (see above).
2. Decide whether `tx_power_watts` should accept tenths of a watt (finer low-power points).
3. Watt Meter Calibrate page (deskHPSDR `pa_trim[0..10]`, 10-point interpolation of the displayed power; note that
   deskHPSDR overwrites these values with a straight line on an HL2 with PA enabled, `reassign_pa_trim`).
4. Optional fallback in `resolved_pa_gain_db` so that Gen / XVTR / 136kHz / 472kHz / 8m use their stored values.
5. Fix the old tab's "positive reduces output" comment once the sign is verified.
6. Retire the old *PA Calibration* tab when the new window replaces it.

## Where things are

`src/pa_window.rs` (window, Save/Import, Linearization page, Measure assistant); `src/main.rs`: `mod pa_window`,
`SettingsTab::Pa` (tab strip, dispatch, redirect), `pa_window_open`, `pa_measure`, the render call after the TX Menu
window, `pa_window::measure_tick(connected)`, the clamp of `cfg.pa_calibration` on load (38.8..70.0) and the widened
slider range of the old tab; the out-of-band checkbox was removed from `render_sdr_device`.
