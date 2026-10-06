//! Minimal FreeDV Reporter (Socket.IO v4 / Engine.IO v4 over wss) client.
//! One worker thread; all public methods are non-blocking (channel send).
//! Global singleton API at the bottom (`sync`, `rx_report`, `stop`...).
use serde_json::{json, Value};
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

const HOST: &str = "qso.freedv.org";
const PROTOCOL_VERSION: i32 = 2;

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

enum Cmd {
    Freq(u64),
    Tx(String, bool),
    Msg(String),
    Rx(String, String, i32),
    Hide,
    Stop,
}

struct Reporter {
    tx: Sender<Cmd>,
    th: Option<JoinHandle<()>>,
}

#[derive(Default, Clone)]
struct State {
    freq: Option<u64>,
    tx: Option<(String, bool)>,
    msg: Option<String>,
    hidden: bool,
}

impl Reporter {
    fn start(callsign: &str, grid_square: &str, version: &str) -> Reporter {
        let (tx, rx) = channel();
        let auth = json!({
            "role": "report", "callsign": callsign, "grid_square": grid_square,
            "version": version, "rx_only": false,
            "os": std::env::consts::OS, "protocol_version": PROTOCOL_VERSION,
        });
        let th = std::thread::Builder::new()
            .name("freedv-reporter".into())
            .spawn(move || {
                let mut state = State::default();
                let mut backoff = 2u64;
                loop {
                    let t0 = Instant::now();
                    match session(&auth, &rx, &mut state) {
                        Ok(true) => return,
                        Ok(false) => eprintln!("[fdv] disconnected"),
                        Err(e) => eprintln!("[fdv] error: {e}"),
                    }
                    if t0.elapsed() > Duration::from_secs(60) {
                        backoff = 2;
                    }
                    let until = Instant::now() + Duration::from_secs(backoff);
                    while Instant::now() < until {
                        match rx.recv_timeout(Duration::from_millis(200)) {
                            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                            Ok(c) => apply(&mut state, c),
                            Err(_) => {}
                        }
                    }
                    backoff = (backoff * 2).min(60);
                }
            })
            .expect("spawn");
        Reporter { tx, th: Some(th) }
    }
    fn set_frequency(&self, hz: u64) {
        let _ = self.tx.send(Cmd::Freq(hz));
    }
    fn set_transmitting(&self, mode: &str, tx: bool) {
        let _ = self.tx.send(Cmd::Tx(mode.into(), tx));
    }
    fn set_message(&self, m: &str) {
        let _ = self.tx.send(Cmd::Msg(m.into()));
    }
    fn rx_report(&self, callsign: &str, mode: &str, snr: i32) {
        let _ = self.tx.send(Cmd::Rx(callsign.into(), mode.into(), snr));
    }
    fn hide(&self) {
        let _ = self.tx.send(Cmd::Hide);
    }
    /// Graceful disconnect; the worker finishes on its own (never blocks the caller,
    /// which may be the UI thread while a connect is in progress).
    fn stop(self) {
        let _ = self.tx.send(Cmd::Stop);
        drop(self.th);
    }
}

fn apply(s: &mut State, c: Cmd) {
    match c {
        Cmd::Freq(f) => s.freq = Some(f),
        Cmd::Tx(m, t) => s.tx = Some((m, t)),
        Cmd::Msg(m) => s.msg = Some(m),
        Cmd::Hide => s.hidden = true,
        _ => {}
    }
}

fn ev(name: &str, data: Value) -> Message {
    Message::text(format!("42{}", json!([name, data])))
}

fn send(ws: &mut Ws, m: Message) -> Result<(), String> {
    ws.send(m).map_err(|e| e.to_string())
}

