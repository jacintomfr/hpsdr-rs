/*
    Support for the Radioberry "Juice" host program
    (pa3gsb/Radioberry-2.x, juice/firmware-extended/) -- a separate
    executable (radioberry-juice.exe on Windows, radioberry-juice on
    Linux) that talks to the Radioberry board over USB/FTDI D2XX,
    loads the FPGA gateware, and then exposes the radio to this
    program the normal way (standard openHPSDR discovery/UDP, same as
    any Metis/Hermes-family board) -- hpsdr-rs itself never speaks to
    the Radioberry hardware directly.

    This module only covers the two things the Discover window's
    "Radioberry Juice setup" section needs:
      - launching the juice executable as a plain child process, so the
        user doesn't need a separate terminal open just to start it
      - reading/writing the `fpga=` line in its `radioberry.props` file
        (CL016 or CL025, matching the board's actual FPGA), which juice
        reads from its own working directory at startup

    See claude/radioberry-juice-windows-build.md (or the equivalent
    install docs) for how juice itself is built/installed -- this
    module assumes a working juice executable already exists somewhere
    on disk; it doesn't build or install one.
*/

use std::collections::VecDeque;
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// The two FPGA variants juice's `radioberry.props` supports (see
/// gateware/README.md in the Radioberry-2.x repo) -- deliberately not
/// a bare `String` in the UI so an invalid value can't be typed in and
/// silently written to the props file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Fpga {
    Cl016,
    Cl025,
}

impl Fpga {
    /// Exact text juice expects after `fpga=` in radioberry.props.
    pub fn as_str(self) -> &'static str {
        match self {
            Fpga::Cl016 => "CL016",
            Fpga::Cl025 => "CL025",
        }
    }

    /// Parses the value half of a `fpga=...` line. Anything other than
    /// the two recognized values is treated as absent rather than
    /// guessed at -- an unrecognized value (hand-edited file, a future
    /// FPGA variant) shouldn't silently get overwritten with a picked
    /// default the next time this UI touches the file.
    fn from_str(s: &str) -> Option<Fpga> {
        match s.trim() {
            "CL016" => Some(Fpga::Cl016),
            "CL025" => Some(Fpga::Cl025),
            _ => None,
        }
    }

    pub const ALL: [Fpga; 2] = [Fpga::Cl016, Fpga::Cl025];
}

impl std::fmt::Display for Fpga {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Longest juice launches this session are shown for on screen -- past
/// this many lines the oldest are dropped, so a long-running board
/// (left connected for hours) can't grow this without bound. The full
/// history still always lands in radioberry-juice.log regardless of
/// this cap -- this only limits what the live viewer keeps in memory.
const MAX_CONSOLE_LINES: usize = 2000;

/// A launched juice process, plus its live console output -- shared
/// (cheap to `.clone()`, `Arc`s underneath) between the background
/// reader threads that fill the console, the Discover window that
/// created it, and the "Radioberry Juice Console" window in the main
/// connected screen, so all three see the same running process and the
/// same output regardless of which one launched or last restarted it.
///
/// `process` is `None` once juice has exited (checked lazily by
/// `is_running`/`stop`, not proactively watched) -- there's normally no
/// need to watch for exit on its own, since the only actions that care
/// (Stop, Restart) already check first.
#[derive(Clone)]
pub struct JuiceHandle {
    lines: Arc<Mutex<VecDeque<String>>>,
    process: Arc<Mutex<Option<Child>>>,
    exe_path: PathBuf,
}

impl JuiceHandle {
    fn push_line(&self, line: String) {
        let mut lines = self.lines.lock().unwrap();
        if lines.len() >= MAX_CONSOLE_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// Snapshot of everything currently buffered, oldest first, for the
    /// console window to redraw each frame. A `Vec` copy rather than a
    /// borrowed lock guard -- keeps the mutex held only as long as the
    /// copy itself takes, not for however long the UI spends drawing.
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }

