# RADE: why the end-of-over callsign was almost never received

**Symptom.** Transmitting RADE from hpsdr-rs (Windows, Metis) and receiving it
on another hpsdr-rs (Raspberry Pi, Hermes-Lite 2): speech decoded fine, SYNC
stayed green at 14 dB or better, but the callsign carried in the End-of-Over
(EOO) frame was received only now and then. Callsigns from other stations
decoded normally, so the receiver was not at fault.

**Root cause.** The transmit path converts the modem's 8 kHz output to the
audio rate with a resampler that only processes **whole 1024-sample input
chunks** and keeps the remainder pending until more input arrives. When an
over ended, the EOO frame (1152 modem samples, 144 ms) was pushed in and then
nothing more, so up to 128 ms at the **end** of the burst never left the
resampler. The receiver finds an EOO frame by correlating two pilot symbols;
the second one is in the final ~24 ms. With that part missing, the
end-of-over correlation was about half of a normal frame's and below the
detection threshold, so the frame was never flagged. Whether it worked
depended on how the stream length fell relative to the 1024-sample chunk,
which is why it succeeded "sometimes".

**Fix** (`src/rade/worker.rs`): after the EOO frame, push one chunk of silence
through the resampler so the real tail is released. A unit test
(`src/rade/resample.rs`) documents the held-back tail.

## Other defects found on the way

* **EOO dropped by a full ring.** The EOO was pushed into the transmit ring
  with a non-blocking write while the ring still held the last second of
  speech, so 5000-6000 of its ~6900 samples were discarded. The EOO (and the
  last speech block) now wait for room instead of dropping.
* **One second of extra transmit latency.** Microphone audio keeps being
  pushed to the worker while idle (up to the ring size). Keying used to
  encode all of that stale audio at once, adding a permanent second of delay
  and leaving the ring full. Keying now discards it.
* **PTT hold.** The real PTT is released 500 ms after the transmit queue
  drains (was 300 ms), with a 2 s safety timeout.

## What was ruled out

* **Callsign encoding / decoding:** an in-process loopback test
  (`eoo_callsign_survives_the_modem_loopback`) encodes a callsign, runs it
  through `rade_tx_eoo` and `rade_rx`, and decodes it; it passes, with and
  without added noise.
* **The transmit hard clip:** the burst peaks at exactly 1.0 and decodes even
  when clipped at 0.5 (`eoo_decode_through_the_tx_hard_clip`).
* **Receiver gain:** replaying captured audio at gains from 0.1x to 4x made
  no difference.

## How it was found (for the next time)

1. Per-over log lines on both ends: `End-of-Over emitted ... dropped N` and
   `PTT released ...` on the transmitter; `over ended: sync lost ...,
   End-of-Over frame flagged ...` on the receiver (`HPSDR_RS_LOG_FILE=1`
   writes the log to the settings folder).
2. Dumping the last seconds of receive audio when an over ends, then
   analysing it offline with the ignored tests in `src/rade/mod.rs`
   (`analyze_tails`, `verbose_tail`; set `RADE_TAILS_DIR`). Correlating the
   dump with the locally generated EOO showed the burst was on the air
   (correlation 0.77-0.84); the library's own verbose output showed the
   end-of-over pilot correlation at about half of its normal value. The dump
   code itself has since been removed.
