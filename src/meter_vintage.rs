//! The analog meter of deskHPSDR (src/meter.c, `analog_meter`): a cream "old needle instrument" face (vertical gradient, soft glow at the
//! top, rounded corners), a dark scale, a dark red needle, red alarm marks. Ported drawing for drawing: the geometry, the colours, the line
//! widths, the text positions/sizes, the S-word table and the "sedated" needle ballistics (CNTMAX / EXPAV1 / EXPAV2) are those of meter.c.
//! It is the third MeterStyle ("Analog (deskHPSDR)").
//!
//! Everything is drawn in meter.c's own units on a virtual surface `W x vh` with `W = 250` (the meter width deskHPSDR uses when the screen is
//! wide enough: the `METER_WIDTH >= 210` text layout) and `vh` following the aspect of the rectangle given, then scaled to it. The text font
//! is deskHPSDR's FreeSansBold when it is installed (/usr/share/fonts/truetype/freefont), the default font otherwise. cairo's text origin is
//! the baseline; egui's is a box, so the baseline is placed with `BASELINE_DESCENT`.
//!
//! The face (gradient, glow, rounded mask) is rendered once into a texture per size.

use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke};

/// meter.c's METER_WIDTH for a wide screen (MIN_METER_WIDTH 200 + 50).
const W: f32 = 250.0;
const FACE_RADIUS: f32 = 8.0;
/// Texture oversampling of the face.
const K: f32 = 3.0;
/// Fraction of the font size between the baseline and the bottom of the text box.
const BASELINE_DESCENT: f32 = 0.21;

const PAN_LINE_EXTRA: f32 = 2.0;
const PAN_LINE_ZEIGER: f32 = 3.0;
const PAN_LINE_THICK: f32 = 1.0;

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied((r * 255.0 + 0.5) as u8, (g * 255.0 + 0.5) as u8, (b * 255.0 + 0.5) as u8, (a * 255.0 + 0.5) as u8)
}
/// meter_set_analog_scale_colour
fn scale_colour() -> Color32 {
    rgba(0.06, 0.045, 0.030, 1.0)
}
/// meter_set_analog_needle_colour
fn needle_colour() -> Color32 {
    rgba(0.55, 0.055, 0.030, 1.0)
}
const COLOUR_ALARM: Color32 = Color32::from_rgb(255, 0, 0);
const COLOUR_BLACK: Color32 = Color32::BLACK;

// ---- S-word table (meter.c: lowlimitsHF/uplimitsHF/lowlimitsUKW/uplimitsUKW, dbm2smeter)
const NUM_SWERTE: usize = 19;
const LOW_HF: [i32; NUM_SWERTE] = [-200, -121, -115, -109, -103, -97, -91, -85, -79, -73, -68, -63, -58, -53, -48, -43, -33, -23, -13];
const UP_HF: [i32; NUM_SWERTE] = [-122, -116, -110, -104, -98, -92, -86, -80, -74, -69, -64, -59, -54, -49, -44, -34, -24, -14, 0];
const LOW_UKW: [i32; NUM_SWERTE] = [-200, -141, -135, -129, -123, -117, -111, -105, -99, -93, -88, -83, -78, -73, -68, -63, -53, -43, -33];
const UP_UKW: [i32; NUM_SWERTE] = [-142, -136, -130, -124, -118, -112, -106, -100, -94, -89, -84, -79, -74, -69, -64, -54, -44, -34, 0];
const DBM2SMETER: [&str; NUM_SWERTE + 1] = [
    "no signal", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S9+5db", "S9+10db", "S9+15db", "S9+20db", "S9+25db", "S9+30db", "S9+40db",
    "S9+50db", "S9+60db", "out of range",
];

fn get_swert(dbm: i32, freq_hz: f64) -> usize {
    for i in 0..NUM_SWERTE {
        let (lo, up) = if freq_hz > 30_000_000.0 { (LOW_UKW[i], UP_UKW[i]) } else { (LOW_HF[i], UP_HF[i]) };
        if dbm >= lo && dbm <= up {
            return i;
        }
    }
    NUM_SWERTE
}

// ---- needle ballistics (meter.c meter_update: CNTMAX, EXPAV1, EXPAV2), one step per display frame
const CNTMAX: u32 = 5;
const EXPAV1: f64 = 0.75;
const EXPAV2: f64 = 0.25;
const MIN_RXLVL: f64 = -200.0;
const MIN_ALC: f64 = -100.0;
const MIN_PWR: f64 = 0.0;

