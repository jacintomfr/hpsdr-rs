# BandStack (src/bandstack.rs)

deskHPSDR's band stacks. Every band keeps a short list of frequency + mode entries (band.c's defaults: e.g. 20m 14.010 CWU, 14.150 / 14.230 / 14.336 USB; Gen: three AM broadcast frequencies) and which one is current; saved per radio.

* **NEW MENU -> BandStack**: a compact overlay at the bottom with rounded corners (like the Band / Mode / Filter windows): the entries of the current band as buttons, 4 per row, the current one lit. A tap tunes to that entry (frequency and mode) and makes it current.
* **The button of the band you are already on** (band buttons, the Band window, the MIDI band actions) steps to the next entry, wrapping round (deskHPSDR vfo_band_changed).
* The current entry follows the dial frequency and mode all the time (deskHPSDR copies the VFO into it when it leaves the entry). The first visit to a band opens its current stack entry instead of the old fixed default.
* Not ported: per-entry filter, FM deviation and CTCSS (this program keeps the filter width per mode).
