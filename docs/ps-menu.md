# PureSignal menu (deskHPSDR `ps_menu.c`)

Opened from the **PS** button of the NEW MENU (`src/ps_window.rs`): a compact overlay above the toolbar (same style as the AGC and
DSP menus), Close top left. It needs a transmitter (the button does nothing without one). The old *Settings -> PureSignal* tab is kept.

## Layout (deskHPSDR order)

| Row | Controls |
|---|---|
| 1 | **Enable PS**, **Two Tone** (same function as the toolbar button), **Auto Attenuate**, **OFF** (reset), **Restart** |
| 2 | **OneShot**, **PS Stability** (Strict 0.06 / Medium 0.04 / Relaxed 0.02 -> `SetPSDeadlockMinFrac`) |
| 3 | **Feedback Lvl** (>181 blue, >128 green, >90 yellow, else red), **Correcting** (yes green / no red), "[Optimal feedback level between 140..165]" |
| 4-5 | **feedbk** (info[4]), **cor.cnt** (info[5]), **sln.chk** (info[6]), **status** (info[15] as text) |
| 6 | **GetPk** (`GetPSMaxTX`), **SetPk** (hardware peak, 0.01..1.01), **TX ATT** (HL2 -29..+31, others 0..31; a read-only box while Auto Attenuate is on) |

Saved: OneShot and PS Stability (`TxUiExtra`), SetPk (`ps_hw_peak`), TX ATT (`ps_tx_attenuation`), Enable PS (`puresignal_enabled`).
Not in this version (need engine support that does not exist yet): Noise generator, MON (feedback spectrum), PS FeedBk ANT, AmpView.

## Auto Attenuate (ps_menu.c `ps_calibration_timer`)
`ps_window::auto_tick`, every 100 ms while PS is enabled, Two Tone is transmitting and Auto Attenuate is on: when the feedback level has a new
value (or after more than 10 ticks) and is outside 150..155 the attenuation moves by `lround(20*log10(level/152.293))` (+15 above 275,
-15 below 25), limited to the radio's range, and the calibration restarts (`ps_calibrate`). Restart with Two Tone + Auto sets the attenuation to 0.

## Engine fixes made together with this menu (all Protocol 1, mainly Hermes Lite 2; see the commit)
1. The feedback DDCs are tuned to the **TX** frequency (piHPSDR `channel_freq`), not the RX dial frequency.
2. `SetPSFeedbackRate` follows the session RX rate and the number of feedback pairs per TX chunk scales with it.
3. HL2: the TX-DAC feedback is multiplied by `0.9999 / drive_scale` (piHPSDR `drive_iscal`).
4. HL2: the TX-time LNA register (0x1C C3) has one value (commands 6 and 11 used to alternate between two): PureSignal feedback gain
   `31 - attenuation` while PS is on, otherwise as before. Command 4 C4 follows the same rule while transmitting.
5. Switching PS off resets the WDSP engine (`SetPSControl` reset, `SetPSMox(0)`, 7 empty `pscc` blocks), like `tx_ps_onoff(0)`.
Also: TUNE / Two Tone power with "Tune Drive = TX drive" (TX menu) uses the TX power itself (it was ignored, and the Tune % was an integer
division: 6 W x 20 % = 1 W).