#[derive(Clone, Copy)]
struct Ballistics {
    max_rxlvl: f64,
    max_alc: f64,
    max_pwr: f64,
    max_count: u32,
    max_pwrcount: u32,
    max_alccount: u32,
    last_power: Option<bool>,
}

impl Default for Ballistics {
    fn default() -> Self {
        Self { max_rxlvl: MIN_RXLVL, max_alc: MIN_ALC, max_pwr: MIN_PWR, max_count: 0, max_pwrcount: 0, max_alccount: 0, last_power: None }
    }
}

fn ballistics(ui: &egui::Ui, power: bool, f: impl FnOnce(&mut Ballistics)) -> Ballistics {
    let id = egui::Id::new("meter_vintage_ballistics");
    let mut b: Ballistics = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    if b.last_power != Some(power) {
        // meter type changed: reset the max values
        b = Ballistics { last_power: Some(power), ..Ballistics::default() };
    }
    f(&mut b);
    ui.ctx().data_mut(|d| d.insert_temp(id, b));
    b
}

// ---- fonts
const FONT_PATH: &str = "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf";

fn meter_font(ctx: &egui::Context, size: f32) -> FontId {
    // State: None = not tried; Some((ok, pass)) = tried at that pass. `set_fonts` only takes effect on a later pass, so the named family is used
    // from then on (using it earlier panics: "not bound to any fonts").
    let id = egui::Id::new("meter_vintage_font_installed");
    let state: Option<(bool, u64)> = ctx.data(|d| d.get_temp(id));
    let pass = ctx.cumulative_pass_nr();
    let state = match state {
        Some(s) => s,
        None => {
            let mut ok = false;
            if let Ok(bytes) = std::fs::read(FONT_PATH) {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert("freesansbold".to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                let mut list = vec!["freesansbold".to_string()];
                list.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
                fonts.families.insert(FontFamily::Name("meter".into()), list);
                ctx.set_fonts(fonts);
                ok = true;
            }
            ctx.data_mut(|d| d.insert_temp(id, (ok, pass)));
            (ok, pass)
        }
    };
    let ready = state.0 && pass > state.1 + 2;
    FontId::new(size, if ready { FontFamily::Name("meter".into()) } else { FontFamily::Proportional })
}

// ---- the face texture
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn face_pixel(x: f32, y: f32, vh: f32) -> [u8; 4] {
    // rounded-rectangle mask (meter_rounded_rectangle(0.5, 0.5, W-1, vh-1, 8)): signed distance
    let (hw, hh) = ((W - 1.0) / 2.0, (vh - 1.0) / 2.0);
    let (px, py) = ((x - W / 2.0).abs(), (y - vh / 2.0).abs());
    let (qx, qy) = (px - (hw - FACE_RADIUS), py - (hh - FACE_RADIUS));
    let sd = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - FACE_RADIUS;
    let cover = (0.5 - sd * K).clamp(0.0, 1.0);
    if cover <= 0.0 {
        return [0, 0, 0, 0];
    }
    // vertical gradient: 0.00 (0.98,0.91,0.78), 0.50 (0.93,0.83,0.64), 1.00 (0.82,0.69,0.48)
    let t = (y / vh).clamp(0.0, 1.0);
    let (a, b, u) = if t < 0.5 { ([0.98, 0.91, 0.78], [0.93, 0.83, 0.64], t / 0.5) } else { ([0.93, 0.83, 0.64], [0.82, 0.69, 0.48], (t - 0.5) / 0.5) };
    let mut c = [lerp(a[0], b[0], u), lerp(a[1], b[1], u), lerp(a[2], b[2], u)];
    // radial glow centred (W*0.5, vh*0.05), radius 1 .. W*0.75: 0 (1,0.97,0.88,0.55), 0.55 (1,0.95,0.82,0.14), 1 (1,0.95,0.82,0)
    let d = ((x - W * 0.5).powi(2) + (y - vh * 0.05).powi(2)).sqrt();
    let g = ((d - 1.0) / (W * 0.75 - 1.0)).clamp(0.0, 1.0);
    let (rgb, ga) = if g < 0.55 {
        let u = g / 0.55;
        ([lerp(1.0, 1.0, u), lerp(0.97, 0.95, u), lerp(0.88, 0.82, u)], lerp(0.55, 0.14, u))
    } else {
        ([1.0, 0.95, 0.82], lerp(0.14, 0.0, (g - 0.55) / 0.45))
    };
    for i in 0..3 {
        c[i] = c[i] * (1.0 - ga) + rgb[i] * ga;
    }
    [(c[0] * 255.0 + 0.5) as u8, (c[1] * 255.0 + 0.5) as u8, (c[2] * 255.0 + 0.5) as u8, (cover * 255.0 + 0.5) as u8]
}

fn face_texture(ctx: &egui::Context, vh: f32) -> egui::TextureHandle {
    let id = egui::Id::new(("meter_vintage_face", (vh * 100.0) as i32));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let (w, h) = ((W * K) as usize, (vh * K).ceil() as usize);
    let mut pixels = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let p = face_pixel((i as f32 + 0.5) / K, (j as f32 + 0.5) / K, vh);
            pixels.push(Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]));
        }
    }
    let img = egui::ColorImage { size: [w, h], source_size: egui::vec2(w as f32, h as f32), pixels };
    let tex = ctx.load_texture("meter_vintage_face", img, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

// ---- drawing context in meter.c's units
struct Surface<'a> {
    /// Scale of the layer transform (the kiosk draws the meter enlarged): text is laid out at its final size and scaled back, so the glyphs are
    /// rasterised at the size they are shown at (scaling a small raster up blurs bold digits).
    tscale: f32,
    painter: Painter,
    ctx: &'a egui::Context,
    origin: Pos2,
    s: f32,
    vh: f32,
}

