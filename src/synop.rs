//! Plain-language (Portuguese) translation of WMO FM-12 SYNOP weather reports, as broadcast in RTTY by the Deutscher Wetterdienst (DDK2 4583 kHz,
//! DDH7 7646 kHz, DDK9 10100.8 kHz, 14467.3 kHz; 50 baud, 450 Hz shift).
//!
//! The RTTY decoder gives a running text. `SynopView::update` is called with that text every frame: it keeps a buffer of what has not been translated yet,
//! translates each station report once it is complete (at its terminator, `;` or `=`), and shows the translation after a short delay, one line at a time, so
//! the translation trails the raw text by about one report. A report that never gets its terminator is translated after the text has been quiet for a while.
//!
//! Report layout (section 1): `IIiii iRiXhVV Nddff 1snTTT 2snTdTdTd 3PPPP 4PPPP 5appp 6RRRtR 7wwW1W2 8NhCLCMCH`, then `333` (regional: `1` Tx, `2` Tn,
//! `6` precipitation) and `555` (national, not translated). A group with a character that is not a digit or `/` (a reception error) is reported as
//! unreadable instead of guessed. Tables follow WMO-No. 306 (FM-12) and the SYNOP decoder of the mcHF/UHSDR port (`synop_decoder.c`) this one is based on.
//! Ship reports (`BBXX`) are announced but not translated.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// One bulletin's context: the day and hour from its `AAXX YYGGi` header and the wind unit.
#[derive(Clone, Copy, Default)]
struct Context {
    day: Option<u8>,
    hour: Option<u8>,
    /// Wind speed in knots (indicator 3 or 4) instead of metres per second.
    knots: bool,
    /// The bulletin is a ship report (BBXX): not translated.
    ship: bool,
}

/// The state of the translation of one running text.
pub struct SynopStream {
    /// Length (in bytes) of the text already moved into `buf`.
    consumed: usize,
    /// The last characters consumed, to find the place again if the decoder trims the start of its text.
    tail: String,
    buf: String,
    ctx: Context,
    last_input: Option<Instant>,
}

impl Default for SynopStream {
    fn default() -> Self {
        SynopStream { consumed: 0, tail: String::new(), buf: String::new(), ctx: Context::default(), last_input: None }
    }
}

/// How long the text may be quiet before an unterminated report is translated anyway.
const QUIET_FLUSH: Duration = Duration::from_secs(8);
/// The buffer never keeps more than this (text that never forms a report).
const BUF_MAX: usize = 3000;

