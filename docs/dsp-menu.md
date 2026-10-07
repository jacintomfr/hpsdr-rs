# DSP menu (deskHPSDR `fft_menu.c`)

Opened from the **DSP** button of the NEW MENU (`src/dsp_window.rs`), full screen, one column per channel (RX1, TX).
The reference (deskHPSDR `fft_menu.c`) has, per channel: WDSP FIR filter type, FIR size NC, Binaural (RX only), RX image
measure, image offset, RX IQ gain / phase, Reset, Auto RX IQ and the IQ status.

## Phase 1 (done): FIR type, FIR size, Binaural

| Row | RX1 | TX |
|-----|-----|----|
| WDSP FIR Filter Type | Linear Phase / Low Latency -> `RXASetMP` | same -> `TXASetMP`; Low Latency switches **Auto CESSB** off (deskHPSDR `tx_set_latency`), and switching CESSB on in the TX menu switches Low Latency off |
| WDSP FIR Filter NC | 2048 / 4096 / 8192 / 16384 -> `RXASetNC` | same -> `TXASetNC` |
| Binaural | existing BIN (`set_binaural`) | - |

* Stored in `RxExtra` (`fir_low_latency`, `fir_nc`) and `TxExtra` (same names), which are already saved with the configuration.
* The DSP threads send the values to WDSP only when they change (`last_fir` in `spectrum.rs` and `tx.rs`); the channels are opened
  with linear phase / 2048, which is also the default, so nothing is sent at start-up.
* CESSB only runs when the low-latency filter is off (deskHPSDR `transmitter.c`).
* RX2 (extra receivers) is not in the window yet: their DSP parameters are saved by a separate mechanism.

## Phase 2 (done): RX image measure + manual IQ correction (RX1)
* **RX Image Measure** (not saved, like deskHPSDR): the panadapter row is read at +offset and -offset from the centre of the display (maximum of +-2 bins);
  signal minus mirror is shown as "IRR x dB" at the bottom left of the spectrum. Not measured while transmitting, and not when the offset falls
  outside the visible span.
* **Image Offset Hz** 100..10000 (step 10), **RX IQ Gain** -5..+5 dB and **RX IQ Phase** -20..+20 deg (step 0.01), **Reset**: stored in `RxExtra`
  (`image_measure_hz`, `iq_gain_db`, `iq_phase_deg`, saved with the configuration).
* The correction is applied in place to each IQ chunk before the analyzer and the demodulator (`spectrum.rs`, run loop): I stays, Q = (Q*g + I*sin(phi)) / cos(phi), g = 10^(dB/20).
  The raw IQ tap for TCI is taken before it.

## Phase 3 (done): Auto RX IQ (IQ page, header row)
The deskHPSDR `rx_iq_auto_thread` algorithm, run as a state machine one step per UI frame (`iq_auto_tick` in `dsp_window.rs`):
* Needs RX Image Measure on and a calibration signal (signal level above -100 dB), otherwise the status says "No calibration signal".
* Candidates: gain 0 / phase 0 first, then sweeps around the best value so far: gain +-0.05 dB step 0.005, phase +-0.50 deg step 0.05, gain
  +-0.01 step 0.001, phase +-0.10 step 0.01. Each candidate: set, wait 250 ms, then 5 IRR samples 100 ms apart, averaged.
* The best |IRR| wins; it is kept only if it is at least 0.5 dB better than at the start ("Complete (x.x dB)"), otherwise the old correction
  comes back ("No improvement"). Longest run 10 s, as in deskHPSDR (`AUTO_DEADLINE`): the full sweep list needs about 64 s, so a run normally ends by the time limit with the best value found so far.
* Reset (or switching the measurement off) cancels a run and restores the old values; the status texts are Idle, Auto calibrating..., Complete, No improvement, No calibration signal, Cancelled.
