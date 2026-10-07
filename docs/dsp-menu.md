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

## Phase 2 (planned): RX image measure + manual IQ correction
Image measure (signal at +offset, mirror at -offset in the panadapter -> "IRR x dB"), offset 100..10000 Hz, RX IQ gain -5..+5 dB,
phase -20..+20 deg, Reset. The correction is applied to the IQ samples before WDSP: Q' = (Q*g + I*sin(phi)) / cos(phi).

## Phase 3 (planned): Auto RX IQ
One-shot search of gain / phase that maximises the IRR (needs a calibration signal), with status text.