    /// Whether juice is (as far as we can tell right now) still
    /// running. Calling this is also what notices and reaps an exit
    /// that happened on its own (a crash, or the user closing it by
    /// hand) -- cheap enough to call every frame the UI needs it,
    /// same as any other `try_wait()` use.
    pub fn is_running(&self) -> bool {
        let mut guard = self.process.lock().unwrap();
        match guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_status)) => {
                    *guard = None;
                    false
                }
                Ok(None) => true,
                // Can't tell either way -- side with "still running"
                // rather than let Stop/Restart silently treat a
                // genuinely running process as already gone.
                Err(_) => true,
            },
            None => false,
        }
    }

    /// Stops juice -- tries a graceful shutdown first (see
    /// `graceful_stop`'s own doc comment for exactly what that does and
    /// why it matters here), only forcibly terminating the process if
    /// that doesn't work within a few seconds. Also sweeps up any other
    /// running process with the same executable name regardless of
    /// whether this handle knows about it -- see `force_kill_all`'s own
    /// doc comment for why that matters (a real report needed a full
    /// machine reboot after Stop, traced to exactly this).
    pub fn stop(&self) {
        let mut guard = self.process.lock().unwrap();
        if let Some(mut child) = guard.take() {
            drop(guard);
            if graceful_stop(&mut child) {
                self.push_line("--- stopped (graceful shutdown, USB released cleanly) ---".to_string());
            } else {
                let _ = child.kill();
                let _ = child.wait(); // reap it -- no lingering zombie/handle
                self.push_line("--- forced kill sent (juice didn't exit on its own in time) ---".to_string());
            }
        }
        if force_kill_all(&self.exe_path) && !is_elevated() {
            self.push_line(
                "--- warning: hpsdr-rs is NOT running as Administrator -- the kill above may \
                 have silently failed to fully release the USB device (\"Access is denied\" is \
                 the typical Windows failure mode here). Use \"Relaunch as Administrator\" and \
                 try Stop again. ---"
                    .to_string(),
            );
        }
    }

    /// Stops juice if it's running, then launches a fresh instance of
    /// the same executable -- appending to this same live console/log
    /// rather than starting a new one, so a Restart still shows
    /// whatever run led up to it (useful context for diagnosing
    /// whatever made a Restart necessary). Works equally as a plain
    /// "start it again" if juice had already exited on its own.
    pub fn restart(&self) -> io::Result<()> {
        if self.is_running() {
            self.stop();
        }
        // A real report hit "device busy" launching immediately after a
        // forced stop -- Windows needs a brief moment to actually
        // release the FTDI D2XX handle before a new juice can reopen
        // it. Harmless extra delay on a graceful stop too.
        std::thread::sleep(std::time::Duration::from_millis(500));
        self.push_line("--- restarting ---".to_string());
        self.spawn_into()
    }

    /// The heavier recovery option, for when a plain Restart isn't
    /// enough and the board would otherwise need the USB cable
    /// physically unplugged and replugged: stops juice, asks Windows to
    /// disable then re-enable the Radioberry's own USB device (its
    /// FT2232H, VID 0403 / PID 6010 -- see reset_usb_device's own doc
    /// comment), then launches juice again. This is a real USB-level
    /// reset (same effect as a physical unplug/replug) rather than
    /// anything aimed at juice itself -- it doesn't depend on juice
    /// handling any particular signal gracefully, since this module has
    /// no visibility into juice's own source to know whether it does.
    pub fn reset_usb_and_restart(&self) -> io::Result<()> {
        if self.is_running() {
            self.stop();
        }
        self.push_line("--- resetting USB device (VID_0403&PID_6010) ---".to_string());
        reset_usb_device()?;
        // Give Windows time to re-enumerate the device after Enable-
        // PnpDevice returns -- that cmdlet returning success doesn't
        // guarantee the device is already fully back and ready for
        // FT_Open the instant it returns.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        self.push_line("--- restarting after USB reset ---".to_string());
        self.spawn_into()
    }

    /// Shared by `launch` (fresh handle) and `restart` (existing
    /// handle, same console/log) -- starts the child process itself
    /// and its two output-reader threads.
    fn spawn_into(&self) -> io::Result<()> {
        let mut cmd = Command::new(&self.exe_path);
        if let Some(dir) = self.exe_path.parent() {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // juice is a plain console app -- without this, Windows pops up
        // its own separate console window for it (since hpsdr-rs itself
        // has none to inherit). That window is outside hpsdr-rs's
        // control: closing it by hand kills juice "uncleanly" (no
        // chance to release its FTDI D2XX handle), which a real report
        // showed can leave the board stuck until the USB cable is
        // physically unplugged and replugged. Suppressing the window
        // entirely removes that external "terminal" altogether --
        // stdout/stderr are already piped above regardless, so nothing
        // about the live console/log is lost. Also puts juice in its
        // own process group (separate from hpsdr-rs's own, which has no
        // console at all) so `graceful_stop` can signal it individually
        // without needing a shared console.
        set_no_window_own_group(&mut cmd);

        let mut child = cmd.spawn()?;
        let log_file = Arc::new(Mutex::new(open_log_file(&self.exe_path)));

        if let Some(stdout) = child.stdout.take() {
            let handle = self.clone();
            let log_file = Arc::clone(&log_file);
            std::thread::spawn(move || pump_lines(stdout, handle, log_file));
        }
        if let Some(stderr) = child.stderr.take() {
            let handle = self.clone();
            let log_file = Arc::clone(&log_file);
            std::thread::spawn(move || pump_lines(stderr, handle, log_file));
        }

        *self.process.lock().unwrap() = Some(child);
        Ok(())
    }
}