impl SynopStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// `text` is the whole running text of the RTTY decoder. Returns the translations of the reports that became complete since the last call.
    pub fn feed(&mut self, text: &str) -> Vec<String> {
        // Where does the unseen part start? Normally at `consumed`; if the decoder trimmed the front of its text, find the remembered tail again.
        let mut start = self.consumed;
        let cut_ok = start <= text.len() && text.is_char_boundary(start) && text[..start].ends_with(self.tail.as_str());
        if !cut_ok {
            start = self.find_tail(text);
            if start == 0 && !self.tail.is_empty() {
                // Cleared or replaced: start over.
                self.buf.clear();
            }
        }
        let new = &text[start.min(text.len())..];
        let mut out = Vec::new();
        if !new.is_empty() {
            self.buf.push_str(new);
            self.last_input = Some(Instant::now());
        }
        self.consumed = text.len();
        self.tail = text.chars().rev().take(48).collect::<Vec<_>>().into_iter().rev().collect();
        // Complete reports: everything up to a terminator.
        while let Some(pos) = self.buf.find(['=', ';']) {
            let report: String = self.buf[..pos].to_string();
            self.buf.drain(..=pos);
            out.extend(self.translate_chunk(&report));
        }
        // A long quiet spell: translate what is there.
        if !self.buf.trim().is_empty() && self.last_input.is_some_and(|t| t.elapsed() >= QUIET_FLUSH) {
            let report = std::mem::take(&mut self.buf);
            out.extend(self.translate_chunk(&report));
        }
        if self.buf.len() > BUF_MAX {
            let mut cut = self.buf.len() - BUF_MAX / 2;
            while !self.buf.is_char_boundary(cut) {
                cut += 1;
            }
            self.buf.drain(..cut);
        }
        out
    }

    /// Where the unseen text starts in `text` when its front was trimmed: after the last place the remembered tail (or a shorter end of it) occurs. 0 if none.
    fn find_tail(&self, text: &str) -> usize {
        let chars: Vec<char> = self.tail.chars().collect();
        for keep in [chars.len(), 24, 12] {
            if keep == 0 || keep > chars.len() {
                continue;
            }
            let t: String = chars[chars.len() - keep..].iter().collect();
            if let Some(i) = text.rfind(t.as_str()) {
                return i + t.len();
            }
        }
        0
    }

    /// A chunk is the text before a terminator: possibly a header (`AAXX 09124`) in front of the first report.
    fn translate_chunk(&mut self, chunk: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut tokens: Vec<&str> = chunk.split_whitespace().collect();
        let header = tokens.iter().position(|t| t.eq_ignore_ascii_case("AAXX") || t.eq_ignore_ascii_case("BBXX"));
        if let Some(i) = header {
            if tokens[i].eq_ignore_ascii_case("BBXX") {
                // Ships and buoys: no date group after the header, every report carries its own (call sign, YYGGi, position).
                self.ctx = Context { ship: true, ..Context::default() };
                out.push("--- Boletim SYNOP de navios e bóias (BBXX) ---".to_string());
                tokens.drain(..=i);
            } else if let Some(ctx) = tokens.get(i + 1).and_then(|h| parse_header(h)) {
                self.ctx = ctx;
                let unit = if ctx.knots { "vento em nós" } else { "vento em m/s" };
                out.push(format!("--- Boletim SYNOP, dia {:02}, {:02}:00 UTC ({unit}) ---", ctx.day.unwrap_or(0), ctx.hour.unwrap_or(0)));
                tokens.drain(..i + 2);
            }
        }
        // A ship or buoy report: call sign, YYGGi, 99LaLaLa, QcLoLoLoLo. Recognised by its position group even when the BBXX header was missed.
        if let Some(k) = ship_start(&tokens, self.ctx.ship) {
            out.extend(decode_ship(&tokens[k..]));
            return out;
        }
        if self.ctx.ship {
            return out;
        }
        // Leading junk (NNNN, ZCZC 042, SMDL01 EDZW 091200 ...): the report starts at the first five-digit station index.
        if let Some(k) = tokens.iter().position(|t| t.len() == 5 && t.bytes().all(|c| c.is_ascii_digit())) {
            if let Some(line) = decode_report(&tokens[k..], &self.ctx) {
                out.push(line);
            }
        }
        out
    }
}

/// Where a ship report starts in `tokens` (its call sign), if there is one: the position group `99LaLaLa` is the third token, after the call sign and the date group.
fn ship_start(tokens: &[&str], in_ship_bulletin: bool) -> Option<usize> {
    let j = tokens.iter().position(|t| t.len() == 5 && t.starts_with("99") && t.as_bytes()[2] != b'9' && t[2..].bytes().all(|c| c.is_ascii_digit()))?;
    if j < 2 || tokens[j - 1].len() != 5 || tokens[j - 2].len() < 3 {
        return None;
    }
    // Outside a BBXX bulletin, a land report with a wind group like 99xxx is possible only when the first token is a plain five-digit index: ask for a letter or the bulletin.
    let call = tokens[j - 2];
    if in_ship_bulletin || j == 2 && call.bytes().any(|c| c.is_ascii_alphabetic()) || j == 2 && tokens.get(j + 1).is_some_and(|q| q.len() == 5) {
        Some(j - 2)
    } else {
        None
    }
}

/// Translates a ship / buoy report: `CALL YYGGi 99LaLaLa QcLoLoLoLo iRixhVV Nddff 1.. 4.. 5.. 7.. 8.. 222.. ICE ..`.
fn decode_ship(tokens: &[&str]) -> Vec<String> {
    let (call, time, lat, rest) = (tokens[0], tokens[1], tokens[2], &tokens[3..]);
    let mut head = format!("Navio/bóia {call}");
    if let (Some(d), Some(h)) = (num(time, 0, 2), num(time, 2, 4)) {
        head.push_str(&format!(", dia {d:02} {h:02}h UTC"));
    }
    let knots = matches!(dig(time, 4), Some(3) | Some(4));
    if let (Some(la), Some(q)) = (num(lat, 2, 5), rest.first().and_then(|q| dig(q, 0))) {
        let ns = if matches!(q, 3 | 5) { "S" } else { "N" };
        let ew = if matches!(q, 5 | 7) { "O" } else { "E" };
        if let Some(lo) = rest.first().and_then(|q| num(q, 1, 5)) {
            head.push_str(&format!(", posição {} °{ns} {} °{ew}", dec1(la as i32), dec1(lo as i32)));
        }
    }
    let groups = if rest.is_empty() { rest } else { &rest[1..] };
    let parts = decode_groups(groups, knots);
    vec![if parts.is_empty() { format!("{head}: sem dados legíveis") } else { format!("{head}: {}", parts.join(", ")) }]
}

