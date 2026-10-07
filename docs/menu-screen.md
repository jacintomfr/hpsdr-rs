# Menu screen (NEW MENU)

A full-screen window (`src/menu_window.rs`) that groups every menu like deskHPSDR's main menu screen. Opened by the
MIDI / toolbar action **NEW MENU** (appended at the end of the action lists, so saved MIDI/toolbar assignments keep
their meaning). The top-right MENU button of the main window is unchanged (it still opens Settings).

## Layout (1024x600)

Title `hpsdr-rs - Menu [<settings dir>]` (`config::settings_dir()`). Header row: `Restart Protocol` and `Iconify`
(greyed, each with a "!" help) on the left, yellow CLOSE at the top right (same placement as the other full-screen
windows). Escape closes. Then 6 columns x 6 rows of equal buttons (48 px high, 16 px font, rounded, thin grey outline;
12 px gaps) and `About` centred under the grid (two cells wide).

Height budget: 4 margin + 22 title + 54 header + 12 gap + (6 x 48 + 5 x 12 = 348) grid + 12 gap + 48 About = 500 px
of the 590 px window; side margins 24 px.

| R | C1 | C2 | C3 | C4 | C5 | C6 |
|---|----|----|----|----|----|----|
| 1 | SDR Device | VFO | RX | TX | DSP (grey) | Toolbar |
| 2 | Screen | Band | RX Filter | PA | WDSP EQ | CAT/TCI |
| 3 | Display | BandStack (grey) | Noise | VOX | Ant | MIDI |
| 4 | Meter | Mode | AGC | PS | OC Output | - |
| 5 | XVTR | Memory (grey) | - | RADE | Extras (grey) | - |
| 6 | Discovery (grey) | - | - | CW | - | - |

## Targets (same code paths as today)

| Button | Opens |
|---|---|
| SDR Device / RX / TX / PA / Display / Noise / WDSP EQ | `sdr_window_open` / `rx_window_open` / `tx_window_open` (only with a TX handle, else greyed with "!") / `pa_window_open` / `display_window_open` / `noise_window_open` / `eq_window_open` |
| VFO | the VFO keypad window (`frequency_entry`, VFO A), as a tap on the VFO A frequency |
| VOX | `vox_window_open` (VoxMenu action) |
| Band / Mode / RX Filter / AGC | the compact popups (`band_/mode_/filter_/agc_window_open`); opening one closes the others |
| Meter | the kiosk meter options window (`meter_window_open`, same as tapping the meter) |
| RADE | `toggle_rade_direct` (same as the RADE toolbar function: it toggles RADE on/off) |
| Screen, Toolbar, Ant, OC Output, PS, XVTR, MIDI, CW, About | Settings window on the tab Screen, Toolbar, Antenna, OpenCollector, PureSignal, Xvtr, Midi, Cw, About |
| CAT/TCI | Settings window, Network tab (rigctl, CAT and TCI servers) |

Every button that opens something closes the Menu first and closes every other overlay (`close_overlays`), so two
full-screen windows are never open together. NEW MENU does the same when it opens the Menu (and closes Settings).

## Greyed buttons (visible, inert, "!" help)

DSP, BandStack, Memory, Extras ("Not available in this version"); Restart Protocol, Iconify ("Not implemented");
Discovery: the only way back to discovery today is EXIT, which disconnects the radio, so it is not offered here.

## Settings window changes (kiosk only)

The tabs SDR Device, Noise, TX Menu, PA, Display, RX Menu and Equalizer are removed from the tab strip when
`lcd_kiosk_mode()`. Enum variants, dispatch arms and the desktop behaviour are untouched; the old PA Calibration and
Spectrum tabs remain. Nothing else navigates the kiosk Settings window to a removed tab.

## NEW MENU action

`MidiAction::NewMenu` ("NEW MENU", toolbar short label `NEW MENU`); the toolbar box is lit while the Menu is open.
Pressing it again closes the Menu.

## Verified / unverified

Verified: cross build (WSL) compiles without errors. Unverified: on-screen look and touch behaviour on the Pi (not
captured), the "!" overlay position inside greyed cells, and the VFO / RADE / Settings-tab openings at runtime.

## Return to the Menu (owner request)

A window opened from the Menu (any button: full-screen window, compact popup, VFO keypad, Settings tab, meter window) brings the Menu back when it
is closed, instead of leaving the main screen. Implementation: `menu_return` (ConnectedState) is set when the Menu opens a target
(`menu_window::open`); `menu_window::return_tick` runs every frame and, while the flag is set and no window is open any more
(`any_overlay_open`), re-opens the Menu and clears the flag. This needs no change in the individual windows (their own CLOSE keeps working) and also
covers targets that open nothing (TX without a transmitter, the RADE toggle). The flag is cleared when the Menu itself is closed or toggled
from MIDI/toolbar ("NEW MENU"). Windows opened by other means (the MENU button, other MIDI actions) do not return to the Menu.

## Toolbar moved into the Menu (owner request)

The Toolbar editor (8 layers x 8 boxes, FNC(1)..FNC(8)) is now its own full-screen window, `src/toolbar_window.rs`, opened by the **Toolbar** button of the
Menu (`Target::Toolbar`, flag `toolbar_window_open`). It reuses `render_toolbar_config` and the function chooser unchanged; title "hpsdr-rs - Toolbar",
the yellow CLOSE at the top right, Escape closes (not while the function chooser is open), and CLOSE returns to the Menu (`return_tick` /
`any_overlay_open` include the window). In the kiosk the **Settings -> Toolbar tab is removed** from the tab strip (the arm stays for the desktop code
path). The FNC's list closes when this window opens (`fnc_window::covered`). Checked on the Pi: Menu -> Toolbar, CLOSE back to the Menu, and the
Settings strip without the Toolbar tab.

## Restart Protocol, Iconify, Discovery (working)

* **Restart Protocol**: stops the radio session and reconnects to the same radio (the flow the in-app firmware update uses), so the protocol starts from scratch with the saved settings.
* **Iconify**: minimises the application window (egui `ViewportCommand::Minimized`); restore it from the desktop taskbar.
* **Discovery**: the same as Stop: saves the settings, disconnects and returns to the device list.

None of the three comes back to the Menu afterwards. The "!" help circles of the inert buttons were removed; the remaining inert buttons (DSP, BandStack, Memory, Extras) are simply greyed. All Menu buttons use the same 22 px font.