/// Prevents Windows from popping up a separate console window for a
/// child process -- see the main call site's own doc comment for why.
/// `CREATE_NO_WINDOW` still leaves stdout/stderr fully redirectable
/// (already piped by callers regardless), it just skips creating a
/// window for it. A no-op on other platforms, which never had this
/// separate-terminal problem in the first place.
#[cfg(windows)]
fn set_no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn set_no_window(_cmd: &mut Command) {}

/// Same as `set_no_window`, but also puts the child in its own process
/// group (`CREATE_NEW_PROCESS_GROUP`). Not currently load-bearing for
/// anything this module does (graceful shutdown now goes over UDP, see
/// `graceful_stop`/`send_shutdown_packet`, not a console-control
/// event) -- kept because it's harmless and keeps juice from ever being
/// affected by a console-control event sent to hpsdr-rs's own process
/// group, if that ever becomes relevant again.
#[cfg(windows)]
fn set_no_window_own_group(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
}
#[cfg(not(windows))]
fn set_no_window_own_group(_cmd: &mut Command) {}

/// Asks juice to shut itself down the way its own author built it to,
/// instead of forcibly terminating it -- and waits a few seconds to see
/// if it actually does. This matters a lot more than it might sound:
/// direct inspection of juice's own source (radioberry.c/stream.c)
/// shows the actual cleanup that releases the FTDI D2XX handle
/// (`closeRadioberry` -> `deinit_stream` -> `FT_Close`) only runs when
/// its main loop notices its `closerb` flag and exits normally. A hard
/// kill (`TerminateProcess`, what `Child::kill` does) skips that path
/// entirely, which is almost certainly why Stop sometimes left the
/// board stuck until the cable was physically unplugged and replugged
/// -- Windows still closes juice's raw OS handles on a forced kill, but
/// the FTDI driver itself was never told to release/reset the device
/// the way `FT_Close` does.
///
/// EARLIER APPROACH (superseded): this used to send `CTRL_BREAK_EVENT`
/// via `GenerateConsoleCtrlEvent`, relying on juice's `SIGINT` handler.
/// That depends on juice actually having a console to receive the
/// event on -- but `set_no_window_own_group` (used specifically so
/// juice never pops up a window the user could accidentally close, see
/// its own doc comment) means it likely has none at all, so the event
/// had nowhere to be delivered. A real report confirmed Stop kept
/// falling back to a forced kill even after fixing an unrelated
/// visibility bug in juice's own `closerb`/`running` flags (they
/// weren't `volatile`), which pointed squarely at the signal itself
/// never arriving rather than juice ignoring it.
///
/// CURRENT APPROACH: sends a UDP packet directly to juice's own
/// `SERVICE_PORT` (1024, see radioberry.h) on localhost, using a
/// dedicated "please shut down entirely" command (`0x0005feef`) added
/// to juice's own packet handler specifically for this -- distinct
/// from the existing protocol Stop (`0x0004feef`), which only pauses
/// streaming so a real SDR client can Start again later without
/// juice exiting. juice needs this source-level addition to understand
/// the new command (see radioberry.c's handlePacket) -- against an
/// unpatched juice build, this packet is simply ignored (falls into
/// its "Received packages not for me!" default case) and `stop` falls
/// straight through to its forced-kill fallback, same as before this
/// change; nothing is worse off either way.
fn graceful_stop(child: &mut Child) -> bool {
    use std::time::{Duration, Instant};

    send_shutdown_packet();

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_status)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(_) => return false,
        }
    }
    false
}