fn parse_header(h: &str) -> Option<Context> {
    let b = h.as_bytes();
    if b.len() != 5 || !b.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let day = (b[0] - b'0') * 10 + (b[1] - b'0');
    let hour = (b[2] - b'0') * 10 + (b[3] - b'0');
    if !(1..=31).contains(&day) || hour > 24 {
        return None;
    }
    Some(Context { day: Some(day), hour: Some(hour), knots: matches!(b[4], b'3' | b'4'), ship: false })
}

fn is_group(t: &str) -> bool {
    t.len() == 5 && t.bytes().all(|c| c.is_ascii_digit() || c == b'/')
}

/// A digit of a group, or None for `/` (not observed).
fn dig(t: &str, i: usize) -> Option<u32> {
    t.as_bytes().get(i).and_then(|c| (*c as char).to_digit(10))
}

/// A number made of the digits `a..b` of a group, None if any is `/`.
fn num(t: &str, a: usize, b: usize) -> Option<u32> {
    let mut v = 0;
    for i in a..b {
        v = v * 10 + dig(t, i)?;
    }
    Some(v)
}

fn dec1(tenths: i32) -> String {
    format!("{:.1}", tenths as f64 / 10.0).replace('.', ",")
}

/// 1snTTT / 2snTdTdTd: sn 0 = positive, 1 = negative; in the dew-point group sn 9 means the relative humidity in per cent.
fn temperature(t: &str, what: &str) -> Option<String> {
    let sn = dig(t, 1)?;
    let v = num(t, 2, 5)? as i32;
    if what == "ponto de orvalho" && sn == 9 {
        return Some(if v > 100 { format!("humidade relativa {} %", dec1(v)) } else { format!("humidade relativa {v} %") });
    }
    let v = if sn == 1 { -v } else { v };
    Some(format!("{what} {} °C", dec1(v)))
}

fn compass(dd: u32) -> &'static str {
    const NAMES: [&str; 16] = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSO", "SO", "OSO", "O", "ONO", "NO", "NNO"];
    let deg = (dd * 10) % 360;
    NAMES[(((deg as f64 + 11.25) / 22.5) as usize) % 16]
}

fn visibility(vv: u32) -> String {
    match vv {
        0 => "visibilidade < 0,1 km".to_string(),
        1..=50 => format!("visibilidade {} km", dec1(vv as i32)),
        56..=80 => format!("visibilidade {} km", vv - 50),
        81..=88 => format!("visibilidade {} km", (vv - 80) * 5 + 30),
        89 => "visibilidade > 70 km".to_string(),
        90 => "visibilidade < 0,05 km".to_string(),
        91 => "visibilidade 0,05 km".to_string(),
        92 => "visibilidade 0,2 km".to_string(),
        93 => "visibilidade 0,5 km".to_string(),
        94 => "visibilidade 1 km".to_string(),
        95 => "visibilidade 2 km".to_string(),
        96 => "visibilidade 4 km".to_string(),
        97 => "visibilidade 10 km".to_string(),
        98 => "visibilidade 20 km".to_string(),
        99 => "visibilidade ≥ 50 km".to_string(),
        _ => "visibilidade ?".to_string(),
    }
}

fn cloud_base(h: u32) -> &'static str {
    match h {
        0 => "base das nuvens < 50 m",
        1 => "base das nuvens 50-100 m",
        2 => "base das nuvens 100-200 m",
        3 => "base das nuvens 200-300 m",
        4 => "base das nuvens 300-600 m",
        5 => "base das nuvens 600-1000 m",
        6 => "base das nuvens 1000-1500 m",
        7 => "base das nuvens 1500-2000 m",
        8 => "base das nuvens 2000-2500 m",
        _ => "base das nuvens > 2500 m ou sem nuvens",
    }
}