impl Surface<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + egui::vec2(x * self.s, y * self.s)
    }
    fn line(&self, a: (f32, f32), b: (f32, f32), w: f32, c: Color32) {
        self.painter.add(Shape::line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], Stroke::new(w * self.s, c)));
    }
    fn arc(&self, cx: f32, cy: f32, r: f32, a0_deg: f32, a1_deg: f32, w: f32, c: Color32) {
        let n = (((a1_deg - a0_deg).abs() / 1.0).ceil() as usize).max(2);
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0_deg + (a1_deg - a0_deg) * i as f32 / n as f32).to_radians();
                self.p(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.painter.add(Shape::line(pts, Stroke::new(w * self.s, c)));
    }
    fn rect_fill(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.painter.rect_filled(Rect::from_min_size(self.p(x, y), egui::vec2(w * self.s, h * self.s)), 0.0, c);
    }
    fn font(&self, size: f32) -> FontId {
        meter_font(self.ctx, size * self.s * self.tscale)
    }
    /// Width of `text` in meter units (cairo_text_extents.width).
    fn text_w(&self, text: &str, size: f32) -> f32 {
        self.painter.layout_no_wrap(text.to_string(), self.font(size), Color32::WHITE).size().x / self.tscale / self.s
    }
    /// cairo_show_text: `(x, y)` is the baseline origin.
    fn text(&self, x: f32, y: f32, text: &str, size: f32, c: Color32) {
        let t = self.tscale;
        let galley = self.painter.layout_no_wrap(text.to_string(), self.font(size), c);
        let bottom_left = self.p(x, y + BASELINE_DESCENT * size);
        let top_left = bottom_left - egui::vec2(0.0, galley.size().y / t);
        let mut shape = Shape::galley(top_left, galley, c);
        shape.transform(egui::emath::TSTransform::new(top_left.to_vec2() * (1.0 - 1.0 / t), 1.0 / t));
        self.painter.add(shape);
    }
    fn polar(&self, cx: f32, r: f32, a_deg: f32) -> (f32, f32) {
        let a = a_deg.to_radians();
        (cx + r * a.cos(), cx + r * a.sin())
    }
}

fn surface<'a>(ui: &'a egui::Ui, rect: Rect) -> Surface<'a> {
    let s = rect.width() / W;
    let vh = rect.height() / s;
    let painter = ui.painter().with_clip_rect(rect);
    // The surface behind the face is black (meter_configure_event_cb).
    painter.rect_filled(rect, 0.0, COLOUR_BLACK);
    let tex = face_texture(ui.ctx(), vh);
    painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // outline: rgba(0.28, 0.20, 0.12), 1 unit, rounded rectangle at 0.5
    painter.rect_stroke(
        Rect::from_min_size(rect.min + egui::vec2(0.5 * s, 0.5 * s), egui::vec2(rect.width() - s, rect.height() - s)),
        FACE_RADIUS * s,
        Stroke::new(s, rgba(0.28, 0.20, 0.12, 1.0)),
        egui::StrokeKind::Middle,
    );
    let tscale = ui.ctx().layer_transform_to_global(ui.layer_id()).map_or(1.0, |t| t.scaling).max(0.1);
    Surface { tscale, painter, ctx: ui.ctx(), origin: rect.min, s, vh }
}

