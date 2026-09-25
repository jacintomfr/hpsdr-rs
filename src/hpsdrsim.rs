/*
    A minimal, built-in Protocol 1 (Metis/old protocol) hardware
    emulator -- lets hpsdr-rs's own discovery/connect/RX pipeline be
    tested with no real radio attached at all, similar in spirit to
    piHPSDR's separate `hpsdrsim`/`newhpsdrsim` command-line tool
    (src/hpsdrsim.c upstream), but implemented in-process here instead
    of as an external program, and deliberately scoped down: this
    synthesizes a simple RX test signal (a fixed tone + light noise) and
    answers discovery/Start/Stop correctly for the wire, rather than
    porting that tool's full feature set (TX feedback distortion
    modeling, digital I/O simulation, a 60-second pre-recorded speech
    IQ dump, etc.) -- none of which this project's own RX-focused
    testing need actually exercises.

    Everything here mirrors radio.rs's OWN P1 wire-format knowledge
    exactly (frame sync bytes, C&C byte layout, 24-bit sample packing,
    the receiver-count-dependent interleave stride) rather than
    independently re-deriving it from the openHPSDR spec or piHPSDR's
    C source -- since radio.rs's receive side is the actual, real-
    hardware-confirmed ground truth this emulator needs to match, and
    is already right here in the same codebase.
*/

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DISCOVERY_PORT: u16 = 1024;
const USB_FRAME_SIZE: usize = 512;
const HEADER_SIZE: usize = 8;
const PACKET_SIZE: usize = HEADER_SIZE + USB_FRAME_SIZE * 2;
const EP_IQ_DATA: u8 = 0x06;
/// Host -> radio C&C/mic-audio packets -- matches radio.rs's own
/// `EP_COMMAND_AUDIO`. Real request: the synthesized tone used to stay
/// at a fixed offset inside the baseband regardless of VFO, unlike a
/// real signal (which stays put on the actual RF spectrum and moves
/// THROUGH the baseband as you tune past it) -- parsing this lets the
/// tone behave the same way.
const EP_COMMAND_AUDIO: u8 = 0x02;
/// The synthesized signal's fixed position on the "RF dial", Hz --
/// arbitrary (doesn't correspond to a real transmitter), picked to sit
/// inside the 40m band's SSB/CW portion where a fresh discovery-time
/// default VFO frequency is likely to already be tuned nearby.
const TEST_SIGNAL_ABS_FREQ_HZ: i64 = 7_100_700;

/// Whether the Discover window's whole "hpsdrsim" section (board
/// picker, Start/Stop, this module's own explanatory text) is shown at
/// all -- a real request: this is purely a development/testing aid for
/// exercising the discovery/connect/RX pipeline with no radio attached,
/// completely unrelated to this app's own normal operation, and should
/// have zero footprint (not even visible in the UI) unless deliberately
/// opted into -- same `HPSDR_xxx=1` env var convention as
/// `lcd_kiosk_mode()` (main.rs) uses for its own opt-in-only feature.
/// Checked once per frame at the UI call site rather than cached, same
/// reasoning as that function: cheap, and lets toggling the env var
/// take effect on the next Discover window repaint without a restart
/// being required to LEAVE it off (only relevant for someone actively
/// developing this feature itself).
pub fn hpsdrsim_enabled() -> bool {
    std::env::var("HPSDR_SIM").map(|v| v != "0").unwrap_or(false)
}

/// Which board this session pretends to be -- see board_id/version/
/// mac/receivers below for exactly what differs on the wire (a real
/// request: "os comandos são diferentes", confirmed against radio.rs's
/// own discovery::board_info_p1 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimBoard {
    Metis,
    HermesLite2,
}

