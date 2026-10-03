# Why the UI redrew at 60 fps whatever `spectrum_fps` said

**Symptom.** On the Raspberry Pi 5 kiosk the `hpsdr-rs` UI thread used about 85 %
of a core, although Settings -> Spectrum FPS was 15-30. The status bar's
`UI: NN fps` counter (added for this) read 60.

**Root cause.** Every frame, the Connected view called
`ctx.send_viewport_cmd(ViewportCommand::Title(..))` with the same window
title. egui treats any viewport command as a request for an *immediate*
repaint, so the UI redrew back to back at the display rate and the
`request_repaint_after(1000 / fps)` throttle never had any effect.

**Fix** (`src/main.rs`): the title is sent only when it changes.

Result on the Pi: 60 fps -> about 17 fps (the Radioberry config has no
`spectrum_fps`, so it follows the default of 30 minus frame time), UI thread
about 85 % -> about 20 % of a core. Desktop builds had the same busy loop.

## How it was found

1. Status bar `UI: NN fps (MM passes) X.X ms` (hover shows waterfall rebuild
   cost). 30 fps / 60 passes first looked like egui's two-pass layout, but
   limiting `max_passes` to 1 only turned it into 60 fps / 60 passes, so the
   cause was a continuous repaint request, not a second pass.
2. `ctx.repaint_causes()` lists the file and line of every call that asked for
   the last repaint. Dumping it showed `send_viewport_cmd` among the callers.

**Rule for contributors:** never call `send_viewport_cmd*` (title, size,
position, focus...) every frame; send it when the value changes.

## Also in this change

* **Juice Console** is now the **Juice** tab of Settings (shown only when a
  juice process was started from Discover): status, Stop, Restart, Run as
  Administrator (Windows) and the live log. It no longer has a toolbar button
  or its own window, so the main layout is untouched.