/// Arc limits (meter.c: chosen from the geometry).
fn angles(cx: f32, radius: f32, vh: f32) -> (f32, f32) {
    if cx - 0.342 * radius < vh - 5.0 {
        (200.0, 340.0)
    } else if cx - 0.5 * radius < vh - 5.0 {
        (210.0, 330.0)
    } else {
        (220.0, 320.0)
    }
}

/// What the mic / VOX | ALC bar graph needs (meter.c "ANALOG-ANZEIGE").
#[derive(Clone, Copy)]
pub struct Bars {
    /// TXA_MIC_AV in dB (-inf..0).
    pub mic_av_db: f64,
    /// The ALC meter in dB (per the ALC mode).
    pub alc_db: f64,
    pub vox_enabled: bool,
    /// vox_get_peak(), linear 0..1.
    pub vox_peak: f64,
    /// vox_threshold, linear 0..1.
    pub vox_threshold: f64,
}

fn analog_value(v_db: f64, lo: f64, a: f64, b: f64, c: f64, max: f64) -> f64 {
    let v = v_db.clamp(lo, 5.0);
    (a * v * v + b * v + c).clamp(0.0, max)
}

/// meter.c `analog_vox_meter_value` (level is linear).
fn analog_vox_meter_value(level: f64) -> f64 {
    let db = if level <= 1.0e-12 { -240.0 } else { 20.0 * level.log10() };
    analog_value(db, -30.0, 0.0571, 3.7143, 60.0, 80.0)
}

fn draw_bars(sf: &Surface, bars: &Bars, line_width: f32) {
    let (x0, y0) = (5.0f32, 10.0f32);
    let c = scale_colour();
    for (a, b) in [
        ((x0, y0), (x0, y0 + 80.0)),
        ((x0, y0), (x0 + 20.0, y0)),
        ((x0, y0 + 20.0), (x0 + 3.0, y0 + 20.0)),
        ((x0, y0 + 40.0), (x0 + 3.0, y0 + 40.0)),
        ((x0, y0 + 60.0), (x0 + 3.0, y0 + 60.0)),
        ((x0, y0 + 80.0), (x0 + 20.0, y0 + 80.0)),
        ((x0 + 20.0, y0 + 80.0), (x0 + 20.0, y0 + 40.0)),
    ] {
        sf.line(a, b, line_width, c);
    }
    sf.rect_fill(x0, y0, 21.0, 20.0, rgba(0.5, 0.0, 0.0, 1.0)); // COLOUR_ALARM_WEAK
    sf.rect_fill(x0, y0 + 20.0, 21.0, 20.0, rgba(0.0, 0.5, 0.0, 1.0)); // COLOUR_OK_WEAK
    let peak = if bars.vox_enabled { analog_vox_meter_value(bars.vox_peak) } else { analog_value(bars.mic_av_db, -30.0, 0.0571, 3.7143, 60.0, 80.0) };
    sf.rect_fill(x0 + 4.0, (y0 + 80.0) - peak as f32, 4.0, peak as f32, c);
    let alc = analog_value(bars.alc_db, -30.0, 0.0571, 3.7143, 60.0, 80.0);
    sf.rect_fill(x0 + 13.0, (y0 + 80.0) - alc as f32, 4.0, alc as f32, c);
    sf.text(x0 + 25.0, y0 + 5.0, if bars.vox_enabled { "VOX | ALC" } else { "Mic | ALC" }, 10.0, c);
    if bars.vox_enabled {
        let thr = analog_vox_meter_value(bars.vox_threshold) as f32;
        sf.line((x0, y0 + 80.0 - thr), (x0 + 10.0, y0 + 80.0 - thr), line_width + 1.5, rgba(1.0, 1.0, 0.0, 1.0)); // COLOUR_ATTN
    }
}

/// RX meter input.
pub struct SInfo {
    /// The S-meter reading in dBm.
    pub dbm: f64,
    pub freq_hz: f64,
    pub peak_mode: bool,
    pub receivers: usize,
    pub rx_id: usize,
    /// Mic / VOX | ALC bars: shown only while VOX is enabled (and not in CW), as in meter.c.
    pub bars: Option<Bars>,
}

