# CW menu (deskHPSDR `cw_menu.c`)

Opened from the **CW** button of the NEW MENU (`src/cw_window.rs`): a compact overlay above the toolbar (rounded corners and shadow like the FILTER / MODE / BAND windows
), Close top left, a label box in front of every control and - / + spin buttons. It replaces Settings -> CW in the kiosk (the tab stays on the desktop).
Every value is stored where the tab stored it, so the saved configuration does not change.

| Page | Controls |
|---|---|
| **Keyer** | Paddle Mode (Straight Key / Iambic Mode A / Iambic Mode B), CW Speed (1-60 WPM), Weight (0-100), Sidetone Level (0-127), Sidetone Freq (100-1000 Hz, step 10), Break-in Delay (0-1000 ms, step 10), CW Pitch (300-1000 Hz, step 10), PC Sidetone |
| **Messages** | the 5 saved CW texts (the on-screen keyboard appears for them) |

deskHPSDR's menu also has "CW handled in Radio", "CW Break-In", "CW Zero Beat Freq. Corr", "Keys reversed" and "Enforce letter spacing". This program has no
state behind them yet (the keyer atomics are mode, speed, weight, sidetone level / frequency and hang time), so they are not shown.

## Host Keyer page and MIDI CW keying (src/cw_keyer.rs)

Third tab, **Host Keyer**: *CW handled in Radio* (default ON: the radio's own keyer keys CW) and *CW Break-In* (default ON). Both are saved per radio.

MIDI actions (Settings -> MIDI, key bindings; press AND release are used whatever "momentary" says), handled on the MIDI thread:

| Action | Source | Needs "CW handled in Radio" OFF |
|---|---|---|
| CW Left (paddle) / CW Right (paddle) | piHPSDR CW_LEFT / CW_RIGHT | yes: iambic A/B or straight/bug, per Paddle Mode |
| CW Straight Key | deskHPSDR CW_STRAIGHT_KEY | yes |
| PTT (CW Keyer) + CW Key (Keyer) | piHPSDR CW_KEYER_PTT / CW_KEYER_KEYDOWN | no: an external keyer with its own PTT; while PTT is held the radio's own keyer is disarmed |

Break-in: the first key event raises MOX, waits for the TX thread, and MOX drops after the *Break-in Delay* (Keyer page) with no further element. Key timing is carried as `(state, wait)` events in samples (piHPSDR's event ring) and played back by the TX thread in 2.7 ms slices; the PC sidetone follows the same key state directly. Measured on the Pi (hpsdrsim, MIDI loopback): MIDI note -> keyer 0.0 ms, key -> sidetone audible 14-20 ms, key -> first IQ on the wire 2-5 ms (plus the HL2's own 40 ms TX latency).

Not ported: Keys reversed, Enforce letter spacing, CAT CW abort on key hit, the radio's own paddle contacts as a host-keyer source.