/// The UDP port juice's own SERVICE_PORT binds to (see radioberry.h) --
/// both for openHPSDR discovery/start/stop commands from a real SDR
/// client, and now (see `graceful_stop`) for hpsdr-rs's own "please
/// exit entirely" request.
const JUICE_SERVICE_PORT: u16 = 1024;

/// Sends the `0x0005feef` "shut down entirely" command described in
/// `graceful_stop`'s doc comment to juice on localhost. Best-effort:
/// juice is always run on the same machine as hpsdr-rs (see `launch`),
/// so failures here would mean something is wrong with the local
/// network stack itself, not anything juice-specific -- there's
/// nothing more targeted to fall back to, so this is silent and the
/// caller's own timeout/kill fallback covers the "didn't work" case
/// either way.
fn send_shutdown_packet() {
    use std::net::UdpSocket;
    // Matches the wire layout hpsdr-rs's own discovery code uses for
    // this packet family (see discovery.rs's protocol1_discovery): a
    // fixed 63-byte buffer with byte 0-1 = 0xEF 0xFE (the openHPSDR
    // packet-family marker) and byte 2 = the command -- 0x05 here,
    // matching radioberry.c's new `case 0x0005feef:`.
    let mut packet = [0u8; 63];
    packet[0] = 0xEF;
    packet[1] = 0xFE;
    packet[2] = 0x05;

    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else { return };
    let _ = socket.send_to(&packet, ("127.0.0.1", JUICE_SERVICE_PORT));
}

/// Sweeps up any process with the same executable name as `exe_path`,
/// regardless of whether this `JuiceHandle` (or any handle at all)
/// ever launched it. This matters because a plain `Child::kill()` can
/// only ever affect the one process a handle actually spawned -- if an
/// earlier hpsdr-rs session (or a crash, or a manual launch outside
/// hpsdr-rs entirely) left a juice process running that nothing in the
/// current session has a handle to, Stop would otherwise silently do
/// nothing about it while it keeps holding the board's USB device
/// open. `/T` also takes down any child processes under it, not just
/// the top-level one.
///
/// Returns `true` if a matching process was found (whether or not the
/// kill itself actually succeeded -- see `is_elevated`'s own doc
/// comment for why it very often doesn't without Administrator
/// rights), so the caller can tell "nothing was running" apart from
/// "something was running and we tried to stop it".
pub fn force_kill_all(exe_path: &Path) -> bool {
    let Some(name) = exe_path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    #[cfg(windows)]
    {
        if !is_named_process_running(exe_path) {
            return false;
        }
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/T", "/IM", name]);
        set_no_window(&mut cmd);
        let _ = cmd.output();
        true
    }
    #[cfg(not(windows))]
    {
        let found = Command::new("pgrep").args(["-x", name]).output().map(|o| !o.stdout.is_empty()).unwrap_or(false);
        if found {
            let _ = Command::new("pkill").args(["-9", "-x", name]).status();
        }
        found
    }
}

/// Whether a process named like `exe_path` is currently running,
/// anywhere on this machine -- not just one this session launched.
/// Used to offer Stop for a juice instance left running from an
/// earlier hpsdr-rs session or a manual launch, which this session has
/// no `JuiceHandle` for at all.
#[cfg(windows)]
pub fn is_named_process_running(exe_path: &Path) -> bool {
    let Some(name) = exe_path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FI", &format!("IMAGENAME eq {name}"), "/NH"]);
    set_no_window(&mut cmd);
    match cmd.output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_lowercase().contains(&name.to_lowercase()),
        Err(_) => false,
    }
}
#[cfg(not(windows))]
pub fn is_named_process_running(exe_path: &Path) -> bool {
    let Some(name) = exe_path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    Command::new("pgrep").args(["-x", name]).output().map(|o| !o.stdout.is_empty()).unwrap_or(false)
}

