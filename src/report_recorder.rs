/*
    Temporary (in-memory, never written to disk) RX-audio record/replay
    -- for giving a contact a quick "here's how you're coming in"
    report, the same purpose as piHPSDR's own REC/PLAY toolbar buttons
    (see radio.h's CAP_RECORDING/CAP_XMIT capture_state). Deliberately
    NOT audio_recorder.rs's "Record" button (which streams a whole RX
    session to a permanent WAV file on disk) -- this holds at most 60
    seconds of audio in RAM, replaced the next time REC is pressed.

    REC captures RX audio (the same post-Audio-Gain (l, r) tap
    audio_recorder.rs's own write_frame uses, see spectrum.rs's run())
    into a mono buffer, auto-stopping at 60s. PLAY re-injects that
    buffer as this session's TX mic input instead of the real mic/TCI
    audio (see tx.rs's run(), which checks is_playing()/next_sample()
    before its usual source-selection) -- so pressing PLAY while
    transmitting sends the recording back over the air for the contact
    to hear. Pressing either button again while it's running aborts it
    early rather than running to completion -- a real request: "se
    clicar aos 30s só grava 30s".
*/

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const SAMPLE_RATE_HZ: usize = 48_000;
pub const MAX_SECONDS: f32 = 60.0;
const MAX_SAMPLES: usize = (SAMPLE_RATE_HZ as f32 * MAX_SECONDS) as usize;

#[derive(Clone)]
pub struct ReportRecorder {
    recording: Arc<AtomicBool>,
    playing: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    play_pos: Arc<AtomicUsize>,
    started_at: Arc<Mutex<Option<Instant>>>,
}

impl ReportRecorder {
    pub fn new() -> Self {
        Self {
            recording: Arc::new(AtomicBool::new(false)),
            playing: Arc::new(AtomicBool::new(false)),
            buffer: Arc::new(Mutex::new(Vec::new())),
            play_pos: Arc::new(AtomicUsize::new(0)),
            started_at: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn has_recording(&self) -> bool {
        !self.buffer.lock().unwrap().is_empty()
    }

    /// Starts a fresh capture (discarding any previous recording), or
    /// stops early if already recording -- see the module doc comment's
    /// abort behavior. Also stops playback if it happened to be active,
    /// so the state machine stays correct even though the UI already
    /// disables Record while Play is running.
    pub fn toggle_record(&self) {
        if self.recording.load(Ordering::Relaxed) {
            self.recording.store(false, Ordering::Relaxed);
            return;
        }
        self.playing.store(false, Ordering::Relaxed);
        self.buffer.lock().unwrap().clear();
        *self.started_at.lock().unwrap() = Some(Instant::now());
        self.recording.store(true, Ordering::Relaxed);
    }

    /// RX audio tap -- see spectrum.rs's run(), same call site as
    /// audio_recorder.rs's own write_frame. Cheap no-op when not
    /// recording. Mono (L+R averaged): this is a spoken report memo,
    /// not a stereo recording.
    pub fn write_frame(&self, l: f32, r: f32) {
        if !self.recording.load(Ordering::Relaxed) {
            return;
        }
        let mut buf = self.buffer.lock().unwrap();
        buf.push((l + r) * 0.5);
        if buf.len() >= MAX_SAMPLES {
            self.recording.store(false, Ordering::Relaxed);
        }
    }

    /// Starts playback from the beginning, or stops early if already
    /// playing -- same abort behavior as toggle_record. No-op if
    /// nothing has been recorded yet.
    pub fn toggle_play(&self) {
        if self.playing.load(Ordering::Relaxed) {
            self.playing.store(false, Ordering::Relaxed);
            return;
        }
        if self.buffer.lock().unwrap().is_empty() {
            return;
        }
        self.play_pos.store(0, Ordering::Relaxed);
        self.playing.store(true, Ordering::Relaxed);
    }

    /// TX mic-input tap -- see tx.rs's run(). `None` once playback is
    /// off, including just now having reached the end of the buffer
    /// (which clears `playing` itself, so the caller's normal mic/TCI
    /// source selection takes back over starting the NEXT chunk).
    pub fn next_sample(&self) -> Option<f32> {
        if !self.playing.load(Ordering::Relaxed) {
            return None;
        }
        let buf = self.buffer.lock().unwrap();
        let pos = self.play_pos.fetch_add(1, Ordering::Relaxed);
        if pos >= buf.len() {
            drop(buf);
            self.playing.store(false, Ordering::Relaxed);
            return None;
        }
        Some(buf[pos])
    }

    /// 0.0..=1.0 while recording (elapsed / MAX_SECONDS) -- `None` when
    /// idle, so the caller only shows the progress bar "only while in
    /// use" per the original request.
    pub fn record_progress(&self) -> Option<f32> {
        if !self.recording.load(Ordering::Relaxed) {
            return None;
        }
        let started = (*self.started_at.lock().unwrap())?;
        Some((started.elapsed().as_secs_f32() / MAX_SECONDS).min(1.0))
    }

    /// 0.0..=1.0 while playing (play_pos / recording length) -- same
    /// "only while active" contract as record_progress.
    pub fn play_progress(&self) -> Option<f32> {
        if !self.playing.load(Ordering::Relaxed) {
            return None;
        }
        let len = self.buffer.lock().unwrap().len();
        if len == 0 {
            return None;
        }
        let pos = self.play_pos.load(Ordering::Relaxed).min(len);
        Some(pos as f32 / len as f32)
    }
}

impl Default for ReportRecorder {
    fn default() -> Self {
        Self::new()
    }
}