/// Ok(true) = stopped by user, Ok(false) = server closed, Err = failure (reconnect).
fn session(auth: &Value, rx: &Receiver<Cmd>, st: &mut State) -> Result<bool, String> {
    let url = format!("wss://{HOST}/socket.io/?EIO=4&transport=websocket");
    let (mut ws, _) = tungstenite::connect(url).map_err(|e| e.to_string())?;
    match ws.get_mut() {
        MaybeTlsStream::Rustls(s) => s.sock.set_read_timeout(Some(Duration::from_millis(100))),
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(Duration::from_millis(100))),
        _ => Ok(()),
    }
    .map_err(|e| e.to_string())?;

    let mut last_rx = Instant::now();
    let mut ping_limit = Duration::from_secs(45);
    let mut ready = false;
    loop {
        loop {
            match rx.try_recv() {
                Ok(Cmd::Stop) => {
                    if ready {
                        let _ = send(&mut ws, Message::text("41")); // socket.io DISCONNECT
                    }
                    let _ = ws.close(None);
                    for _ in 0..20 {
                        if ws.flush().is_err() || ws.read().is_err() {
                            break;
                        }
                    }
                    return Ok(true);
                }
                Ok(c) => {
                    if ready {
                        let m = match &c {
                            Cmd::Freq(f) => Some(ev("freq_change", json!({"freq": f}))),
                            Cmd::Tx(mo, t) => Some(ev("tx_report", json!({"mode": mo, "transmitting": t}))),
                            Cmd::Msg(m) => Some(ev("message_update", json!({"message": m}))),
                            Cmd::Rx(c, m, s) => Some(ev("rx_report", json!({"callsign": c, "mode": m, "snr": s}))),
                            Cmd::Hide => Some(Message::text("42[\"hide_self\"]")),
                            Cmd::Stop => None,
                        };
                        if let Some(m) = m {
                            send(&mut ws, m)?;
                        }
                    }
                    apply(st, c);
                }
                Err(TryRecvError::Empty) => break,
                Err(_) => return Ok(true),
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                last_rx = Instant::now();
                let t: &str = t.as_ref();
                match t.as_bytes().first() {
                    Some(b'0') => {
                        if let Ok(v) = serde_json::from_str::<Value>(&t[1..]) {
                            let pi = v["pingInterval"].as_u64().unwrap_or(25000);
                            let pt = v["pingTimeout"].as_u64().unwrap_or(20000);
                            ping_limit = Duration::from_millis(pi + pt);
                        }
                        send(&mut ws, Message::text(format!("40{auth}")))?;
                    }
                    Some(b'1') => return Ok(false),
                    Some(b'2') => send(&mut ws, Message::text("3"))?,
                    Some(b'4') => {
                        let body = &t[1..];
                        match body.as_bytes().first() {
                            Some(b'4') => return Err(format!("namespace error: {t}")),
                            Some(b'2') => {
                                let v: Value = serde_json::from_str(&body[1..]).unwrap_or(Value::Null);
                                let name = v[0].as_str().unwrap_or("?");
                                if name == "connection_successful" {
                                    ready = true;
                                    if st.hidden {
                                        send(&mut ws, Message::text("42[\"hide_self\"]"))?;
                                    } else {
                                        if let Some(f) = st.freq {
                                            send(&mut ws, ev("freq_change", json!({"freq": f})))?;
                                        }
                                        if let Some((m, t)) = &st.tx {
                                            send(&mut ws, ev("tx_report", json!({"mode": m, "transmitting": t})))?;
                                        }
                                        if let Some(m) = &st.msg {
                                            send(&mut ws, ev("message_update", json!({"message": m})))?;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => return Ok(false),
            Ok(_) => last_rx = Instant::now(),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.to_string()),
        }
        if last_rx.elapsed() > ping_limit {
            return Err("ping timeout".into());
        }
    }
}

// ---------------------------------------------------------------- singleton

struct Active {
    rep: Reporter,
    call: String,
    grid: String,
    freq: Option<u64>,
    tx: Option<bool>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

/// Per-frame state sync from the UI. Cheap and non-blocking: starts the worker
/// when enabled and callsign + locator are set, forwards frequency / TX state
/// only when changed, stops it when disabled.
pub fn sync(enabled: bool, own_call: &str, own_locator: &str, freq_hz: u64, transmitting: bool) {
    let Ok(mut g) = ACTIVE.lock() else { return };
    let want = enabled && !own_call.trim().is_empty() && !own_locator.trim().is_empty();
    if !want {
        if let Some(a) = g.take() {
            drop(g);
            a.rep.stop();
        }
        return;
    }
    let (call, grid) = (own_call.trim(), own_locator.trim());
    if g.as_ref().map_or(false, |a| a.call != call || a.grid != grid) {
        if let Some(a) = g.take() {
            a.rep.stop();
        }
    }
    let a = g.get_or_insert_with(|| Active {
        rep: Reporter::start(call, grid, &format!("hpsdr-rs {}", env!("CARGO_PKG_VERSION"))),
        call: call.to_string(),
        grid: grid.to_string(),
        freq: None,
        tx: None,
    });
    if freq_hz > 0 && a.freq != Some(freq_hz) {
        a.freq = Some(freq_hz);
        a.rep.set_frequency(freq_hz);
    }
    if a.tx != Some(transmitting) {
        a.tx = Some(transmitting);
        a.rep.set_transmitting("RADEV1", transmitting);
    }
}

/// Disconnect (no-op when not running).
pub fn stop() {
    let a = ACTIVE.lock().ok().and_then(|mut g| g.take());
    if let Some(a) = a {
        a.rep.stop();
    }
}

/// Report a decoded callsign. Audio-thread safe: try_lock + channel push, no-op when off.
pub fn rx_report(callsign: &str, mode: &str, snr: i32) {
    if let Ok(g) = ACTIVE.try_lock() {
        if let Some(a) = g.as_ref() {
            a.rep.rx_report(callsign, mode, snr);
        }
    }
}