/// Whether hpsdr-rs itself is currently running with Administrator
/// rights. This matters a great deal for Stop/Restart/Reset USB:
/// `taskkill` can fail silently or with "Access is denied" against a
/// process that's holding open a driver-backed device handle (exactly
/// what juice does via FTDI D2XX) unless the caller is elevated, and
/// `Disable-PnpDevice`/`Enable-PnpDevice` (used by Reset USB) require
/// it outright. This is checked by running `net session`, a
/// long-standing plain-Windows idiom for this exact question -- it
/// only succeeds when run elevated -- rather than calling into the
/// Windows API directly, to avoid FFI for something this well-trodden
/// a command already answers.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    let mut cmd = Command::new("net");
    cmd.arg("session");
    set_no_window(&mut cmd);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}
#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    true // not applicable -- this module's elevation concerns are Windows-specific
}

/// Relaunches hpsdr-rs itself elevated (via PowerShell's `Start-Process
/// -Verb runas`, which triggers the normal Windows UAC prompt), for
/// the "Stop/Restart isn't working, I don't know how to fix this on
/// Windows" case -- one click instead of the user having to know to
/// close hpsdr-rs and manually re-open it via "Run as Administrator".
/// The caller is expected to exit the current (non-elevated) process
/// right after calling this succeeds, since having two copies of
/// hpsdr-rs running at once (one elevated, one not) would just be
/// confusing.
#[cfg(windows)]
pub fn relaunch_elevated() -> io::Result<()> {
    let current_exe = std::env::current_exe()?;
    let script = format!(
        "Start-Process -FilePath '{}' -Verb runas",
        current_exe.display().to_string().replace('\'', "''")
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    set_no_window(&mut cmd);
    let output = cmd.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_string()))
    }
}
#[cfg(not(windows))]
pub fn relaunch_elevated() -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Elevation is only meaningful on Windows"))
}

