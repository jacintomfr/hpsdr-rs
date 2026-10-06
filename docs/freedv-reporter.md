# FreeDV Reporter

The RADE panel has a **FreeDV Reporter** checkbox (default **off**, saved in the config as
`freedv_reporter_enabled`). The "!" button next to it explains it in the app.

When enabled, and both *Your Callsign* and *Your Locator* are set in SDR Device, the app
connects to <https://qso.freedv.org> and appears in the FreeDV Reporter station list.

## What is sent

- your callsign and grid locator, the app name/version (`hpsdr-rs <version>`) and the OS name;
- the dial frequency (only when it changes);
- the RADE transmit state (mode `RADEV1`, `transmitting: true` only while a RADE over is really
  being sent);
- every callsign decoded by RADE, once each, with its SNR.

Nothing is sent while the checkbox is off or the radio is disconnected.

## How it works

`src/freedv_reporter.rs` is a minimal Socket.IO v4 / Engine.IO v4 client over a `wss://` WebSocket
(`tungstenite`, TLS by `rustls` with the `ring` provider and the webpki root store, so no
aws-lc-rs/cmake is needed). One worker thread; every call from the UI or audio thread is a channel
push and never blocks. Connection errors are only logged, and the worker reconnects with a
back-off (2 s up to 60 s). The UI calls `freedv_reporter::sync()` every frame; the RADE decoder
calls `rx_report()`.

## How it was tested

A standalone prototype of the same client connected to qso.freedv.org and showed up in the station
list. The repository's unit tests never contact the server.
