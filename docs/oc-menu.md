# OC Output window (src/oc_window.rs)

deskHPSDR oc_menu.c in the kiosk: a window that stops above the toolbar (the toolbar stays usable), square corners, no shadow, like the PA window. Opened from NEW MENU -> OC Output (or Settings -> Open Collector in the kiosk).

* Band rows (reachable bands, Gen, configured XVTRs) with the 7 Rx and 7 Tx outputs as big X boxes; the rows scroll with the touch bar. Same `oc_settings` values as before, nothing changes in the saved configuration.
* **Tune column** (ORed into Tx while TUNE is on) stays in view.
* **Sending now** (header): the 7 outputs currently sent to the radio (Rx or Tx value), like deskHPSDR's OC monitor.
* **Full Tune (ms) / Memory Tune (ms)** and **Arm Full / Arm Memory Tune** (deskHPSDR `OCfull_tune_time`, `OCmemory_tune_time`, `full_tune`, `memory_tune`): for an antenna tuner driven by the OC lines. A TUNE armed as Full (or Memory) starts a window of that many ms in which the Tune outputs are on. As in deskHPSDR, with Memory Tune not 0 the Tune outputs are only on inside that window; with both at 0 (default) they stay on for the whole TUNE, as before. Saved per radio. The arming is also available as the MIDI / toolbar actions "Tune Full" and "Tune Memory" (not saved).
* The long description that headed the old page is behind the round "!" (and the Tune help behind the one next to "Tune").

Not ported: deskHPSDR links 8m with 6m (this program has no 8m row on a Hermes Lite).
