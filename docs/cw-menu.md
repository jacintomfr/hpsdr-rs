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