impl SimBoard {
    pub fn label(&self) -> &'static str {
        match self {
            SimBoard::Metis => "Metis",
            SimBoard::HermesLite2 => "HermesLite2",
        }
    }

    /// Protocol 1 discovery board id -- see discovery.rs's
    /// board_info_p1 (0 = Metis, 6 = HermesLite/HermesLite2 depending
    /// on the version byte).
    fn board_id(&self) -> u8 {
        match self {
            SimBoard::Metis => 0,
            SimBoard::HermesLite2 => 6,
        }
    }

    /// >=42 selects the HermesLite2 (not classic HermesLite) branch of
    /// board_info_p1 -- an arbitrary value above that threshold,
    /// doesn't need to match any real firmware release.
    fn version(&self) -> u8 {
        match self {
            SimBoard::Metis => 31, // "3.1" -- matches a real Metis unit seen this session
            SimBoard::HermesLite2 => 44,
        }
    }

    /// buf[19] of the discovery reply -- HermesLite2's own supported-
    /// receivers count (board_info_p1 reads this directly for that
    /// board only); irrelevant for Metis.
    fn buf19_receivers(&self) -> u8 {
        match self {
            SimBoard::Metis => 0,
            SimBoard::HermesLite2 => 4,
        }
    }

    /// Distinct, clearly-fake MACs (real OUI prefix, obviously-fake
    /// suffix) -- NOT RADIOBERRY_SENTINEL_MAC (discovery.rs), so this
    /// never gets misidentified as an actual Radioberry/Juice session.
    fn mac(&self) -> [u8; 6] {
        match self {
            SimBoard::Metis => [0x00, 0x1C, 0xC0, 0xFF, 0xEE, 0x01],
            SimBoard::HermesLite2 => [0x00, 0x1C, 0xC0, 0xFF, 0xEE, 0x02],
        }
    }

    /// The wire receiver-count stride this board's packets use -- see
    /// radio.rs's ps_feedback_config/ps_wire_total doc comments: Metis
    /// (max_real=0) has NO PureSignal wire reservation at all, so a
    /// freshly-connected session (1 active receiver, PS off) streams at
    /// stride 1; HermesLite2 (max_real=2) unconditionally reserves 2
    /// extra wire slots for PS feedback regardless of whether PS is
    /// actually on, so it ALWAYS streams at stride 4 (tx_idx=3, +1)
    /// even with only 1 real receiver active. Getting this wrong for
    /// either board would desync radio.rs's own receive-side demux
    /// exactly the way the real silent-RX bug this project chased
    /// earlier did -- see receiver_loop's ps_wire_total-based `receivers`
    /// computation for the client side of this same contract.
    fn wire_receivers(&self) -> usize {
        match self {
            SimBoard::Metis => 1,
            SimBoard::HermesLite2 => 4,
        }
    }
}

/// Handle to a running (or stopped) emulator session -- Start/Stop from
/// the Discover window own one of these, same "background thread + stop
/// flag" shape as radioberry_juice::JuiceHandle.
pub struct SimHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    board: SimBoard,
}

