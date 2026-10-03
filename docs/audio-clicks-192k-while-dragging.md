# Audio clicks at 192 kHz while dragging the spectrum (Raspberry Pi 5 + Radioberry)

**Symptom.** Radioberry through `radioberry-juice` at 192 kHz on the Pi 5 kiosk: audio clicks
and a rising "Audio glitches" counter, but only while dragging the spectrum/frequency. CPU
was never above ~60 % busy, so it did not look like a CPU limit.

It was four separate problems. They were found one at a time with measurements, not guesses
(see "How it was measured").

## 1. Output buffer had no reserve (`src/audio.rs`)
The speaker callback drained the queue with no jitter buffer, so any late DSP block ran it dry.
Now playback starts, and restarts after a glitch, only once 40 ms are queued. Only the moment
the queue runs dry counts as a glitch, not the refill that follows. The status bar shows
`Audio glitches: N/min (buf XX ms)`, the lowest queue level in the last second.

## 2. The UI held the lock the DSP needs (`src/spectrum.rs`, `src/main.rs`)
Dragging rebuilds the waterfall every frame. The UI copied every waterfall row (a `Vec<Vec<f32>>`)
while holding the `display` mutex that the DSP thread needs in the middle of each block, so
the DSP waited. `SpectrumDisplay::waterfall_rows` now holds `Arc<Vec<f32>>`, so the copy
under the lock is a few reference-count bumps.

## 3. `SetRXAShiftFreq` blocked the DSP thread (`src/spectrum.rs`)
Per-stage timing showed 100 % of the slow DSP blocks (up to 40 ms against ~3 ms normally) were
inside the CTUN block, in `SetRXAShiftFreq`, which waits until WDSP has finished the block it is
processing. It now runs on its own thread (`wdsp-shift`, `shift_worker_send`), pending changes
for a channel are coalesced, and a flush runs before a channel is closed so WDSP is never
called on a destroyed channel. The NBP retune calls are also rate-limited (250 ms).

## 4. juice lost samples when the UI was busy (`src/radioberry_juice.rs`)
With 1-3 fixed, packets from juice dropped from ~10 735/s (idle) to ~10 500/s while dragging:
juice, at normal priority, was delayed by the UI and lost 2-3 % of the samples, which drained
the audio queue. Giving the juice threads real-time priority (`SCHED_FIFO` 40) removed the loss
(10 737 packets/s while dragging, 0 glitches in 45 s). `boost_juice_priority` does this after
launching juice, through `sudo -n /usr/bin/chrt -f -p 40 <tid>`. The kiosk package installs
`/etc/sudoers.d/hpsdr-rs-rt`, which allows exactly that command for user `pi`; `postinst`
checks it with `visudo -cf` and removes it if invalid. Without the rule nothing happens and juice
runs at normal priority. Remove the file to undo it.

Result at 192 kHz while dragging: 0 glitches, buffer ~53 ms, RX jitter ~3.7 ms.

## How it was measured (for the next time)
* Status bar: `UI: fps (passes) ms` and `DSP: gap proc (set zp feed) ms q`.
* `touch /tmp/hpsdr_diag.enable` makes the app append one line per frame to
  `/tmp/hpsdr_diag.csv`: wall time, pointer down/moved, RX jitter, buffer, glitch count, DSP
  gap/proc/queue, time of each part of a DSP block, and per-group WDSP setting times.
  (The app keeps the file open: do not `rm` it while running, read `/proc/<pid>/fd/<n>`.)
* A raw-socket sniffer on `lo` counting juice's packets per second, lined up with the log by wall
  clock, showed the loss was upstream of hpsdr-rs.
* Moving the USB interrupts to another CPU changed nothing; the CPU was mostly idle.