pub fn draw_s_meter(ui: &mut egui::Ui, rect: Rect, info: &SInfo) {
    let b = ballistics(ui, false, |b| {
        // SMETER case of meter_update
        if b.max_count > CNTMAX {
            b.max_rxlvl = EXPAV1 * b.max_rxlvl + EXPAV2 * info.dbm;
            if b.max_rxlvl < MIN_RXLVL {
                b.max_rxlvl = MIN_RXLVL;
            }
        }
        if info.dbm > b.max_rxlvl {
            b.max_rxlvl = info.dbm;
            b.max_count = 0;
        }
        b.max_count += 1;
    });
    let sf = surface(ui, rect);
    let cx = W / 2.0;
    let radius = cx - 25.0;
    let (min_angle, max_angle) = angles(cx, radius, sf.vh);
    let bydb = (max_angle - min_angle) / 114.0;
    let scale = scale_colour();

    sf.arc(cx, cx, radius, min_angle + 6.0 * bydb, max_angle, PAN_LINE_EXTRA, scale);
    sf.arc(cx, cx, radius + 2.0, min_angle + 54.0 * bydb, max_angle, 4.0, COLOUR_ALARM);
    for i in 1..10 {
        let angle = (i as f32 * 6.0 * bydb) + min_angle;
        if i % 2 == 1 {
            let (x1, y1) = sf.polar(cx, radius + 4.0, angle);
            let (x2, y2) = sf.polar(cx, radius, angle);
            sf.line((x1, y1), (x2, y2), PAN_LINE_EXTRA, scale);
            let label = format!("{i}");
            let w = sf.text_w(&label, 17.5);
            let (mut x, y) = sf.polar(cx, radius + 6.0, angle);
            // At x=0, move left the whole width, at x==cx half of the width, and at x=2 cx do not move
            x += w * (x / (2.0 * cx) - 1.0);
            sf.text(x, y, &label, 17.5, scale);
        } else {
            let (x1, y1) = sf.polar(cx, radius + 2.0, angle);
            let (x2, y2) = sf.polar(cx, radius, angle);
            sf.line((x1, y1), (x2, y2), PAN_LINE_EXTRA, scale);
        }
    }
    for i in [20, 40, 60] {
        let angle = bydb * (i as f32 + 54.0) + min_angle;
        let (x1, y1) = sf.polar(cx, radius + 4.0, angle);
        let (x2, y2) = sf.polar(cx, radius, angle);
        sf.line((x1, y1), (x2, y2), 2.0, scale);
        let label = format!("+{i}");
        let w = sf.text_w(&label, 15.0);
        let (mut x, y) = sf.polar(cx, radius + 7.0, angle);
        x += w * (x / (2.0 * cx) - 1.0);
        sf.text(x, y, &label, 15.0, COLOUR_ALARM);
    }
    // needle
    let angle = if info.freq_hz > 30_000_000.0 {
        // VHF/UHF (beyond 30 MHz): -147 dBm is S0
        (b.max_rxlvl.max(-147.0) + 147.0) as f32 * bydb + min_angle
    } else {
        // HF (up to 30 MHz): -127 dBm is S0
        (b.max_rxlvl.max(-127.0) + 127.0) as f32 * bydb + min_angle
    };
    let tip = sf.polar(cx, radius + 8.0, angle);
    sf.line(tip, (cx, cx), PAN_LINE_ZEIGER, needle_colour());

    // readings (METER_WIDTH >= 210 layout)
    let current = (b.max_rxlvl - 0.5) as i32;
    let alarm = current > -69;
    let col = if alarm { COLOUR_ALARM } else { COLOUR_BLACK };
    // The two big readings sit near the bottom edge of the face (deskHPSDR's own surface is about 95 units high, so its y = cx - radius + 64
    // lands 6 units above the bottom); keep that distance from the bottom whatever the height of the box.
    let y_read = sf.vh - 6.0;
    sf.text(cx + 10.0, y_read, &format!("{current} dBm"), 22.0, col);
    sf.text(cx - 90.0, y_read, DBM2SMETER[get_swert(current, info.freq_hz)], 22.0, col);
    let mode_txt = if info.peak_mode { "Peak" } else { "Average" };
    let mode_w = sf.text_w(mode_txt, 14.5);
    sf.text(cx - mode_w / 2.0, cx - radius + 22.0, mode_txt, 14.5, COLOUR_BLACK);
    if info.receivers > 1 {
        sf.text(cx - 115.0, cx - radius - 3.0, &format!("RX{}", info.rx_id + 1), 14.5, COLOUR_BLACK);
    }
    if let Some(bars) = &info.bars {
        draw_bars(&sf, bars, PAN_LINE_ZEIGER);
    }
    let _ = PAN_LINE_THICK;
}