impl SimHandle {
    /// Binds the shared discovery/data port (1024, SO_REUSEADDR so this
    /// doesn't fight a real discovery client's own throwaway sockets on
    /// the same machine) and spawns the emulator thread. Fails the same
    /// way a real "port already in use" would (e.g. another instance,
    /// or radioberry-juice, already bound to 1024) -- surfaced to the
    /// caller rather than silently retried, so the Discover window can
    /// show a real error instead of a mysteriously-unresponsive sim.
    pub fn start(board: SimBoard) -> io::Result<Self> {
        let socket = bind_shared_udp(DISCOVERY_PORT)?;
        // Read timeout, not non-blocking -- lets the loop below block
        // efficiently between packets/streaming ticks instead of
        // busy-spinning, while still waking up often enough to notice
        // `stop` promptly and to keep the streaming pacing below on
        // schedule.
        socket.set_read_timeout(Some(Duration::from_millis(2)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            // Diagnostic wrapper (temporary, chasing a real report): a
            // second silent thread death was observed even after
            // fixing recv_from's WSAEMSGSIZE crash -- no "fatal error"
            // line THIS time either, which rules that specific error
            // path out and points at an unwinding panic somewhere else
            // in run() instead (send_to's own error is already
            // discarded via `let _ =`, so it's not that either, unless
            // something upstream of it -- e.g. build_iq_packet's slice
            // indexing -- panics first). catch_unwind + an explicit
            // eprintln! makes that panic's actual message/location
            // show up in the log instead of the thread just vanishing.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(socket, board, thread_stop);
            }));
            if let Err(e) = result {
                let msg = e
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| e.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                eprintln!("hpsdrsim: run() PANICKED: {msg}");
            }
        });
        Ok(Self { stop, thread: Some(thread), board })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread.is_some() && !self.stop.load(Ordering::Relaxed)
    }

    pub fn board(&self) -> SimBoard {
        self.board
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// SO_REUSEADDR via socket2, matching radio.rs's own P1 discovery
/// socket setup (discovery.rs's open_socket) -- lets this coexist with
/// hpsdr-rs's own discovery client sockets on the same machine, and
/// with the OS having briefly kept the previous run's binding around
/// (TIME_WAIT) after Stop.
fn bind_shared_udp(port: u16) -> io::Result<UdpSocket> {
    use socket2::{Domain, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

/// The emulator's main loop: answers discovery, tracks Start/Stop, and
/// streams synthetic IQ at the correct pace/format while "running".
fn run(socket: UdpSocket, board: SimBoard, stop: Arc<AtomicBool>) {
    // ROOT CAUSE FIX for a real report: was `[0u8; 1024]`, exactly the
    // size of this emulator's own largest packet (PACKET_SIZE, P1 IQ
    // data) -- but hpsdr-rs's own discover() always tries BOTH P1 and
    // P2 discovery on every interface (discovery.rs, protocol1_discovery
    // then protocol2_discovery back to back), and a P2 discovery/general
    // packet this Protocol-1-only emulator was never meant to parse can
    // exceed 1024 bytes. On Windows, recv_from into a too-small buffer
    // doesn't truncate the datagram -- it fails outright with
    // WSAEMSGSIZE ("os error 10040"), which used to hit this loop's
    // catch-all error branch and silently kill the ENTIRE emulator
    // thread (no panic, no log) the moment discovery included any P2
    // traffic at all, closing this socket and leaving Protocol 1's own
    // subsequent Start command (which by itself was always fine) with
    // nowhere left to arrive. 2048 comfortably covers any real openHPSDR
    // P1 or P2 packet on a local network with room to spare -- oversized
    // traffic should just be ignored (falls through the `0xEF 0xFE`
    // check below), never treated as fatal.
    let mut buf = [0u8; 2048];
    let mut client: Option<SocketAddr> = None;
    let mut running = false;
    let mut seq: u32 = 0;
    // Synthesizer phase accumulators -- see synth_sample's own doc
    // comment for what they drive.
    let mut tone_phase: f64 = 0.0;
    let mut rng_state: u64 = 0x2545_F491_4F6C_DD1D;
    // RX0's current dial frequency, as last seen in the host's own C&C
    // stream (register 0x04 -- see extract_rx0_frequency's own doc
    // comment) -- drives the synthesized tone's baseband offset so it
    // behaves like a real, fixed-frequency signal that moves through
    // the passband as you tune, instead of always sitting at the same
    // spot on screen. Seeded at the test signal's own frequency so it
    // starts centered even before the first C&C packet updates it.
    let mut rx0_freq_hz: i64 = TEST_SIGNAL_ABS_FREQ_HZ;

    let receivers = board.wire_receivers();
    // Same integer-division stride radio.rs's own parse_iq_stream uses
    // to derive how many interleaved sample-groups fit in one 512-byte
    // USB frame for this receiver count -- see that function's own
    // `iq_samples` computation (its doc comment there explains the
    // "+2" mic-sample slot and why this is intentionally an integer
    // floor, not exact).
    let samples_per_frame = (USB_FRAME_SIZE - HEADER_SIZE) / (receivers * 6 + 2);
    let samples_per_packet = samples_per_frame * 2; // two USB frames per packet
    // Fixed at 48kHz -- see the module doc comment's own "deliberately
    // scoped down" note. Real hardware's sample-rate/frequency/etc. are
    // all negotiated via the ongoing C&C stream from the host; this
    // emulator doesn't parse that (Start/Stop is all it needs to react
    // to), so it always streams at this one fixed, common rate instead.
    const SAMPLE_RATE_HZ: f64 = 48000.0;
    let packet_period = Duration::from_secs_f64(samples_per_packet as f64 / SAMPLE_RATE_HZ);
    let mut next_send = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((amt, src)) => {
                if amt >= 3 && buf[0] == 0xEF && buf[1] == 0xFE {
                    match buf[2] {
                        // Discovery request -- see discovery.rs's own
                        // protocol1_discovery (the exact request this
                        // answers: `EF FE 02` + zero padding to 63
                        // bytes).
                        0x02 => {
                            let reply = discovery_reply(board);
                            let _ = socket.send_to(&reply, src);
                        }
                        // Host -> radio C&C/mic-audio (EP 0x01, buf[3]
                        // distinguishes WHICH command within that EP --
                        // see radio.rs's own `packet[2] = 0x01;
                        // packet[3] = EP_COMMAND_AUDIO;`; buf[2] alone
                        // is the EP, not the sub-command, unlike every
                        // other case in this match). See
                        // extract_rx0_frequency's own doc comment for
                        // why this is the one thing this emulator DOES
                        // parse out of this otherwise-ignored packet
                        // family. ROOT CAUSE FIX for a real report: this
                        // used to be matched as `EP_COMMAND_AUDIO =>`
                        // directly against buf[2] -- since
                        // EP_COMMAND_AUDIO is 0x02, the SAME value as
                        // the Discovery arm just above, that branch was
                        // unreachable dead code, so rx0_freq_hz never
                        // actually updated and the tone silently stayed
                        // wherever the initial seed left it.
                        0x01 if amt >= 4 && buf[3] == EP_COMMAND_AUDIO => {
                            if let Some(freq) = extract_rx0_frequency(&buf[..amt]) {
                                rx0_freq_hz = freq;
                            }
                        }
                        // General Control command -- bit 0 of byte[3] is the
                        // real "run" bit (bit 1 is a separate wideband
                        // toggle this emulator ignores). Used to require
                        // byte[3]==0x03 exactly, which hpsdr-rs's own
                        // sender happens to send but real clients like
                        // deskHPSDR/piHPSDR send 0x01 -- they were never
                        // recognized as Start, so this emulator just
                        // silently never streamed IQ to them.
                        0x04 if amt >= 4 => {
                            if buf[3] & 0x01 != 0 {
                                client = Some(src);
                                running = true;
                                next_send = Instant::now();
                                eprintln!("hpsdrsim: Start received from {src}, streaming to it now");
                            } else {
                                running = false;
                                eprintln!("hpsdrsim: Stop received from {src}");
                            }
                        }
                        // Anything else (e.g. a stray Protocol 2 packet
                        // -- see the buffer-size comment above) --
                        // still worth remembering the sender in case
                        // Start's own packet is ever missed/reordered
                        // relative to these.
                        _ => {
                            if client.is_none() {
                                client = Some(src);
                            }
                        }
                    }
                }
            }
            // ROOT CAUSE FIX for a real report: this used to be `Err(e)
            // if WouldBlock || TimedOut => {}, Err(_) => break` -- ANY
            // other error silently killed the whole receive loop (no
            // panic, no log line, the thread just returned normally),
            // which on Windows turned out to be a real, reproducible
            // trap for a UDP socket specifically: sending a reply to a
            // client whose own listening socket has since closed (e.g.
            // discovery.rs's own protocol1_discovery closes its socket
            // after its own SOCKET_TIMEOUT, which can easily have
            // already elapsed by the time this emulator gets around to
            // replying to a LATER interface in the same discovery pass)
            // provokes an ICMP Port Unreachable back at the OS level,
            // which Windows then surfaces as `WSAECONNRESET` (mapped to
            // `ErrorKind::ConnectionReset`) on this SAME socket's *next*
            // `recv_from` call -- even though nothing about that next,
            // unrelated packet was actually reset. Confirmed against a
            // real hang: `netstat` showed nothing at all bound to UDP
            // 1024 minutes after the UI still claimed "Status: running"
            // (`SimHandle::is_running` only checks the JoinHandle/stop
            // flag, not whether the thread's own loop is still alive --
            // a thread that returns normally, as this one now no longer
            // does, leaves both of those looking exactly like "still
            // running"), while the actual Start command the client kept
            // sending correctly (confirmed via its own send() succeeding)
            // had nowhere left to be received. `ConnectionReset` treated
            // the same as WouldBlock/TimedOut -- meaningless noise for a
            // connectionless UDP socket, not a reason to ever stop
            // listening. Every OTHER error is still treated as fatal,
            // but now at least says so instead of vanishing silently.
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut
                    || e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(e) => {
                eprintln!("hpsdrsim: recv_from fatal error ({e}), emulator loop exiting");
                break;
            }
        }

        if running {
            if let Some(dest) = client {
                let now = Instant::now();
                if now >= next_send {
                    let packet = build_iq_packet(
                        seq,
                        receivers,
                        samples_per_frame,
                        rx0_freq_hz,
                        SAMPLE_RATE_HZ,
                        &mut tone_phase,
                        &mut rng_state,
                    );
                    let _ = socket.send_to(&packet, dest);
                    seq = seq.wrapping_add(1);
                    // Fixed-cadence scheduling (add one period, not
                    // "now + period") -- keeps the long-run average rate
                    // correct even if an individual tick is a little
                    // late (e.g. this thread got descheduled briefly),
                    // rather than compounding drift forward every time
                    // that happens.
                    next_send += packet_period;
                    if next_send < now {
                        next_send = now;
                    }
                }
            }
        }
    }
}

/// Builds the 60-byte P1 discovery reply -- see discovery.rs's
/// Device::from_p1_reply for the exact byte offsets this must match
/// (status@2, MAC@3..9, version@9, board_id@10, buf19@19).
fn discovery_reply(board: SimBoard) -> [u8; 60] {
    let mut reply = [0u8; 60];
    reply[0] = 0xEF;
    reply[1] = 0xFE;
    reply[2] = 2; // status: 2 = available (see discovery_ui.rs's device_available)
    let mac = board.mac();
    reply[3..9].copy_from_slice(&mac);
    reply[9] = board.version();
    reply[10] = board.board_id();
    reply[19] = board.buf19_receivers();
    reply
}

/// Extracts RX0's dial frequency from a host -> radio C&C packet, if
/// this specific one happens to carry it -- `None` for every other
/// register (mic audio, TX frequency, drive level, etc.), which is
/// most of them; the caller simply keeps the last value it saw. Mirrors
/// radio.rs's own `p1_build_packet` register-2 encoding exactly (see
/// its own `2 =>` match arm): each 1032-byte packet holds 2 USB frames
/// (offsets HEADER_SIZE and HEADER_SIZE+USB_FRAME_SIZE), each an 8-byte
/// sync+C&C header (`7F 7F 7F`, then C0-C4) followed by payload; C0's
/// low bit is the MOX flag (unrelated to register addressing -- masked
/// off here), and register `0x04` is specifically RX0 (vs. `0x04 + 2*n`
/// for extra receivers, which this single-RX emulator has no use for)
/// with its frequency as a big-endian u32 in C1-C4.
fn extract_rx0_frequency(packet: &[u8]) -> Option<i64> {
    for frame_start in [HEADER_SIZE, HEADER_SIZE + USB_FRAME_SIZE] {
        let frame = packet.get(frame_start..frame_start + 8)?;
        if frame[0] != 0x7F || frame[1] != 0x7F || frame[2] != 0x7F {
            continue;
        }
        if frame[3] & 0xFE == 0x04 {
            return Some(u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]) as i64);
        }
    }
    None
}

