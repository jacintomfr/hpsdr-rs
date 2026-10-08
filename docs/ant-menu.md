# Ant window (src/ant_window.rs)

deskHPSDR ant_menu.c in the kiosk (NEW MENU -> Ant, or Settings -> Antenna): a window that stops above the toolbar (the toolbar stays usable), square corners, no shadow, like the OC Output and PA windows.

* **HF** page: the reachable bands and Gen, **XVTR** page (only when a transverter is configured): the named transverter bands. Two band groups side by side (Band | RX Ant | TX Ant, twice), filled left to right like the original.
* RX Ant: Ant1, Ant2, Ant3, Ext1, Ext2, Xvtr; TX Ant: Ant1-3 (Ext / Xvtr are RX-only). Same `antenna_settings` values as the old Settings page (by band name), nothing changes in the saved configuration.
* "ANAN 100/200 new PA board" in the header for Hermes / Angelia / Orion boards, with its own "!".
* The description that headed the old page is behind the round "!".
* The drop-down lists are tall enough for all six RX ports (`choice_combo_h` now sizes its popup to the list, up to 380 px).