/// TX meter input.
pub struct TxInfo {
    /// Instantaneous forward power (W); the needle ballistics are applied here.
    pub watts: f64,
    pub swr: f64,
    pub swr_alarm: f64,
    /// Full scale = the PA power setting (W).
    pub max_watts: u32,
    pub alc_db: f64,
    pub cw: bool,
    pub bars: Option<Bars>,
}

pub fn draw_power_meter(ui: &mut egui::Ui, rect: Rect, info: &TxInfo) {
    let b = ballistics(ui, true, |b| {
        // POWER case of meter_update
        if b.max_pwrcount > CNTMAX {
            b.max_pwr = EXPAV1 * b.max_pwr + EXPAV2 * info.watts;
            if b.max_pwr < MIN_PWR {
                b.max_pwr = MIN_PWR;
            }
        }
        if b.max_alccount > CNTMAX {
            b.max_alc = EXPAV1 * b.max_alc + EXPAV2 * info.alc_db;
            // alc goes to -Infinity during CW
            if b.max_alc < MIN_ALC {
                b.max_alc = MIN_ALC;
            }
        }
        if info.watts > b.max_pwr {
            b.max_pwr = info.watts;
            b.max_pwrcount = 0;
        }
        if info.alc_db > b.max_alc {
            b.max_alc = info.alc_db;
            b.max_alccount = 0;
        }
        b.max_pwrcount += 1;
        b.max_alccount += 1;
    });
    let sf = surface(ui, rect);
    let cx = W / 2.0;
    let radius = cx - 25.0;
    let pp = info.max_watts.max(1) as f32;
    let (units, interval) = if info.max_watts <= 1 { (1, 0.1f32) } else { (2, 0.1 * pp) };
    let (min_angle, max_angle) = angles(cx, radius, sf.vh);
    let scale = scale_colour();

    sf.arc(cx, cx, radius, min_angle, max_angle, PAN_LINE_THICK, scale);
    for i in 0..=100 {
        let angle = i as f32 * 0.01 * max_angle + (100 - i) as f32 * 0.01 * min_angle;
        if i % 10 == 0 {
            let (x1, y1) = sf.polar(cx, radius + 4.0, angle);
            let (x2, y2) = sf.polar(cx, radius, angle);
            sf.line((x1, y1), (x2, y2), PAN_LINE_THICK, scale);
            if i % 20 == 0 {
                let label = if units == 1 {
                    format!("{:.1}", 0.1 * interval * i as f32)
                } else {
                    let p = (0.1 * interval * i as f32) as i32;
                    // "1000" overwrites the right margin, replace by "1K"
                    if p == 1000 { "1K".to_string() } else { format!("{p}") }
                };
                let w = sf.text_w(&label, 17.5);
                let (mut x, y) = sf.polar(cx, radius + 5.0, angle);
                x += w * (x / (2.0 * cx) - 1.0);
                sf.text(x, y, &label, 17.5, scale);
            }
        }
    }
    let mut angle = b.max_pwr as f32 * (max_angle - min_angle) / (10.0 * interval) + min_angle;
    if angle > max_angle + 5.0 {
        angle = max_angle + 5.0;
    }
    let tip = sf.polar(cx, radius + 8.0, angle);
    sf.line(tip, (cx, cx), PAN_LINE_EXTRA, needle_colour());

    let txt = if info.max_watts <= 1 {
        format!("{}mW", (1000.0 * b.max_pwr + 0.5) as i32)
    } else if info.max_watts == 5 || info.max_watts == 10 {
        format!("{:.1}W", b.max_pwr)
    } else {
        format!("{}W", (b.max_pwr + 0.5) as i32)
    };
    // The watts and the SWR: larger than meter.c's 18 / 14 (the deskHPSDR screen shows them about 1.2x that) and centred, lower in the face.
    // The Mic | ALC bars and the ALC text are not shown (the main window already has MIC / ALC).
    let w_txt = sf.text_w(&txt, 22.0);
    sf.text(cx - w_txt / 2.0, sf.vh * 0.595, &txt, 22.0, scale);
    let swr_col = if info.swr > info.swr_alarm { COLOUR_ALARM } else { scale };
    let swr_txt = format!("SWR {:.1}:1", info.swr);
    let w_swr = sf.text_w(&swr_txt, 17.0);
    sf.text(cx - w_swr / 2.0, sf.vh * 0.79, &swr_txt, 17.0, swr_col);
    let _ = (info.cw, &info.bars, b.max_alc);
}