/// Present weather ww (code table 4677).
fn present_weather(ww: u32) -> Option<&'static str> {
    const WW: [&str; 100] = [
        "céu limpo", "nuvens a dissipar-se", "céu sem alteração", "nuvens a desenvolver-se",
        "visibilidade reduzida por fumo", "névoa seca", "poeira em suspensão",
        "poeira/areia levantada pelo vento", "remoinhos de poeira/areia", "tempestade de areia/poeira ao longe",
        "neblina", "nevoeiro raso em zonas", "nevoeiro raso contínuo",
        "relâmpagos sem trovoada audível", "precipitação avistada, não no local",
        "precipitação distante", "precipitação próxima", "trovoada sem precipitação no local",
        "rajadas à vista sem precipitação", "nuvens funil (tornado/tromba)",
        "chuvisco na última hora", "chuva na última hora", "neve na última hora",
        "chuva e neve na última hora", "chuva gelada na última hora", "aguaceiros na última hora",
        "aguaceiros de neve na última hora", "aguaceiros de granizo na última hora",
        "nevoeiro na última hora", "trovoada na última hora",
        "tempestade de poeira fraca/moderada a diminuir", "tempestade de poeira fraca/moderada sem alteração",
        "tempestade de poeira fraca/moderada a aumentar", "tempestade de poeira forte a diminuir",
        "tempestade de poeira forte sem alteração", "tempestade de poeira forte a aumentar",
        "neve levantada fraca/moderada (baixa)", "neve levantada forte (baixa)",
        "neve levantada fraca/moderada (alta)", "neve levantada forte (alta)",
        "nevoeiro ao longe", "nevoeiro em zonas", "nevoeiro, céu visível, a dissipar",
        "nevoeiro, céu invisível, a dissipar", "nevoeiro, céu visível, sem alteração",
        "nevoeiro, céu invisível, sem alteração", "nevoeiro, céu visível, a intensificar",
        "nevoeiro, céu invisível, a intensificar", "nevoeiro com geada, céu visível",
        "nevoeiro com geada, céu invisível",
        "chuvisco fraco intermitente", "chuvisco fraco contínuo", "chuvisco moderado intermitente",
        "chuvisco moderado contínuo", "chuvisco forte intermitente", "chuvisco forte contínuo",
        "chuvisco gelado fraco", "chuvisco gelado moderado/forte", "chuvisco e chuva fracos",
        "chuvisco e chuva moderados/fortes",
        "chuva fraca intermitente", "chuva fraca contínua", "chuva moderada intermitente",
        "chuva moderada contínua", "chuva forte intermitente", "chuva forte contínua",
        "chuva gelada fraca", "chuva gelada moderada/forte", "chuva e neve fracas",
        "chuva e neve moderadas/fortes",
        "neve fraca intermitente", "neve fraca contínua", "neve moderada intermitente",
        "neve moderada contínua", "neve forte intermitente", "neve forte contínua",
        "poeira de diamante", "grãos de neve", "cristais de neve", "granizo miúdo",
        "aguaceiros fracos de chuva", "aguaceiros moderados/fortes de chuva", "aguaceiros violentos de chuva",
        "aguaceiros fracos de chuva e neve", "aguaceiros moderados/fortes de chuva e neve",
        "aguaceiros fracos de neve", "aguaceiros moderados/fortes de neve",
        "aguaceiros fracos de granizo miúdo", "aguaceiros moderados/fortes de granizo miúdo",
        "aguaceiros fracos de granizo", "aguaceiros moderados/fortes de granizo",
        "trovoada recente, agora chuva fraca", "trovoada recente, agora chuva moderada/forte",
        "trovoada recente, agora neve fraca", "trovoada recente, agora neve moderada/forte",
        "trovoada fraca/moderada", "trovoada fraca/moderada com granizo", "trovoada forte",
        "trovoada forte com tempestade de poeira", "trovoada forte com granizo",
    ];
    WW.get(ww as usize).copied()
}

/// Past weather W1 (code table 4561).
fn past_weather(w: u32) -> Option<&'static str> {
    const W: [&str; 10] = [
        "nuvens cobriram menos de metade do céu", "nuvens variáveis, por vezes mais de metade",
        "nuvens cobriram mais de metade do céu", "tempestade de areia/poeira ou neve levantada",
        "nevoeiro ou neblina espessa", "chuvisco", "chuva", "neve ou chuva e neve", "aguaceiros", "trovoada",
    ];
    W.get(w as usize).copied()
}

fn low_cloud(c: u32) -> Option<&'static str> {
    const T: [&str; 10] = [
        "sem nuvens baixas", "cumulus (bom tempo)", "cumulus moderado/congestus", "cumulonimbus sem bigorna", "stratocumulus de cumulus",
        "stratocumulus", "stratus contínuo", "stratus/cumulus fractus (mau tempo)", "cumulus e stratocumulus", "cumulonimbus com bigorna",
    ];
    T.get(c as usize).copied()
}

