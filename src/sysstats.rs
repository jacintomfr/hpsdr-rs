/*
    Background sampler for the main window's status bar (Settings-free,
    always-on, next to the Stop button -- see main.rs's own doc comment
    at that UI site): this process's own CPU%/memory, plus a best-effort
    ping RTT to the connected radio's IP.

    A real ask: "quality of network" matters a lot more for a Hermes/
    HermesLite2-class board reached over real Ethernet/Wi-Fi than for a
    USB-connected Ozy, and CPU/memory alone say nothing about that link
    -- radio.rs's own packet-sequence-gap tracking (RadioSession::
    rx_packets_total/rx_packets_lost) covers the "is the radio's own
    data actually arriving" side of that; this module's ping RTT is the
    complementary "how's the path to it right now" side, sampled
    independently of whatever traffic the radio itself happens to be
    sending at any given moment.

    Runs on its own background thread, sampling once a second -- cheap
    enough not to matter, but frequent enough to feel "live" without
    redoing the (system-call-heavy) CPU/memory refresh or spawning a
    `ping` child process any faster than that.
*/

use std::net::IpAddr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Snapshot read by the UI once a frame -- cheap to clone (all `Copy`
/// fields plus one `Option<f32>`), so main.rs just takes a fresh copy
/// out of the shared handle rather than holding any lock across a
/// frame's own drawing.
#[derive(Clone, Copy, Default)]
pub struct SysSnapshot {
    /// This process's own CPU usage, 0.0-100.0 per core (sysinfo's own
    /// convention -- can exceed 100.0 on a multi-core box if this
    /// process is using more than one core's worth). Matches what Task
    /// Manager's own per-process CPU% column shows, not a whole-system
    /// average -- this status bar is about hpsdr-rs's own footprint,
    /// the same thing a real report already used Task Manager to
    /// investigate.
    pub cpu_percent: f32,
    /// This process's own resident memory, in MB.
    pub mem_mb: f32,
    /// Round-trip ping time to the connected radio's IP, in ms --
    /// `None` while no radio is connected, or if the last ping attempt
    /// itself failed/timed out (that's still meaningful: shown as
    /// "--" rather than a stale old number).
    pub ping_ms: Option<f32>,
}

/// Owns the background sampler thread; dropping this stops it (same
/// "Drop cancels the thread via a stop flag" pattern as SpectrumHandle/
/// TxHandle elsewhere in this project).
pub struct SysStats {
    snapshot: Arc<Mutex<SysSnapshot>>,
    radio_ip: Arc<Mutex<Option<IpAddr>>>,
    stop: Arc<AtomicBool>,
}

impl SysStats {
    pub fn start() -> Self {
        let snapshot = Arc::new(Mutex::new(SysSnapshot::default()));
        let radio_ip = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_snapshot = Arc::clone(&snapshot);
        let thread_radio_ip = Arc::clone(&radio_ip);
        let thread_stop = Arc::clone(&stop);
        thread::spawn(move || run(thread_snapshot, thread_radio_ip, thread_stop));

        Self { snapshot, radio_ip, stop }
    }

    /// Called once per frame from main.rs with whatever radio IP is
    /// currently connected (or `None` while on the Discover screen) --
    /// cheap (a `Mutex<Option<IpAddr>>` write), so no harm calling it
    /// every frame even though the ping thread only reads it once a
    /// second.
    pub fn set_radio_ip(&self, ip: Option<IpAddr>) {
        *self.radio_ip.lock().unwrap() = ip;
    }

    pub fn snapshot(&self) -> SysSnapshot {
        *self.snapshot.lock().unwrap()
    }
}

impl Drop for SysStats {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(snapshot: Arc<Mutex<SysSnapshot>>, radio_ip: Arc<Mutex<Option<IpAddr>>>, stop: Arc<AtomicBool>) {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    // sysinfo's own documented requirement: CPU% needs two refreshes at
    // least MINIMUM_CPU_UPDATE_INTERVAL apart to have anything to
    // diff against -- the first loop iteration's reading is thrown
    // away deliberately (see the `first` flag below) rather than
    // shown as a misleading 0.0%/first-sample artifact.
    let mut first = true;
    // sysinfo's Process::cpu_usage() is normalized per LOGICAL core (a
    // single-threaded process pegging one core reads ~100% regardless
    // of how many total cores the machine has) -- Task Manager's own
    // per-process CPU% column instead normalizes against the WHOLE
    // system (all logical cores = 100%). Confirmed via a real report on
    // a 44-logical-core Xeon: this status bar showed 65.7% for the same
    // moment Task Manager showed 2.8%, a ~44x-ish mismatch matching
    // exactly this normalization difference. Dividing by the logical
    // core count (read fresh each iteration -- cheap, and avoids
    // depending on refresh ordering for a one-time count at startup)
    // converts to Task Manager's own convention so the two actually
    // agree.
    while !stop.load(Ordering::Relaxed) {
        sys.refresh_cpu_usage();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if !first {
            let core_count = sys.cpus().len().max(1) as f32;
            if let Some(proc_) = sys.process(pid) {
                let mut s = snapshot.lock().unwrap();
                s.cpu_percent = proc_.cpu_usage() / core_count;
                s.mem_mb = proc_.memory() as f32 / (1024.0 * 1024.0);
            }
        }
        first = false;

        let ip = *radio_ip.lock().unwrap();
        let ping_ms = ip.and_then(ping_once);
        snapshot.lock().unwrap().ping_ms = ping_ms;

        thread::sleep(Duration::from_secs(1));
    }
}

/// Best-effort single ping via the OS's own `ping` command rather than a
/// raw ICMP socket -- ICMP needs elevated/administrator privileges on
/// Windows for a raw socket, which this app has no other reason to
/// require; shelling out to `ping` (which itself already runs with
/// whatever privilege it needs, same as any other installed system
/// tool) avoids that entirely. Parses the LAST "<digits>ms" substring in
/// the output rather than matching an English-specific "time=" label --
/// `ping`'s own output is localized (e.g. Windows in Portuguese prints
/// "tempo=" not "time="), but the "ms" unit abbreviation itself isn't,
/// so this stays correct regardless of the OS's display language.
/// Returns `None` on any failure (unreachable, timeout, parse failure,
/// `ping` itself missing) -- the UI shows that as "--", not a stale or
/// fabricated number.
fn ping_once(ip: IpAddr) -> Option<f32> {
    let output = ping_command(ip).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Scan for the LAST "<number>ms" token (a multi-reply `ping` output
    // -- not used here (count is always 1 below), but this stays
    // correct if that ever changes -- lists times in order, so the
    // last one is the most recent).
    let mut last_ms: Option<f32> = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'm' && bytes[i + 1] == b's' {
            // Walk backwards over digits (and one optional '<' for
            // Windows' own "<1ms" sub-millisecond wording) immediately
            // before "ms".
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                if let Ok(v) = text[j..i].parse::<f32>() {
                    last_ms = Some(v);
                }
            }
        }
        i += 1;
    }
    last_ms
}

#[cfg(windows)]
fn ping_command(ip: IpAddr) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("ping");
    cmd.args(["-n", "1", "-w", "1000", &ip.to_string()]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}
#[cfg(not(windows))]
fn ping_command(ip: IpAddr) -> Command {
    let mut cmd = Command::new("ping");
    cmd.args(["-c", "1", "-W", "1", &ip.to_string()]);
    cmd
}