/// One synthetic I/Q sample -- a single fixed tone (so a connected
/// client's spectrum/waterfall shows a real, visible signal to confirm
/// against) plus light white noise (so it doesn't look suspiciously
/// like a pure test signal, and so NR/NB/etc. have something to
/// actually act on). Deliberately simple compared to piHPSDR's own
/// hpsdrsim (pre-recorded speech IQ, man-made-noise tables, etc.) --
/// see the module doc comment for why.
///
/// `tone_offset_hz` (baseband, can be negative) is TEST_SIGNAL_ABS_FREQ_HZ
/// minus the currently tuned RX0 frequency -- a real request: this used
/// to be a fixed 700Hz regardless of VFO, so the tone never moved
/// on-screen no matter where you tuned, unlike a real transmitter
/// (which stays at a fixed spot on the RF spectrum and sweeps THROUGH
/// the passband as you tune past it). `None` when the offset has moved
/// outside this baseband entirely (beyond +-Nyquist) -- the tone
/// correctly disappears off-screen instead of aliasing back in from the
/// wrong side, same as a real signal tuned out of range would.
fn synth_sample(tone_offset_hz: Option<f64>, sample_rate_hz: f64, tone_phase: &mut f64, rng_state: &mut u64) -> (f32, f32) {
    const TONE_AMPLITUDE: f32 = 0.25;
    const NOISE_AMPLITUDE: f32 = 0.02;

    let (i_tone, q_tone) = match tone_offset_hz {
        Some(hz) => {
            *tone_phase += 2.0 * std::f64::consts::PI * hz / sample_rate_hz;
            if *tone_phase > 2.0 * std::f64::consts::PI {
                *tone_phase -= 2.0 * std::f64::consts::PI;
            } else if *tone_phase < -2.0 * std::f64::consts::PI {
                *tone_phase += 2.0 * std::f64::consts::PI;
            }
            (TONE_AMPLITUDE * tone_phase.cos() as f32, TONE_AMPLITUDE * tone_phase.sin() as f32)
        }
        None => (0.0, 0.0),
    };

    // xorshift64 -- fast, seedable, good enough for "not a pure tone",
    // no real randomness/security property needed here.
    *rng_state ^= *rng_state << 13;
    *rng_state ^= *rng_state >> 7;
    *rng_state ^= *rng_state << 17;
    let noise_i = ((*rng_state & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * NOISE_AMPLITUDE;
    *rng_state ^= *rng_state << 13;
    *rng_state ^= *rng_state >> 7;
    *rng_state ^= *rng_state << 17;
    let noise_q = ((*rng_state & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * NOISE_AMPLITUDE;

    (i_tone + noise_i, q_tone + noise_q)
}

/// Packs a [-1.0, 1.0] sample into 3 big-endian bytes -- identical
/// scale/rounding to radio.rs's own pack_24 (spectrum.rs's IQ_NORM on
/// the decode side uses the same 2^23-1 constant), so a real hpsdr-rs
/// client decodes this emulator's samples exactly as intended, not off
/// by a scale factor.
fn pack_24(v: f32) -> [u8; 3] {
    let scaled = (v.clamp(-1.0, 1.0) * 8_388_607.0) as f64;
    let rounded = if scaled >= 0.0 { (scaled + 0.5).floor() } else { (scaled - 0.5).ceil() };
    let b = (rounded as i32).to_be_bytes();
    [b[1], b[2], b[3]]
}

/// Builds one full 1032-byte P1 IQ data packet (two 512-byte USB
/// frames) -- see radio.rs's parse_iq_stream for the exact layout this
/// mirrors: 3 sync bytes + 5 C&C echo bytes (left at 0 here -- see the
/// module doc comment on why TX/status feedback isn't simulated), then
/// `samples_per_frame` groups of (receivers * 3-byte-I + 3-byte-Q) +
/// one 2-byte mic sample (left at 0, no mic audio simulated).
fn build_iq_packet(
    seq: u32,
    receivers: usize,
    samples_per_frame: usize,
    rx0_freq_hz: i64,
    sample_rate_hz: f64,
    tone_phase: &mut f64,
    rng_state: &mut u64,
) -> [u8; PACKET_SIZE] {
    // Baseband offset of the fixed test signal from the currently tuned
    // RX0 frequency -- see synth_sample's own doc comment. Recomputed
    // once per packet (not per sample -- rx0_freq_hz only changes when
    // a C&C packet updates it, far less often than every sample) and
    // clamped to a hair inside +-Nyquist rather than the exact edge, so
    // a signal parked exactly at the passband boundary doesn't flicker
    // in and out from floating-point rounding alone.
    let nyquist_hz = sample_rate_hz / 2.0;
    let offset_hz = (TEST_SIGNAL_ABS_FREQ_HZ - rx0_freq_hz) as f64;
    let tone_offset_hz = if offset_hz.abs() < nyquist_hz * 0.999 { Some(offset_hz) } else { None };

    let mut packet = [0u8; PACKET_SIZE];
    packet[0] = 0xEF;
    packet[1] = 0xFE;
    packet[2] = 0x01;
    packet[3] = EP_IQ_DATA;
    packet[4..8].copy_from_slice(&seq.to_be_bytes());

    for frame_idx in 0..2 {
        let frame_start = HEADER_SIZE + frame_idx * USB_FRAME_SIZE;
        packet[frame_start] = 0x7F;
        packet[frame_start + 1] = 0x7F;
        packet[frame_start + 2] = 0x7F;
        // packet[frame_start+3..frame_start+8] (C0-C4) left at 0 --
        // PTT/address/status bits all read as "idle", which is a valid,
        // harmless state for a client to see from an emulator that
        // never transmits and reports no ADC overload.
        let mut b = frame_start + HEADER_SIZE;
        for _ in 0..samples_per_frame {
            for _ in 0..receivers {
                // Every receiver slot gets the SAME synthesized signal
                // -- simplest useful choice: HermesLite2's extra
                // reserved-but-unused (PS feedback) slots just carry a
                // harmless copy that the client diverts into its own
                // unused feedback queues, exactly as it would real
                // hardware's genuine feedback ADC output while PS is
                // off.
                let (i, q) = synth_sample(tone_offset_hz, sample_rate_hz, tone_phase, rng_state);
                packet[b..b + 3].copy_from_slice(&pack_24(i));
                b += 3;
                packet[b..b + 3].copy_from_slice(&pack_24(q));
                b += 3;
            }
            // Mic sample slot (2 bytes) -- left at 0, no mic audio
            // simulated (see the module doc comment).
            b += 2;
        }
    }

    packet
}