fn mid_cloud(c: u32) -> Option<&'static str> {
    const T: [&str; 10] = [
        "sem nuvens médias", "altostratus translúcido", "altostratus opaco/nimbostratus", "altocumulus translúcido", "altocumulus em placas",
        "altocumulus em bandas", "altocumulus de cumulus", "altocumulus em várias camadas", "altocumulus castellanus", "altocumulus caótico",
    ];
    T.get(c as usize).copied()
}

fn high_cloud(c: u32) -> Option<&'static str> {
    const T: [&str; 10] = [
        "sem nuvens altas", "cirrus filamentoso", "cirrus denso em placas", "cirrus de bigorna", "cirrus a invadir o céu",
        "cirrus/cirrostratus a invadir (baixo)", "cirrus/cirrostratus a invadir (alto)", "cirrostratus a cobrir todo o céu", "cirrostratus parcial",
        "cirrocumulus",
    ];
    T.get(c as usize).copied()
}

fn tendency(a: u32) -> &'static str {
    match a {
        0 => "a subir e depois a descer",
        1 => "a subir, depois estável",
        2 => "a subir",
        3 => "a descer ou estável e depois a subir",
        4 => "estável",
        5 => "a descer e depois a subir",
        6 => "a descer, depois estável",
        7 => "a descer",
        8 => "estável ou a subir e depois a descer",
        _ => "?",
    }
}

fn precipitation(rrr: u32) -> String {
    match rrr {
        0 => "sem precipitação".to_string(),
        989 => "precipitação ≥ 989 mm".to_string(),
        990 => "vestígios de precipitação".to_string(),
        991..=999 => format!("precipitação 0,{} mm", rrr - 990),
        _ => format!("precipitação {rrr} mm"),
    }
}

fn block_country(index: &str) -> Option<&'static str> {
    Some(match index.get(..2)? {
        "01" => "Noruega",
        "02" => "Suécia",
        "03" => "Reino Unido",
        "04" => "Dinamarca/Islândia/Gronelândia",
        "06" => "Benelux/Dinamarca",
        "07" => "França",
        "08" => "Portugal/Espanha",
        "10" => "Alemanha",
        "11" => "Áustria/Chéquia/Eslováquia",
        "12" => "Polónia",
        "16" => "Itália",
        _ => return None,
    })
}

/// Measurement period of a precipitation group (tR).
fn period_text(tr: Option<u32>) -> &'static str {
    match tr {
        Some(1) => "6 h",
        Some(2) => "12 h",
        Some(3) => "18 h",
        Some(4) => "24 h",
        Some(5) => "1 h",
        Some(6) => "2 h",
        Some(7) => "3 h",
        Some(8) => "9 h",
        Some(9) => "15 h",
        _ => "período ?",
    }
}

/// Translates one station report (`tokens` start with the five-digit station index). None if it does not look like a report.
fn decode_report(tokens: &[&str], ctx: &Context) -> Option<String> {
    let index = *tokens.first()?;
    if index.len() != 5 || !index.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut head = format!("Estação {index}");
    if let Some(c) = block_country(index) {
        head.push_str(&format!(" ({c})"));
    }
    if tokens.get(1).is_some_and(|t| t.eq_ignore_ascii_case("NIL")) {
        return Some(format!("{head}: sem dados (NIL)"));
    }
    let parts = decode_groups(&tokens[1..], ctx.knots);
    if parts.is_empty() {
        return None;
    }
    Some(format!("{head}: {}", parts.join(", ")))
}

