[← Direct connection](21-direct-connection.md) | [Index](README.md)

# Control look, the VFO window and the main-window controls

This page describes how the main window's controls look and behave: the
common button style, the **VFO window** (direct frequency entry), RIT/XIT
gestures, the switches next to the gain sliders, and where things sit on the
desktop layout and on the 1024x600 kiosk layout.

## Button style

Every clickable control in the main window shares one look:

| Look | Meaning |
|---|---|
| Rounded corners, thin outline, **grey** fill | off / idle |
| Rounded corners, thin outline, **orange** fill | on / active (a mode, a band, NR on, RIT on, ...) |
| **Yellow** fill | **SETTINGS...** (it stays easy to find) |
| **Light blue** fill | **DIGITAL...** (kiosk label is upper case, like SETTINGS/STOP) |
| **Red** fill, white text | **STOP**, and **MOX / TUNE / TWO TONE while transmitting** -- the one exception to the grey/orange rule, because the on-air state must be unmistakable |
| Brief **orange flash** | **A>B**, **B>A**, **A<>B** -- they have no on/off state, so a click just flashes |

Slider names (Audio gain, Mic gain, TX Power, ...) sit in a rounded box with
a thin outline and white text. The grey boxes showing a slider's value are
all the same size, white, and right-aligned, so the layout does not jump
when a number goes from one digit to three. Buttons no longer "grow" while
the mouse is over them.

## The VFO window

The **VFO** button, next to **CTUN**, opens a small window modelled on
piHPSDR's VFO menu (right-clicking either VFO box opens the same window):

* **VFO A / VFO B** at the top choose which VFO you are editing (the active
  one is orange). **Close** closes the window.
* The display shows what you have typed. The keypad has **1-9, `.`, 0, BS**,
  and the unit keys **Hz / kHz / MHz** apply the number with that unit
  (type `14.225`, press **MHz**). **Enter** on the PC keyboard is the same as
  **Hz**. **Clear** empties the entry.
* Like piHPSDR, a result below **10 kHz** is ignored, so pressing a unit key
  on an empty entry never sets the frequency to zero.
* **RIT step** (1, 10 or 100 Hz) is the amount one scroll notch changes the
  RIT/XIT offset. **VFO step** is the tuning step (the former "Step" dropdown
  next to Record moved here).

## RIT and XIT

RIT and XIT sit to the right of the band buttons (XIT only appears when
transmit is armed), in fixed-width boxes that show the offset
(e.g. **RIT +250**).

* **Short click**: switches it on/off.
* **Long press** (hold about 0.6 s): clears the offset to zero. There is no
  separate **Clear** button any more.
* **Scroll** over it: changes the offset by the **RIT step** (hold **Shift**
  for 10 Hz).

## Switches beside the gain sliders

* **RxPGA** (Hermes-Lite 2) switches the ADC auto gain. Off: orange box with
  black text. On: inverted (dark box, orange text and outline), so it reads
  as a switch and not as a slider name.
* **AGC Gain** doubles as the **AGC Auto** switch with the same look. The
  separate "AGC Auto" tick box was removed from Settings -> RX; only its
  **AGC Auto offset** remains there.
* **NB** and **NR** (cycle through their algorithms) sit between the Mic/TCI
  and Filter-width value boxes and the **REC** / **PLAY** buttons, one above
  the other and the same width. **SNB**, **ANF** and **BIN** follow the mode
  buttons.
* **REC** / **PLAY** have the normal font and show a real hover shade and a
  tooltip even when disabled (it says why).

## RADE status

While RADE is active but the Digital window is hidden, a compact status
appears beside the AGC mode button: **SYNC** (green when locked, yellow when
marginal, orange "no sync" when not), then SNR, offset and the last callsign
received, each in its own box. See also
[RADE end-of-over callsign](../rade-end-of-over-callsign.md).

## Desktop and kiosk layouts

The look and the logic are shared. Only the **layout** differs on the fixed
1024x600 panel (`HPSDR_LCD_1024X600=1`, the `hpsdr-rs-kiosk` launcher):

* the S/power meter is drawn 1.7x larger;
* the UI scale presets (Settings -> Screen) include 160%, which is the one
  that fits the panel best;
* an extra margin is reserved below the waterfall so the status line is not
  cut off at larger scales;
* secondary windows (Settings, Digital, VFO) hand the focus back when the
  full-screen main window takes it, which avoids a hidden window slowing the
  whole application.

## Settings are saved and restored

Anything you change in the main window or in Settings is saved per radio and
restored the next time you connect. The configuration file is written
atomically, and a copy of the file as it was at start-up is kept next to it
as `config-....json.bak`; if the main file is ever missing or corrupt, the
copy is loaded instead of resetting everything.