/// Disables then re-enables the Radioberry's own USB device at the
/// Windows device level -- the software equivalent of physically
/// unplugging and replugging the USB cable, without touching the
/// cable. Matches by VID 0403 / PID 6010, the FT2232H used on the
/// Radioberry (per the project's own build docs) -- if another FTDI
/// FT2232H happens to be plugged in at the same time, this would reset
/// that one too, since Windows doesn't expose anything more specific
/// to distinguish them without also knowing its serial number.
///
/// Uses `pnputil.exe /disable-device` / `/enable-device` rather than
/// PowerShell's `Disable-PnpDevice`/`Enable-PnpDevice` cmdlets: those
/// go through WMI's PnPEntity provider, which a real report showed
/// fails with "Generic failure" (HRESULT 0x80041001, WBEM_E_NOT_
/// SUPPORTED) against the FTDI driver even when fully elevated -- that
/// specific WMI method just isn't implemented for this device's driver
/// stack. `pnputil` talks to the same underlying device manager
/// through SetupAPI instead, which is the same mechanism Device
/// Manager's own GUI "Disable"/"Enable" menu items use, and works
/// against drivers the WMI cmdlets don't support. `Get-PnpDevice` is
/// still used just to find the device's instance ID (that half worked
/// fine in the report) -- only the disable/enable step itself changed.
///
/// BUG FIX: the instance-ID match used to be a bare substring
/// (`VID_0403&PID_6010`), which also matches the legacy "USB Serial
/// Converter A/B" child interfaces Windows exposes for this chip
/// (their instance IDs look like
/// `USB\VID_0403&PID_6010&MI_00\...`/`...&MI_01\...` -- same VID/PID
/// text, just with `&MI_00`/`&MI_01` appended directly rather than a
/// `\` separator). A real report showed this caused a Reset USB run to
/// disable/enable several sibling nodes of the same physical port at
/// once, which triggered a bus re-enumeration mid-loop -- Windows was
/// left with the composite device AND one of its two channels stuck
/// disabled (`CM_PROB_DISABLED`) afterwards, exactly backwards from
/// what Reset USB is supposed to leave behind. juice genuinely needs
/// BOTH channels enabled (confirmed directly in its own source:
/// gateware.c opens "radioberry-juice B" for FPGA programming, stream.c
/// opens "radioberry-juice A" for the actual IQ sample streaming once
/// running) -- so the fix isn't to ignore the `&MI_00`/`&MI_01`
/// children, it's to never disable/enable them directly as siblings in
/// a loop. Disabling/enabling only the top-level composite device
/// (matched by the anchored pattern below) lets Windows cascade that
/// down to both channels as part of its own normal parent/child device
/// bring-up, rather than racing multiple manual toggles against each
/// other. As a safety net for a channel that's *already* stuck
/// disabled independently of its parent (as one was in that same real
/// report, most likely left over from an earlier broken reset attempt
/// before this fix), a second pass afterwards enables -- never
/// disables -- any `&MI_00`/`&MI_01` child still not `OK`; enabling an
/// already-fine device is a harmless no-op (pnputil just reports
/// "already enabled"), so this pass can't cause the same race the
/// first version did.
///
/// Requires Administrator privileges either way -- if hpsdr-rs isn't
/// running elevated, this fails with a permissions error surfaced back
/// to the caller rather than silently doing nothing.
#[cfg(windows)]
fn reset_usb_device() -> io::Result<()> {
    const VID_PID_PATTERN: &str = r"^USB\\VID_0403&PID_6010\\";
    const VID_PID_CHILD_PATTERN: &str = r"^USB\\VID_0403&PID_6010&MI_0[01]\\";
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         $devs = Get-PnpDevice | Where-Object {{ $_.InstanceId -match '{VID_PID_PATTERN}' -and $_.Status -ne 'Unknown' }}; \
         if (-not $devs) {{ Write-Error 'No matching USB device found ({VID_PID_PATTERN})'; exit 1 }}; \
         $failed = $false; \
         foreach ($d in $devs) {{ \
             pnputil.exe /disable-device \"$($d.InstanceId)\" | Out-Null; \
             if ($LASTEXITCODE -ne 0) {{ $failed = $true }} \
         }}; \
         Start-Sleep -Milliseconds 800; \
         foreach ($d in $devs) {{ \
             pnputil.exe /enable-device \"$($d.InstanceId)\" | Out-Null; \
             if ($LASTEXITCODE -ne 0) {{ $failed = $true }} \
         }}; \
         Start-Sleep -Milliseconds 500; \
         $children = Get-PnpDevice | Where-Object {{ $_.InstanceId -match '{VID_PID_CHILD_PATTERN}' -and $_.Status -eq 'Error' }}; \
         foreach ($c in $children) {{ \
             pnputil.exe /enable-device \"$($c.InstanceId)\" | Out-Null \
         }}; \
         if ($failed) {{ Write-Error 'pnputil reported at least one failure -- see hpsdr-rs error message'; exit 1 }}"
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    set_no_window(&mut cmd);
    let output = cmd.output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!(
            "USB device reset failed (are you running hpsdr-rs as Administrator?): {}",
            stderr.trim()
        )))
    }
}
#[cfg(not(windows))]
fn reset_usb_device() -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "USB device reset is only implemented on Windows"))
}

/// Opens (truncating) the log file for a launch, falling back to a
/// harmless sink if the path isn't writable -- see `log_path_for`'s
/// own doc comment for why this is never fatal to launching juice
/// itself.
fn open_log_file(exe_path: &Path) -> std::fs::File {
    std::fs::File::create(log_path_for(exe_path)).unwrap_or_else(|_| {
        std::fs::OpenOptions::new()
            .write(true)
            .open(if cfg!(windows) { "NUL" } else { "/dev/null" })
            .expect("platform null device should always be openable")
    })
}

/// Reads `reader` line by line until juice exits (or its pipe otherwise
/// closes), mirroring each line into both the shared live buffer and
/// the on-disk log file -- run on its own thread (one per stream) so
/// neither stdout nor stderr can block the other, and so the console
/// keeps filling in the background regardless of whether any hpsdr-rs
/// window is currently open to look at it.
fn pump_lines<R: io::Read>(reader: R, handle: JuiceHandle, log_file: Arc<Mutex<std::fs::File>>) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF -- juice exited or closed the pipe
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                handle.push_line(trimmed.clone());
                if let Ok(mut f) = log_file.lock() {
                    let _ = writeln!(f, "{trimmed}");
                }
            }
            Err(_) => break,
        }
    }
}