/// The groups of one report after the station identification, translated one by one (section 1, then 222 sea / 333 regional; 555 and ICE end it).
fn decode_groups(groups: &[&str], knots: bool) -> Vec<String> {
    if groups.len() < 2 {
        return Vec::new();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    // iRiXhVV: precipitation / station indicators, cloud base height, visibility.
    let mut section_one_started = false;
    if let Some(g) = groups.get(i).filter(|g| is_group(g)) {
        section_one_started = true;
        if let Some(h) = dig(g, 2) {
            parts.push(cloud_base(h).to_string());
        }
        if let Some(vv) = num(g, 3, 5) {
            parts.push(visibility(vv));
        }
        i += 1;
    }
    // Nddff (+ 00fff when the speed is 99 or more).
    if let Some(g) = groups.get(i).filter(|g| is_group(g)) {
        let n = dig(g, 0);
        let dd = num(g, 1, 3);
        let mut ff = num(g, 3, 5);
        i += 1;
        if ff == Some(99) {
            if let Some(extra) = groups.get(i).filter(|g| g.starts_with("00") && is_group(g)) {
                ff = num(extra, 2, 5);
                i += 1;
            }
        }
        if let Some(n) = n {
            parts.push(match n {
                0 => "céu limpo".to_string(),
                9 => "céu invisível".to_string(),
                _ => format!("nebulosidade {n}/8"),
            });
        }
        let unit = if knots { "nós" } else { "m/s" };
        match (dd, ff) {
            (Some(0), Some(0)) => parts.push("vento calmo".to_string()),
            (Some(99), Some(f)) => parts.push(format!("vento variável {f} {unit}")),
            (Some(d), Some(f)) if d <= 36 => parts.push(format!("vento de {} ({}°) {f} {unit}", compass(d), d * 10)),
            (None, Some(f)) => parts.push(format!("vento {f} {unit} (direção ?)")),
            _ => {}
        }
    } else if section_one_started && groups.get(i).is_some() {
        parts.push(format!("grupo de vento ilegível ({})", groups[i]));
        i += 1;
    }
    // The numbered groups; after "333" the regional section (Tx, Tn, precipitation), after "555" the national one (not translated).
    let mut section3 = false;
    let mut sea = false;
    while let Some(g) = groups.get(i) {
        i += 1;
        match *g {
            "333" => {
                section3 = true;
                sea = false;
                continue;
            }
            "222" | "444" => continue,
            "555" | "ICE" | "ice" => break,
            _ => {}
        }
        // 222Dv: the sea section of a ship report (sea temperature, waves).
        if is_group(g) && g.starts_with("222") {
            sea = true;
            section3 = false;
            continue;
        }
        if !is_group(g) {
            parts.push(format!("grupo ilegível ({g})"));
            continue;
        }
        let first = g.as_bytes()[0];
        if sea {
            match first {
                b'0' => {
                    if let (Some(sn), Some(t)) = (dig(g, 1), num(g, 2, 5)) {
                        let v = t as i32;
                        parts.push(format!("temperatura do mar {} °C", dec1(if sn % 2 == 1 { -v } else { v })));
                    }
                }
                b'2' => {
                    if let (Some(p), Some(h)) = (num(g, 1, 3), num(g, 3, 5)) {
                        parts.push(format!("ondas: período {p} s, altura {} m", dec1(h as i32 * 5)));
                    }
                }
                _ => {}
            }
            continue;
        }
        if section3 {
            match first {
                b'1' => parts.extend(temperature(g, "máxima").map(|s| format!("{s} (Tmáx)"))),
                b'2' => parts.extend(temperature(g, "mínima").map(|s| format!("{s} (Tmín)"))),
                b'6' => {
                    if let Some(r) = num(g, 1, 4) {
                        parts.push(format!("{} ({})", precipitation(r), period_text(dig(g, 4))));
                    }
                }
                _ => {}
            }
            continue;
        }
        match first {
            b'1' => parts.extend(temperature(g, "temperatura")),
            b'2' => parts.extend(temperature(g, "ponto de orvalho")),
            b'3' => {
                if let Some(p) = num(g, 1, 5) {
                    let v = p as i32;
                    parts.push(format!("pressão na estação {} hPa", dec1(if v < 5000 { v + 10000 } else { v })));
                }
            }
            b'4' => {
                if let Some(p) = num(g, 1, 5) {
                    let v = p as i32;
                    parts.push(format!("pressão ao nível do mar {} hPa", dec1(if v < 5000 { v + 10000 } else { v })));
                }
            }
            b'5' => {
                if let (Some(a), Some(p)) = (dig(g, 1), num(g, 2, 5)) {
                    parts.push(format!("tendência {} ({} hPa em 3 h)", tendency(a), dec1(p as i32)));
                }
            }
            b'6' => {
                if let Some(r) = num(g, 1, 4) {
                    parts.push(format!("{} ({})", precipitation(r), period_text(dig(g, 4))));
                }
            }
            b'7' => {
                if let Some(text) = num(g, 1, 3).and_then(present_weather) {
                    parts.push(text.to_string());
                }
                if let Some(text) = dig(g, 3).and_then(past_weather) {
                    parts.push(format!("antes: {text}"));
                }
            }
            b'8' => {
                let mut clouds = Vec::new();
                if let Some(t) = dig(g, 2).and_then(low_cloud) {
                    clouds.push(format!("baixas: {t}"));
                }
                if let Some(t) = dig(g, 3).and_then(mid_cloud) {
                    clouds.push(format!("médias: {t}"));
                }
                if let Some(t) = dig(g, 4).and_then(high_cloud) {
                    clouds.push(format!("altas: {t}"));
                }
                if !clouds.is_empty() {
                    parts.push(format!("nuvens {}", clouds.join("; ")));
                }
            }
            _ => {}
        }
    }
    parts
}

/// How long a translated line waits before it is shown, and the minimum gap between two lines appearing.
const REVEAL_DELAY: Duration = Duration::from_millis(1500);
const REVEAL_GAP: Duration = Duration::from_millis(350);
/// Translated lines kept on screen.
const KEEP_LINES: usize = 300;

/// The translated lines as the panel shows them: the stream's output delayed through a queue.
#[derive(Default)]
pub struct SynopView {
    stream: SynopStream,
    pending: VecDeque<(Instant, String)>,
    shown: Vec<String>,
    last_ready: Option<Instant>,
}

impl SynopView {
    /// Forgets everything (the user cleared the received text or switched the translation off).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Call every frame with the running text; returns true while lines are still waiting to appear (the caller keeps repainting).
    pub fn update(&mut self, text: &str) -> bool {
        let now = Instant::now();
        for line in self.stream.feed(text) {
            let ready = match self.last_ready {
                Some(prev) if prev + REVEAL_GAP > now + REVEAL_DELAY => prev + REVEAL_GAP,
                _ => now + REVEAL_DELAY,
            };
            self.last_ready = Some(ready);
            self.pending.push_back((ready, line));
        }
        while self.pending.front().is_some_and(|(t, _)| *t <= now) {
            let (_, line) = self.pending.pop_front().unwrap();
            self.shown.push(line);
        }
        if self.shown.len() > KEEP_LINES {
            let extra = self.shown.len() - KEEP_LINES;
            self.shown.drain(..extra);
        }
        !self.pending.is_empty()
    }

    pub fn lines(&self) -> &[String] {
        &self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Vec<String> {
        SynopStream::new().feed(text)
    }

    #[test]
    fn a_report_is_translated_at_its_terminator() {
        let out = run("AAXX 09124\n10338 41/// ///// 10147 20112 40131 50019 60001=\n");
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].contains("dia 09, 12:00 UTC"), "{}", out[0]);
        let l = &out[1];
        assert!(l.contains("Estação 10338"), "{l}");
        assert!(l.contains("Alemanha"), "{l}");
        assert!(l.contains("temperatura 14,7 °C"), "{l}");
        assert!(l.contains("ponto de orvalho 11,2 °C"), "{l}");
        assert!(l.contains("pressão ao nível do mar 1013,1 hPa"), "{l}");
        assert!(l.contains("a subir"), "{l}");
        assert!(l.contains("1,9 hPa"), "{l}");
    }

    #[test]
    fn the_semicolon_ends_a_report_too() {
        assert_eq!(run("10338 41/// ///// 10147 20112 40131;").len(), 1);
    }

    #[test]
    fn wind_and_visibility() {
        let out = run("AAXX 09123\n10015 11298 83315 10052 20041=");
        let l = &out[1];
        assert!(l.contains("nebulosidade 8/8"), "{l}");
        assert!(l.contains("vento de NNO (330°) 15 nós"), "{l}");
        assert!(l.contains("visibilidade 20 km"), "{l}");
        assert!(l.contains("100-200 m"), "{l}");
        assert!(l.contains("temperatura 5,2 °C"), "{l}");
    }

    #[test]
    fn negative_temperature_and_missing_values() {
        let out = run("10147 41/// ///// 11063 2//// 4////=");
        let l = &out[0];
        assert!(l.contains("temperatura -6,3 °C"), "{l}");
        assert!(!l.contains("orvalho"), "{l}");
    }

    #[test]
    fn a_corrupted_group_is_marked_not_guessed() {
        let out = run("10338 41/// ///// 10147 20112 401:1 50019=");
        let l = &out[0];
        assert!(l.contains("grupo ilegível (401:1)"), "{l}");
        assert!(l.contains("temperatura 14,7 °C"), "{l}");
    }

    #[test]
    fn section_three_gives_tmax_and_tmin() {
        let out = run("10338 41/// ///// 10147 333 10215 20083 69944=");
        let l = &out[0];
        assert!(l.contains("máxima 21,5 °C (Tmáx)"), "{l}");
        assert!(l.contains("mínima 8,3 °C (Tmín)"), "{l}");
        assert!(l.contains("precipitação 0,4 mm (24 h)"), "{l}");
    }

    #[test]
    fn section_five_five_five_is_not_translated() {
        let out = run("10338 41/// ///// 10147 555 1234 56789=");
        assert!(!out[0].contains("1234"), "{}", out[0]);
    }

    #[test]
    fn junk_before_the_station_is_skipped() {
        let out = run("NNNN\nZCZC 042\n10338 41/// ///// 10147=");
        assert!(out[0].starts_with("Estação 10338"), "{}", out[0]);
    }

    #[test]
    fn a_ship_report_from_the_screen_is_translated() {
        // Received 2026-10-10 on 10100.8 kHz.
        let out = run("UFNP 10001 99809 10738 41/92 91906 10017 29890 49998 54001 77027
89/// 22260 00017 2//// 3//// ICE /////;");
        assert_eq!(out.len(), 1, "{out:?}");
        let l = &out[0];
        assert!(l.contains("Navio/bóia UFNP, dia 10 00h UTC"), "{l}");
        assert!(l.contains("posição 80,9 °N 73,8 °E"), "{l}");
        assert!(l.contains("visibilidade 0,2 km"), "{l}");
        assert!(l.contains("vento de S (190°) 6 m/s") || l.contains("vento de SSO (190°) 6 m/s"), "{l}");
        assert!(l.contains("temperatura 1,7 °C"), "{l}");
        assert!(l.contains("humidade relativa 89,0 %"), "{l}");
        assert!(l.contains("pressão ao nível do mar 999,8 hPa"), "{l}");
        assert!(l.contains("temperatura do mar 1,7 °C"), "{l}");
    }

    #[test]
    fn a_second_ship_report_with_sea_state() {
        let out = run("UCLD 10001 99698 11618 41296 80311 11005 29930 49996 53006 77072 8772/ 22200 20503 60000 ICE 00000;");
        let l = &out[0];
        assert!(l.contains("Navio/bóia UCLD"), "{l}");
        assert!(l.contains("posição 69,8 °N 161,8 °E"), "{l}");
        assert!(l.contains("temperatura -0,5 °C"), "{l}");
        assert!(l.contains("ondas: período 5 s, altura 1,5 m"), "{l}");
        assert!(!l.contains("00000"), "{l}");
    }

    #[test]
    fn a_damaged_ship_report_keeps_what_is_readable() {
        let out = run("PDGW 1000/ 99696 10189 41/// ///// 10064 2001840083 58003 7//// 8//// 22200;");
        let l = &out[0];
        assert!(l.contains("Navio/bóia PDGW"), "{l}");
        assert!(l.contains("temperatura 6,4 °C"), "{l}");
        assert!(l.contains("grupo ilegível (2001840083)"), "{l}");
    }

    #[test]
    fn the_bbxx_header_is_announced() {
        let out = run("ZCZC 042
BBXX
UFNP 10001 99809 10738 41/92 91906 10017=");
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].contains("BBXX"), "{}", out[0]);
        assert!(out[1].contains("UFNP"), "{}", out[1]);
    }

    #[test]
    fn nil_report() {
        assert!(run("10147 NIL=")[0].contains("NIL"));
    }

    #[test]
    fn text_that_is_not_a_report_is_ignored() {
        assert!(run("CQ CQ CQ DE DDK2 DDH7 DDK9=").is_empty());
        assert!(run("RYRYRYRYRYRYRYRY\nFREQUENKIES 4583 KHZ I 7646 KHZ 10100.88KHZ=").is_empty());
    }

    #[test]
    fn it_works_on_a_growing_text() {
        let mut s = SynopStream::new();
        assert!(s.feed("AAXX 09124\n10338 41/// ///// 101").is_empty());
        assert!(s.feed("AAXX 09124\n10338 41/// ///// 10147 20112").is_empty());
        let out = s.feed("AAXX 09124\n10338 41/// ///// 10147 20112 40131=\n10382 4");
        assert_eq!(out.len(), 2);
        let out = s.feed("AAXX 09124\n10338 41/// ///// 10147 20112 40131=\n10382 41/// ///// 10090 40125=");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("Estação 10382"));
    }

    #[test]
    fn trimming_the_front_of_the_text_does_not_repeat_reports() {
        let mut s = SynopStream::new();
        let a = "AAXX 09124\n10338 41/// ///// 10147 20112 40131=\n";
        assert_eq!(s.feed(a).len(), 2);
        let b = format!("{}10382 41/// ///// 10090 40125=", &a[12..]);
        let out = s.feed(&b);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].contains("10382"));
    }

    #[test]
    fn the_view_holds_lines_back_for_a_moment() {
        let mut v = SynopView::default();
        assert!(v.update("10338 41/// ///// 10147=")); // translated, waiting
        assert!(v.lines().is_empty());
    }
}