/// Platform-appropriate executable name, for pre-filling the file
/// picker's default directory listing -- juice ships as
/// radioberry-juice-x64.exe/radioberry-juice-x86.exe on Windows (see
/// BUILD-README.md) or plain radioberry-juice on Linux.
#[cfg(windows)]
pub const DEFAULT_EXE_NAME: &str = "radioberry-juice-x64.exe";
#[cfg(not(windows))]
pub const DEFAULT_EXE_NAME: &str = "radioberry-juice";

/// juice reads `radioberry.props` from its own working directory (see
/// BUILD-README.md's "Configuration" section) -- i.e. right next to
/// the executable in the normal `dist/...` layout the build produces.
pub fn props_path_for(exe_path: &Path) -> PathBuf {
    exe_path
        .parent()
        .map(|dir| dir.join("radioberry.props"))
        .unwrap_or_else(|| PathBuf::from("radioberry.props"))
}

/// Reads the current `fpga=` value out of radioberry.props, if the
/// file exists and has one. Returns `None` for a missing file, a
/// missing `fpga=` line, or a value we don't recognize (see
/// `Fpga::from_str`) -- all treated the same by the caller (fpga
/// selection shows as "not set" and picking one adds/corrects it).
pub fn read_fpga(props_path: &Path) -> Option<Fpga> {
    let contents = std::fs::read_to_string(props_path).ok()?;
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("fpga=").and_then(Fpga::from_str))
}

/// Sets (or adds) the `fpga=` line in radioberry.props, preserving
/// every other line as-is. Creates the file if it doesn't exist yet
/// (a fresh `dist/...` checkout ships a template, but this is also
/// used to write straight into a from-scratch working directory).
pub fn set_fpga(props_path: &Path, fpga: Fpga) -> io::Result<()> {
    let existing = std::fs::read_to_string(props_path).unwrap_or_default();
    let new_line = format!("fpga={}", fpga.as_str());

    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            if !found && line.trim_start().starts_with("fpga=") {
                found = true;
                new_line.clone()
            } else {
                line.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(new_line);
    }

    if let Some(parent) = props_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(props_path, lines.join("\n") + "\n")
}

/// Launches the juice executable as a child process, with its working
/// directory set to wherever the executable itself lives (so it finds
/// `radioberry.props` and the `gateware/` folder next to it, matching
/// how the `dist/...` build output is laid out -- see BUILD-README.md).
///
/// juice's stdout/stderr are piped rather than left to inherit
/// hpsdr-rs's own (a GUI app with no attached console on Windows, so
/// inherited output would otherwise go nowhere visible) -- instead
/// they're pumped, line by line, into both the returned `JuiceHandle`'s
/// live console (for a view in the UI) and `radioberry-juice.log` next
/// to the executable (truncated on each launch, so a bug report only
/// needs the latest run). Losing the log if the executable is on a
/// read-only or otherwise unwritable path is not fatal -- the live
/// console still works even then, so launch only fails if juice itself
/// fails to start.
///
/// The returned handle also lets the caller Stop/Restart this process
/// later -- see `JuiceHandle::stop`/`restart`.
pub fn launch(exe_path: &Path) -> io::Result<JuiceHandle> {
    let handle = JuiceHandle {
        lines: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_CONSOLE_LINES))),
        process: Arc::new(Mutex::new(None)),
        exe_path: exe_path.to_path_buf(),
    };
    handle.spawn_into()?;
    Ok(handle)
}

/// Where `launch` sends juice's own console output -- next to the
/// executable, same directory as radioberry.props, so it's easy to
/// find without hpsdr-rs having to display it itself.
pub fn log_path_for(exe_path: &Path) -> PathBuf {
    exe_path
        .parent()
        .map(|dir| dir.join("radioberry-juice.log"))
        .unwrap_or_else(|| PathBuf::from("radioberry-juice.log"))
}
