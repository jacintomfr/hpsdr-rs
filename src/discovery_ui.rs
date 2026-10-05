use crate::bootloader_ui::FirmwareUpdateWindow;
use crate::config::Config;
use crate::discovery::{discover, manual_discovery, Boards, Device};
use eframe::egui;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Render one grid cell as a selectable widget rather than a plain label,
/// so every column in a row participates in click-to-select and shows the
/// same highlight when the row is selected.
fn selectable_cell(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::selectable(selected, text))
}

/// Whether a device is actually startable right now -- the protocol's
/// own `status` byte, AND (RE-ADDED 2026-09-20, after
/// discovery::discover_rx888_usb was changed to actually load firmware
/// before reading the link speed -- see rx888::link_speed_warning's own
/// doc comment) gated on `usb_link_speed_warning` too. This was
/// previously reverted because the discovery-time speed check couldn't
/// reliably tell a genuinely slow link apart from a device that simply
/// hadn't loaded its real firmware yet this session -- a false positive
/// there blocked a working device from connecting at all. Now that
/// discovery brings the device up to the real streaming firmware first
/// (same PID `initialise` itself connects to), the two checks read the
/// SAME underlying speed, so gating here is no longer meaningfully less
/// trustworthy than the connect-time failure it would otherwise just
/// delay -- it just surfaces the same answer earlier.
fn device_available(dev: &Device) -> bool {
    dev.status == 2 && dev.usb_link_speed_warning.is_none()
}

/// Display text for one Ozy firmware/FPGA path row -- an explicit
/// choice always wins; otherwise shows the bundled default's own path
/// (with "(bundled)" so it's clear no action is needed) or "(not
/// found)" if even that's missing (e.g. running a non-packaged build
/// outside its source checkout with no override set yet).
fn effective_path_label(explicit: &Option<String>, default: fn() -> Option<std::path::PathBuf>) -> String {
    if let Some(p) = explicit {
        return p.clone();
    }
    match default() {
        Some(p) => format!("{} (bundled)", p.display()),
        None => "(not found -- use Choose... below)".to_string(),
    }
}

/// Checks for a juice install at the well-known path
/// `scripts/build-juice-deb.sh`'s package (and install-linux.sh directly)
/// use -- see `DEFAULT_INSTALLED_PATH`'s own doc comment. Windows has no
/// such standard location, so this always returns `None` there.
#[cfg(windows)]
fn detect_default_juice() -> Option<(String, Option<crate::radioberry_juice::Fpga>)> {
    None
}
#[cfg(not(windows))]
fn detect_default_juice() -> Option<(String, Option<crate::radioberry_juice::Fpga>)> {
    let path = std::path::Path::new(crate::radioberry_juice::DEFAULT_INSTALLED_PATH);
    if !path.is_file() {
        return None;
    }
    let fpga = crate::radioberry_juice::read_fpga(&crate::radioberry_juice::props_path_for(path));
    Some((path.display().to_string(), fpga))
}

/// What the caller should do after this frame's `show()` call.
pub enum DiscoveryAction {
    None,
    Cancelled,
    /// The second field carries this session's Radioberry Juice
    /// console handle onward, if the user launched juice from this
    /// window before connecting -- so main.rs can hand it to the new
    /// ConnectedState and keep the live console available after this
    /// window itself is gone. `None` for every other radio type, or if
    /// Juice was never launched this session.
    ///
    /// ROOT CAUSE FIX for a real report: the third field does the exact
    /// same hand-off for `sim_handle` (hpsdrsim.rs's built-in emulator),
    /// which this action previously left behind entirely -- `SimHandle`
    /// isn't `Clone` (unlike `JuiceHandle`, an external process this
    /// app doesn't own the lifetime of the same way), so this MOVES it
    /// out via `Option::take()` at the call site instead of cloning.
    /// Without this, `DiscoveryWindow` (which owned `sim_handle`) gets
    /// dropped the moment `AppState` switches from `Discovering` to
    /// `Connected` -- and `SimHandle`'s own `Drop` impl calls `stop()`,
    /// silently killing the emulator's listening thread within a couple
    /// of milliseconds of a successful Start, before it could ever
    /// actually stream anything. Confirmed via `netstat` showing UDP
    /// 1024 unbound again moments after "Started HermesLite2..." logged,
    /// with no panic and no error -- exactly what an intentional,
    /// clean `stop()` from a value going out of scope looks like.
    Start(Device, Option<crate::radioberry_juice::JuiceHandle>, Option<crate::hpsdrsim::SimHandle>),
}

/// Touch-friendly vertical scroll area: the content on the left, on the right a wide bar with an up button, a
/// draggable thumb on a track and a down button (tap or hold to scroll). Dragging the content also scrolls it.
/// `height` None = all the height left in `ui`; `stick_bottom` keeps a log scrolled to its end while it grows.
fn touch_scroll(
    ui: &mut egui::Ui,
    id_salt: &str,
    height: Option<f32>,
    stick_bottom: bool,
    contents: &mut dyn FnMut(&mut egui::Ui),
) {
    const BAR_W: f32 = 30.0;
    const GAP: f32 = 6.0;
    const BTN_H: f32 = 34.0;
    let id = egui::Id::new(("touch_scroll", id_salt));
    let height = height.unwrap_or_else(|| ui.available_height()).max(3.0 * BTN_H);
    let width = ui.available_width();
    let (mut offset, prev_max): (f32, f32) = ui.data(|d| d.get_temp(id)).unwrap_or((0.0, 0.0));
    let was_at_bottom = prev_max - offset < 2.0;

    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let area_rect = egui::Rect::from_min_size(outer.min, egui::vec2((width - BAR_W - GAP).max(50.0), height));
    let mut area_ui = ui.new_child(egui::UiBuilder::new().max_rect(area_rect).layout(egui::Layout::top_down(egui::Align::Min)));
    area_ui.set_clip_rect(area_rect.intersect(ui.clip_rect()));
    let out = egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .vertical_scroll_offset(offset)
        .show(&mut area_ui, |ui| contents(ui));
    let view_h = out.inner_rect.height();
    let max_off = (out.content_size.y - view_h).max(0.0);
    offset = out.state.offset.y;
    if stick_bottom && was_at_bottom {
        offset = max_off;
    }

    // The bar.
    let bar = egui::Rect::from_min_size(egui::pos2(outer.right() - BAR_W, outer.top()), egui::vec2(BAR_W, height));
    let up_rect = egui::Rect::from_min_size(bar.min, egui::vec2(BAR_W, BTN_H));
    let down_rect = egui::Rect::from_min_size(egui::pos2(bar.left(), bar.bottom() - BTN_H), egui::vec2(BAR_W, BTN_H));
    let track = egui::Rect::from_min_max(egui::pos2(bar.left(), up_rect.bottom() + 4.0), egui::pos2(bar.right(), down_rect.top() - 4.0));
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(95));
    let mut held = false;
    for (rect, dir) in [(up_rect, -1.0f32), (down_rect, 1.0f32)] {
        let resp = ui.interact(rect, id.with(("btn", dir as i32)), egui::Sense::click_and_drag());
        let down = resp.is_pointer_button_down_on();
        if down {
            offset += dir * 600.0 * dt;
            held = true;
        } else if resp.clicked() {
            offset += dir * 60.0;
        }
        ui.painter().rect(
            rect,
            5.0,
            if down { egui::Color32::from_gray(80) } else { egui::Color32::from_gray(50) },
            stroke,
            egui::StrokeKind::Inside,
        );
        let c = rect.center();
        let tri = if dir < 0.0 {
            vec![c + egui::vec2(0.0, -6.0), c + egui::vec2(-7.0, 5.0), c + egui::vec2(7.0, 5.0)]
        } else {
            vec![c + egui::vec2(0.0, 6.0), c + egui::vec2(-7.0, -5.0), c + egui::vec2(7.0, -5.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(tri, egui::Color32::from_gray(210), egui::Stroke::NONE));
    }
    ui.painter().rect(track, 5.0, egui::Color32::from_gray(30), stroke, egui::StrokeKind::Inside);
    if max_off > 0.5 {
        // f32::clamp panics if min > max: with a short track (a tall list leaves little room) 40 px may not fit.
        let thumb_len = (track.height() * view_h / (view_h + max_off)).clamp(40.0f32.min(track.height()), track.height());
        let travel = (track.height() - thumb_len).max(1.0);
        let resp = ui.interact(track, id.with("track"), egui::Sense::click_and_drag());
        if let Some(p) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.is_pointer_button_down_on() {
                offset = ((p.y - track.top() - thumb_len / 2.0) / travel).clamp(0.0, 1.0) * max_off;
            }
        }
        offset = offset.clamp(0.0, max_off);
        let top = track.top() + travel * offset / max_off;
        let thumb = egui::Rect::from_min_size(egui::pos2(track.left() + 2.0, top), egui::vec2(BAR_W - 4.0, thumb_len));
        ui.painter().rect(
            thumb,
            5.0,
            if resp.dragged() { egui::Color32::from_gray(150) } else { egui::Color32::from_gray(115) },
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    } else {
        offset = 0.0;
    }
    if held {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, (offset.clamp(0.0, max_off), max_off)));
}

pub struct DiscoveryWindow {
    pub open: bool,
    devices: Arc<Mutex<Vec<Device>>>,
    /// See discover()'s own doc comment -- this machine's own address ->
    /// the interface name it belongs to, for the "Interface" column.
    interface_names: Arc<Mutex<HashMap<IpAddr, String>>>,
    discovering: Arc<Mutex<bool>>,
    selected: Option<usize>,
    manual_ip: String,
    manual_error: Option<String>,
    /// When this window was created -- used to keep re-sending a focus
    /// command for a short window after creation (see `show`'s doc
    /// comment on why a single one-shot Focus isn't reliable enough: the
    /// main/root window is a genuinely separate OS window that can get
    /// mapped/raised by the window manager slightly *after* this one,
    /// re-covering it even though this window already received focus a
    /// moment earlier). `None` once that grace period has ended so the
    /// window stops stealing focus back if the user deliberately clicks
    /// the main window during that window.
    focus_deadline: Option<Instant>,
    /// Kiosk window-size correction (see the code at the top of the viewport closure): the size last requested in
    /// points, how many corrections were sent and frames to wait for the next one.
    size_req: [f32; 2],
    size_attempts: u8,
    size_wait: u8,
    /// Bootloader-mode radios never answer normal discovery (see
    /// bootloader.rs's own doc comment), so this is a standalone entry
    /// point independent of the `devices` list -- same
    /// `Option<...Window>` toggle idiom as every other secondary window
    /// in this app (e.g. ConnectedState::show_settings_window).
    firmware_update: Option<FirmwareUpdateWindow>,
    /// Classic Ozy hardware's user-supplied FX2 firmware (.hex) / FPGA
    /// bitstream (.rbf) paths -- set here, not in the (post-connect)
    /// Settings window, since Ozy needs them just to complete its very
    /// first connect. Persisted under a fixed sentinel MAC ([0;6],
    /// matching discover_ozy_usb's own synthetic Device.mac) via the
    /// same Config file mechanism every other radio's settings use --
    /// see `save_ozy_paths`.
    ozy_firmware_path: Option<String>,
    ozy_fpga_path: Option<String>,
    /// Radioberry Juice host program -- path to the executable and last
    /// FPGA choice, same persistence idiom as the two Ozy fields above
    /// (see save_radioberry_juice_config). `juice_launch_error` holds
    /// the message from the last failed launch attempt, if any, shown
    /// inline instead of silently doing nothing on a bad path.
    radioberry_juice_path: Option<String>,
    radioberry_juice_fpga: Option<crate::radioberry_juice::Fpga>,
    juice_launch_error: Option<String>,
    /// Live console handle for the juice process launched from this
    /// window, if any -- cloned into DiscoveryAction::Start so it
    /// survives past this window's own lifetime (see that variant's
    /// doc comment). Only ever set by a successful Launch click, never
    /// persisted/reloaded -- a fresh discovery session always starts
    /// with no console, same as juice itself isn't already running
    /// until the user explicitly launches it here.
    juice_console: Option<crate::radioberry_juice::JuiceHandle>,
    /// Set to "now + JUICE_REFRESH_DELAY" right after a successful
    /// launch (see the Launch button below) so `show()` can trigger one
    /// automatic re-discovery pass once juice has had time to load the
    /// FPGA and come up on the network, without the user having to
    /// remember to click Refresh themselves. `None` the rest of the
    /// time -- a manual Refresh is unaffected either way.
    juice_refresh_at: Option<Instant>,
    /// Automatic juice restarts already followed by a rediscovery (see JuiceHandle::auto_restart_count).
    juice_auto_restarts_seen: u32,
    /// RX-888 Mk2's own user-supplied FX3 RAM image path -- same
    /// "set here, not post-connect Settings" reasoning as the Ozy paths
    /// above (needed just to complete the very first connect). See
    /// `discovery::RX888_SENTINEL_MAC`'s doc comment for why this uses a
    /// different sentinel MAC than Ozy's.
    rx888_firmware_path: Option<String>,
    /// Built-in hardware emulator (see hpsdrsim.rs's own module doc
    /// comment) -- `None` while stopped, matching juice_console's own
    /// "only set while actually running" idiom. `sim_board` is the
    /// user's Metis/HermesLite2 choice, kept even while stopped so it's
    /// remembered for the next Start.
    sim_handle: Option<crate::hpsdrsim::SimHandle>,
    sim_board: crate::hpsdrsim::SimBoard,
    sim_launch_error: Option<String>,
}

/// How long to wait after launching juice before automatically
/// re-running discovery -- long enough to cover USB enumeration + FPGA
/// bitstream load + the board bringing up its network stack, based on
/// the timings noted in BUILD-README.md; a manual Refresh still works
/// immediately if the board takes longer than this on a given machine.
const JUICE_REFRESH_DELAY: Duration = Duration::from_secs(6);

/// Distinct from OZY_CONFIG_MAC -- Radioberry Juice's settings are
/// independent of classic Ozy hardware, so they need their own
/// dedicated Config-file identity rather than sharing Ozy's sentinel
/// (which would silently mix the two unrelated setting sets together).
const RADIOBERRY_JUICE_CONFIG_MAC: [u8; 6] = [0, 0, 0, 0, 0, 1];

/// Sentinel MAC discover_ozy_usb's synthetic `Device` uses (Ozy has no
/// real MAC) -- doubles as a stable, dedicated Config-file identity for
/// Ozy's own global (not per-connect) settings.
const OZY_CONFIG_MAC: [u8; 6] = [0; 6];

impl DiscoveryWindow {
    /// Creates the window and immediately kicks off a background discovery
    /// pass, same as the original GTK dialog did on open.
    pub fn new(ctx: &egui::Context) -> Self {
        let ozy_cfg = Config::load(OZY_CONFIG_MAC);
        let juice_cfg = Config::load(RADIOBERRY_JUICE_CONFIG_MAC);
        // If juice is already running from an earlier session (or was
        // started manually) and we know where its executable lives,
        // adopt it right away -- gives Status/Stop/Restart/Reset USB &
        // Restart immediately, without the user having to notice it's
        // running and take some separate action first. See
        // JuiceHandle::adopt's own doc comment for what "adopted" means
        // (no live console output until/unless a Restart actually
        // relaunches it through this handle).
        let juice_console = juice_cfg
            .radioberry_juice_path
            .as_deref()
            .map(std::path::Path::new)
            .filter(|path| crate::radioberry_juice::is_named_process_running(path))
            .map(crate::radioberry_juice::JuiceHandle::adopt);
        let rx888_cfg = Config::load(crate::discovery::RX888_SENTINEL_MAC);
        // Nothing configured yet (first run, or a fresh profile) -- on
        // Linux/macOS, check the well-known path scripts/build-juice-deb.sh's
        // package (and install-linux.sh directly) installs to, so
        // `apt install radioberry-juice` alone is enough to make this
        // panel usable without ever touching the Choose... file dialog.
        // Windows has no equivalent standard location (see
        // DEFAULT_INSTALLED_PATH's own doc comment), so this is a no-op
        // there.
        let (radioberry_juice_path, radioberry_juice_fpga) = match juice_cfg.radioberry_juice_path {
            Some(path) => (Some(path), juice_cfg.radioberry_juice_fpga),
            None => match detect_default_juice() {
                Some((path, fpga)) => (Some(path), fpga),
                None => (None, None),
            },
        };
        let window = Self {
            open: true,
            devices: Arc::new(Mutex::new(Vec::new())),
            interface_names: Arc::new(Mutex::new(HashMap::new())),
            discovering: Arc::new(Mutex::new(false)),
            selected: None,
            // See config::load_last_manual_ip's own doc comment -- pre-
            // fills instead of leaving this blank every launch, for
            // setups where broadcast discovery never finds the radio
            // (e.g. a direct USB3-LAN link with no proper switched LAN
            // segment for the discovery broadcast to reach).
            manual_ip: crate::config::load_last_manual_ip().unwrap_or_default(),
            manual_error: None,
            focus_deadline: Some(Instant::now() + std::time::Duration::from_millis(1500)),
            size_req: [0.0, 0.0],
            size_attempts: 0,
            size_wait: 0,
            firmware_update: None,
            ozy_firmware_path: ozy_cfg.ozy_firmware_path,
            ozy_fpga_path: ozy_cfg.ozy_fpga_path,
            radioberry_juice_path,
            radioberry_juice_fpga,
            juice_launch_error: None,
            juice_refresh_at: None,
            juice_auto_restarts_seen: 0,
            juice_console,
            rx888_firmware_path: rx888_cfg.rx888_firmware_path,
            sim_handle: None,
            sim_board: crate::hpsdrsim::SimBoard::Metis,
            sim_launch_error: None,
        };
        // Persist an auto-detected path (see above) so it survives even
        // if the user never touches this panel at all this session --
        // a no-op write when nothing changed from what was already saved.
        window.save_radioberry_juice_config();
        window.spawn_discovery(ctx.clone());
        window
    }

    fn save_ozy_paths(&self) {
        let mut cfg = Config::load(OZY_CONFIG_MAC);
        cfg.ozy_firmware_path = self.ozy_firmware_path.clone();
        cfg.ozy_fpga_path = self.ozy_fpga_path.clone();
        cfg.save(OZY_CONFIG_MAC);
    }

    fn save_radioberry_juice_config(&self) {
        let mut cfg = Config::load(RADIOBERRY_JUICE_CONFIG_MAC);
        cfg.radioberry_juice_path = self.radioberry_juice_path.clone();
        cfg.radioberry_juice_fpga = self.radioberry_juice_fpga;
        cfg.save(RADIOBERRY_JUICE_CONFIG_MAC);
    }

    fn save_rx888_path(&self) {
        let mut cfg = Config::load(crate::discovery::RX888_SENTINEL_MAC);
        cfg.rx888_firmware_path = self.rx888_firmware_path.clone();
        cfg.save(crate::discovery::RX888_SENTINEL_MAC);
    }

    fn spawn_discovery(&self, ctx: egui::Context) {
        let devices = Arc::clone(&self.devices);
        let interface_names = Arc::clone(&self.interface_names);
        let discovering = Arc::clone(&self.discovering);
        *discovering.lock().unwrap() = true;
        thread::spawn(move || {
            discover(Arc::clone(&devices), interface_names);
            *discovering.lock().unwrap() = false;
            ctx.request_repaint(); // wake the UI thread once results land
        });
    }

    fn spawn_manual(&self, ctx: egui::Context, ip: IpAddr) {
        let devices = Arc::clone(&self.devices);
        let discovering = Arc::clone(&self.discovering);
        *discovering.lock().unwrap() = true;
        thread::spawn(move || {
            let found = manual_discovery(Arc::clone(&devices), ip);
            *discovering.lock().unwrap() = false;
            if !found {
                eprintln!("manual_discovery: no radio responded at {ip}");
            }
            ctx.request_repaint();
        });
    }

    /// Draw the window for this frame. Call every frame while `open` is true.
    /// Takes `&mut Ui` (not `&Context`) to match eframe 0.35's App::ui model.
    pub fn show(&mut self, ui: &mut egui::Ui) -> DiscoveryAction {
        let mut action = DiscoveryAction::None;
        let mut still_open = self.open;

        // Auto re-discovery after a successful Juice launch -- see
        // juice_refresh_at's own doc comment. Checked once per frame
        // regardless of what's currently shown, so it still fires even
        // if the user has since collapsed the "Radioberry Juice setup"
        // section or switched focus elsewhere within this window.
        // After the automatic stuck-FPGA recovery restarted juice, look for the board again by itself, like after
        // the Launch / Restart / Reset USB buttons.
        if let Some(console) = &self.juice_console {
            let restarts = console.auto_restart_count();
            if restarts != self.juice_auto_restarts_seen {
                self.juice_auto_restarts_seen = restarts;
                self.juice_refresh_at = Some(Instant::now() + JUICE_REFRESH_DELAY);
            }
        }
        if let Some(at) = self.juice_refresh_at {
            let now = Instant::now();
            if now >= at {
                self.juice_refresh_at = None;
                self.spawn_discovery(ui.ctx().clone());
            } else {
                ui.ctx().request_repaint_after(at - now);
            }
        }

        // Dark theme, matching the main window and every other window in
        // the app (a real report: the earlier light/white override here
        // was uncomfortable to read on a bright kiosk LCD, and looked
        // inconsistent next to the rest of the app's own dark styling
        // anyway) -- explicitly set rather than left to inherit, for the
        // same reason as before: egui only tints a window's title bar
        // while focused, so overriding the whole window's visuals is the
        // only way to keep it consistently themed regardless of focus.
        let light_visuals = crate::with_orange_selection(egui::Visuals::dark());
        let light_style = egui::Style { visuals: light_visuals.clone(), ..Default::default() };
        // Rendered in its own OS-level viewport (like the extra receiver
        // windows and the Settings window -- see its doc comment in
        // main.rs) rather than an embedded egui::Window, so it can be
        // dragged outside the main window's bounds. show_viewport_immediate
        // (not _deferred) since this closure borrows `self`/`action`
        // directly by reference rather than through an Arc<Mutex<>>.
        // Always AlwaysOnTop, for the window's whole lifetime -- a real
        // report: it was only pinned above everything for the brief
        // `focus_deadline` grace period below (originally just to win
        // the initial show/raise race against the main window -- see
        // that field's own doc comment), then dropped back to Normal,
        // letting it get buried behind other windows (a terminal, a
        // browser) while discovery is still running/waiting for a
        // selection. `focus_deadline` still separately controls the
        // one-shot keyboard-focus grab just below -- that's a different
        // concern (focus vs. stacking order) that happens to have
        // shared this same window_level gate before.
        let kiosk = crate::lcd_kiosk_mode();
        // AlwaysOnTop for this window's whole lifetime, in kiosk mode too
        // -- a real report: dropping to Normal after the initial
        // `focus_deadline` grace period (the previous behavior here) let
        // the kiosk main window's own fullscreen undecorated viewport
        // reappear on top of this one once the user spent more than that
        // ~1.5s grace period inside it (e.g. expanding a section, picking
        // a radio type), showing as a plain black window since the root
        // only ever renders real content once a radio is connected --
        // exactly the same underlying race `focus_deadline` was already
        // guarding against, just not for its whole lifetime. The
        // tradeoff this used to buy (a native dialog opened from THIS
        // window, e.g. the Radioberry Juice / RX-888 firmware "Choose..."
        // rfd::FileDialog, coming to the front above it) is no longer
        // made in kiosk mode -- if that resurfaces as its own report, it
        // needs a narrower fix than reintroducing this race.
        let window_level = egui::WindowLevel::AlwaysOnTop;
        let _ = kiosk;
        // 1000x580 in kiosk mode (matching the Settings window's own
        // kiosk size, the widest a secondary window gets here) -- a
        // real report: even at the desktop size below, the 7-column
        // device table's trailing Status column still clipped off the
        // right edge on the fixed kiosk window. Plain 900x500 outside
        // kiosk mode, unchanged.
        // 550 high: the window manager adds ~28 px above an undecorated window, so 580 ran off the bottom of the 600 px panel.
        let discovery_size = if kiosk { [1000.0, 550.0] } else { [900.0, 500.0] };
        // Viewport sizes and positions are in points and the window system multiplies them by its own scale
        // (1.33 on the kiosk): a "1000x580" request became a 1333x773 px window, bigger than the 1024x600 panel, so
        // the right and bottom of this screen were off the screen. Ask for the size in pixels divided by that scale.
        let native_scale = if kiosk { ui.ctx().native_pixels_per_point().unwrap_or(1.0).max(0.5) } else { 1.0 };
        let discovery_size_pts = [discovery_size[0] / native_scale, discovery_size[1] / native_scale];
        let mut discovery_viewport = egui::ViewportBuilder::default()
            .with_title("Discover HPSDR Radios")
            // Widened from 700 -- the Interface column now shows the
            // interface name alongside its address (e.g. "eth0
            // (192.168.1.50)"), which combined with the MAC column's
            // own width was pushing the trailing Status column past
            // the fixed window edge and clipping it.
            .with_inner_size(discovery_size_pts)
            .with_active(true)
            .with_window_level(window_level);
        if kiosk {
            // Fixed 1024x600 kiosk mode -- see lcd_kiosk_mode's/
            // kiosk_centered_pos's doc comments in main.rs. Decorations
            // off too -- see the main Settings window's own
            // with_decorations(false) comment for why (native title bar
            // chrome would otherwise push it past the main window's own
            // size); the existing Cancel button below (and Escape,
            // added below) already covers dismissing this window, so no
            // separate on-screen Close is needed here.
            discovery_viewport = discovery_viewport
                .with_position({
                    let p = crate::kiosk_centered_pos(discovery_size);
                    egui::pos2(p[0] / native_scale, p[1] / native_scale)
                })
                .with_max_inner_size(discovery_size_pts)
                .with_resizable(false)
                .with_decorations(false);
        }
        ui.ctx().show_viewport_immediate(
            egui::ViewportId::from_hash_of("discovery_window"),
            discovery_viewport,
            |ui, _class| {
                if kiosk {
                    // The window system scales viewport sizes by a factor that is not always the one egui reports
                    // (1.33 at start, 1.0 after coming back from the radio screen), so the 1000x550 px window came out
                    // 750x412 px. Measure what we got and re-request the size and position corrected by the real factor.
                    if self.size_req == [0.0, 0.0] {
                        self.size_req = discovery_size_pts;
                    }
                    if self.size_wait > 0 {
                        self.size_wait -= 1;
                    } else if self.size_attempts < 4 {
                        if let Some(r) = ui.input(|i| i.viewport().inner_rect) {
                            let actual = [r.width(), r.height()];
                            if actual[0] > 1.0 && (actual[0] - discovery_size[0]).abs() > 3.0 {
                                let s = actual[0] / self.size_req[0].max(1.0);
                                let req = [discovery_size[0] / s, discovery_size[1] / s];
                                let p = crate::kiosk_centered_pos(discovery_size);
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(req[0], req[1])));
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(p[0] / s, p[1] / s)));
                                self.size_req = req;
                                self.size_attempts += 1;
                                self.size_wait = 6;
                            } else {
                                self.size_attempts = 4;
                            }
                        }
                    }
                }
                if let Some(deadline) = self.focus_deadline {
                    let focused = ui.input(|i| i.viewport().focused).unwrap_or(false);
                    if focused || Instant::now() >= deadline {
                        self.focus_deadline = None;
                    } else {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                        ui.ctx().request_repaint();
                    }
                }
                if ui.input(|i| i.viewport().close_requested()) {
                    still_open = false;
                    return;
                }
                if kiosk {
                    // No native title bar in kiosk mode -- Escape maps
                    // to the same Cancel action as the on-screen button
                    // below, since there's no OS close button to fall
                    // back on.
                    let escape_pressed = ui.input(|i| {
                        i.events.iter().any(|ev| {
                            matches!(
                                ev,
                                egui::Event::Key { key: egui::Key::Escape, pressed: true, .. }
                            )
                        })
                    });
                    if escape_pressed {
                        action = DiscoveryAction::Cancelled;
                    }
                }
                egui::CentralPanel::default().frame(egui::Frame::central_panel(&light_style)).show(
                    ui,
                    |ui| {
                ui.visuals_mut().clone_from(&light_visuals);
                if kiosk {
                    // Kiosk: this screen's text is 3 points larger than the default.
                    for font_id in ui.style_mut().text_styles.values_mut() {
                        font_id.size += 3.0;
                    }
                    // Touch: taller targets (rows, buttons, fields) so a finger hits the first time.
                    ui.spacing_mut().interact_size.y = 36.0;
                    ui.spacing_mut().button_padding = egui::vec2(4.0, 7.0);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    // Buttons and fields: rounded corners and a thin line around.
                    let w = &mut ui.visuals_mut().widgets;
                    for st in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                        st.corner_radius = egui::CornerRadius::same(5);
                        st.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(95));
                    }
                }
                let discovering = *self.discovering.lock().unwrap();

                if discovering {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Discovering...");
                    });
                    ui.add_space(8.0);
                }

                let devices_snapshot = self.devices.lock().unwrap().clone();

                // Default to the first AVAILABLE device in the list as
                // soon as results land, so a single radio (the common
                // case) is ready to Start immediately without an extra
                // click. Specifically NOT just index 0 -- a radio
                // already in use by another program (status 3) still
                // shows up in the list, and defaulting to it would
                // select something Start can't actually act on (the
                // button is disabled for non-available rows) instead of
                // a real radio that's actually usable right now. Only
                // fires while nothing is selected yet -- Rediscover
                // explicitly resets `selected` to None (below) so this
                // re-applies to the next batch of results rather than
                // fighting a deliberate user selection.
                if self.selected.is_none() {
                    if let Some(i) = devices_snapshot.iter().position(device_available) {
                        self.selected = Some(i);
                    }
                }

                let list_frame = if kiosk {
                    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95))).corner_radius(6.0).inner_margin(6.0)
                } else {
                    egui::Frame::NONE
                };
                list_frame.show(ui, |ui| {
                egui::Grid::new("discovery_grid")
                    .num_columns(7)
                    .striped(true)
                    .min_col_width(70.0)
                    .show(ui, |ui| {
                        for heading in
                            ["Device", "Interface", "IP", "MAC", "Protocol", "Version", "Status"]
                        {
                            ui.label(egui::RichText::new(heading).strong());
                        }
                        ui.end_row();

                        let interface_names = self.interface_names.lock().unwrap();
                        for (i, dev) in devices_snapshot.iter().enumerate() {
                            let available = device_available(dev);
                            let is_selected = self.selected == Some(i);

                            let mut row_clicked = false;
                            // Double-click on an available row starts it
                            // immediately, same as selecting it then
                            // pressing Start -- OR'd across all cells the
                            // same way row_clicked already is, since each
                            // is its own egui widget/Response.
                            let mut row_double_clicked = false;
                            let resp = selectable_cell(
                                ui,
                                dev.board_label(),
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            // Real interface name (e.g. "eth0") next to
                            // this machine's own address on it -- a real
                            // report that this column, despite its own
                            // "Interface" heading, was only ever showing
                            // the address. Manually-discovered devices
                            // (manual_discovery has no interface concept)
                            // just fall back to the address alone.
                            // Ozy has no real network address at all (it's
                            // USB) -- `dev.address`/`dev.my_address` are just
                            // sentinels for it (see discover_ozy_usb's doc
                            // comment), so show "USB" instead of formatting
                            // them like a real IP/interface.
                            let interface_cell = if matches!(dev.board, Boards::Ozy | Boards::Rx888) {
                                "USB".to_string()
                            } else {
                                match interface_names.get(&dev.my_address.ip()) {
                                    Some(name) => format!("{name} ({})", dev.my_address.ip()),
                                    None => dev.my_address.ip().to_string(),
                                }
                            };
                            let resp = selectable_cell(
                                ui,
                                interface_cell,
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            let ip_cell = if matches!(dev.board, Boards::Ozy | Boards::Rx888) {
                                "USB".to_string()
                            } else {
                                dev.address.ip().to_string()
                            };
                            let resp = selectable_cell(
                                ui,
                                ip_cell,
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            let resp = selectable_cell(
                                ui,
                                format!("{:02X?}", dev.mac),
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            let resp = selectable_cell(
                                ui,
                                dev.protocol.to_string(),
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            let resp = selectable_cell(
                                ui,
                                format!("{}.{}", dev.version / 10, dev.version % 10),
                                is_selected,
                                available,
                            );
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();
                            let status_text = match dev.status {
                                2 => "Available",
                                3 => "In Use",
                                _ => "Unknown",
                            };
                            // See device_available's own doc comment --
                            // discovery now brings the RX-888 up to its
                            // real streaming firmware before reading this,
                            // so a warning here means the row is actually
                            // disabled (device_available returns false),
                            // not just a soft hint -- worded to match.
                            let status_text = match dev.usb_link_speed_warning {
                                Some(warning) => format!("Too slow ({warning})"),
                                None => status_text.to_string(),
                            };
                            let resp = selectable_cell(ui, status_text, is_selected, available);
                            row_clicked |= resp.clicked();
                            row_double_clicked |= resp.double_clicked();

                            if row_clicked {
                                self.selected = Some(i);
                            }
                            if row_double_clicked && available {
                                self.selected = Some(i);
                                action = DiscoveryAction::Start(*dev, self.juice_console.clone(), self.sim_handle.take());
                            }

                            ui.end_row();
                        }
                    });
                });

                // The list stays fixed on top; the rest of the page scrolls (touch).
                let mut rest = |ui: &mut egui::Ui| {
                if devices_snapshot.is_empty() && !discovering {
                    ui.add_space(8.0);
                    ui.weak("No radios found. Try Rediscover or add one manually below.");
                }

                ui.add_space(12.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!discovering, egui::Button::new("Rediscover"))
                        .clicked()
                    {
                        self.selected = None;
                        self.spawn_discovery(ui.ctx().clone());
                    }

                    ui.separator();

                    ui.label("Manual IP:");
                    ui.add_enabled(
                        !discovering,
                        egui::TextEdit::singleline(&mut self.manual_ip).desired_width(120.0),
                    );
                    if ui
                        .add_enabled(!discovering, egui::Button::new("Add"))
                        .clicked()
                    {
                        match self.manual_ip.trim().parse::<IpAddr>() {
                            Ok(ip) => {
                                self.manual_error = None;
                                // See config::save_last_manual_ip's own
                                // doc comment -- remembered regardless of
                                // whether the radio actually answers
                                // below, since a real address that's just
                                // slow to respond (or momentarily
                                // rebooting) is still the one worth
                                // pre-filling next launch.
                                crate::config::save_last_manual_ip(&ip.to_string());
                                self.spawn_manual(ui.ctx().clone(), ip);
                            }
                            Err(_) => {
                                self.manual_error = Some("Invalid IP address".to_string());
                            }
                        }
                    }
                });

                if let Some(err) = &self.manual_error {
                    ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
                }

                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    let can_start = self
                        .selected
                        .and_then(|i| devices_snapshot.get(i))
                        .map(device_available)
                        .unwrap_or(false);

                    if ui.add_enabled(can_start, egui::Button::new("Start")).clicked() {
                        if let Some(dev) = self.selected.and_then(|i| devices_snapshot.get(i)) {
                            action = DiscoveryAction::Start(*dev, self.juice_console.clone(), self.sim_handle.take());
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        action = DiscoveryAction::Cancelled;
                    }

                    ui.separator();

                    if ui.button("Firmware Update...").on_hover_text(
                        "Update FPGA firmware or change the static IP of a radio in bootloader mode \
                         (Metis/Hermes/Hermes2/Angelia/Orion/Orion2). The radio must already be \
                         physically switched into bootloader mode and power-cycled.",
                    ).clicked() {
                        self.firmware_update = Some(FirmwareUpdateWindow::new_raw_ethernet());
                    }

                    // Kiosk mode only -- see lcd_kiosk_mode's doc comment
                    // in main.rs. On a normal desktop the native title
                    // bar's own close button already quits the app; in
                    // kiosk mode there IS no native title bar (see this
                    // window's own with_decorations(false) above) or
                    // taskbar-friendly way to reach one, so this is the
                    // only way to quit at all. Deliberately placed HERE
                    // (the Discover window) rather than on the main
                    // Connected window's Stop button: this is also where
                    // the Radioberry Juice setup section's own Stop
                    // button lives (just below, when a juice console is
                    // attached), so someone shutting the whole thing down
                    // sees both controls together instead of a one-click
                    // Exit on the main window silently leaving juice
                    // running in the background.
                    if kiosk {
                        ui.separator();
                        // See crate::kiosk_accent_button's own doc
                        // comment -- same attention-grabbing treatment
                        // as this app's other kiosk-only window-chrome
                        // controls (STOP/SETTINGS/MIN/CLOSE).
                        if crate::kiosk_accent_button(ui, "EXIT")
                            .on_hover_text(
                                "Quit hpsdr-rs. If a Radioberry Juice process is running (see \
                                 below), stop it first if you don't want it left running in \
                                 the background.",
                            )
                            .clicked()
                        {
                            std::process::exit(0);
                        }
                    }
                });

                if let Some(fw) = &mut self.firmware_update {
                    fw.show(ui);
                    if !fw.open {
                        self.firmware_update = None;
                    }
                }

                ui.add_space(8.0);
                egui::CollapsingHeader::new("Ozy USB setup").show(ui, |ui| {
                    ui.label(
                        "Classic Ozy/Mercury/Penny hardware needs these two files \
                         to connect. hpsdr-rs bundles its own copies (sourced from \
                         piHPSDR) -- only use Choose... below to override with a \
                         different/custom build.",
                    );
                    ui.horizontal(|ui| {
                        ui.label("FX2 firmware (.hex):");
                        ui.label(effective_path_label(&self.ozy_firmware_path, crate::ozy::default_firmware_path));
                        if ui.button("Choose...").clicked() {
                            if let Some(path) =
                                rfd::FileDialog::new().add_filter("Ozy FX2 firmware", &["hex"]).pick_file()
                            {
                                self.ozy_firmware_path = Some(path.display().to_string());
                                self.save_ozy_paths();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("FPGA bitstream (.rbf):");
                        ui.label(effective_path_label(&self.ozy_fpga_path, crate::ozy::default_fpga_path));
                        if ui.button("Choose...").clicked() {
                            if let Some(path) =
                                rfd::FileDialog::new().add_filter("FPGA firmware", &["rbf"]).pick_file()
                            {
                                self.ozy_fpga_path = Some(path.display().to_string());
                                self.save_ozy_paths();
                            }
                        }
                    });
                });

                ui.add_space(8.0);
                // The help text is a tooltip on the header (three lines of it under the header got in the way every time).
                let juice_header = egui::CollapsingHeader::new("Radioberry Juice setup").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Juice executable:");
                        ui.label(
                            self.radioberry_juice_path
                                .as_deref()
                                .unwrap_or("(not set -- use Choose... below)"),
                        );
                        if ui.button("Choose...").clicked() {
                            // Windows Juice builds are always named
                            // *.exe, so filtering to that extension is a
                            // real help there. Linux/macOS executables
                            // have no fixed extension (e.g. the plain
                            // `radioberry-juice` built by
                            // scripts/build-juice-linux.sh) -- an empty
                            // string isn't "any extension" to rfd, it's
                            // a literal (unmatchable) one, which made the
                            // binary itself unselectable in the dialog.
                            // Leaving the filter off there just shows
                            // every file, same as the dialog's default.
                            let mut dialog = rfd::FileDialog::new();
                            if cfg!(windows) {
                                dialog = dialog.add_filter("Radioberry Juice", &["exe"]);
                            }
                            if let Some(existing) = &self.radioberry_juice_path {
                                if let Some(dir) = std::path::Path::new(existing).parent() {
                                    dialog = dialog.set_directory(dir);
                                }
                            } else {
                                // Nothing chosen yet -- pre-fill the expected filename so
                                // the dialog opens ready to just navigate to the right
                                // folder, rather than an empty file-name field.
                                dialog = dialog.set_file_name(crate::radioberry_juice::DEFAULT_EXE_NAME);
                            }
                            if let Some(path) = dialog.pick_file() {
                                self.radioberry_juice_path = Some(path.display().to_string());
                                // A freshly-chosen executable may already have its own
                                // radioberry.props (e.g. from a previous manual setup) --
                                // reflect whatever FPGA it's currently set to rather than
                                // silently keeping a stale in-memory value from a
                                // different executable/board.
                                let props_path = crate::radioberry_juice::props_path_for(&path);
                                self.radioberry_juice_fpga = crate::radioberry_juice::read_fpga(&props_path);
                                self.juice_launch_error = None;
                                // Same reasoning as new()'s own adoption check -- a
                                // freshly-chosen executable might already be running
                                // (e.g. the user picked the exe of a juice they started
                                // by hand, or that's left over from an earlier session).
                                if crate::radioberry_juice::is_named_process_running(&path) {
                                    self.juice_console = Some(crate::radioberry_juice::JuiceHandle::adopt(&path));
                                }
                                self.save_radioberry_juice_config();
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("FPGA:");
                        let path_set = self.radioberry_juice_path.is_some();
                        for fpga in crate::radioberry_juice::Fpga::ALL {
                            let selected = self.radioberry_juice_fpga == Some(fpga);
                            if ui
                                .add_enabled(path_set, egui::RadioButton::new(selected, fpga.to_string()))
                                .clicked()
                            {
                                self.radioberry_juice_fpga = Some(fpga);
                                self.save_radioberry_juice_config();
                                if let Some(exe) = &self.radioberry_juice_path {
                                    let props_path =
                                        crate::radioberry_juice::props_path_for(std::path::Path::new(exe));
                                    if let Err(e) = crate::radioberry_juice::set_fpga(&props_path, fpga) {
                                        self.juice_launch_error =
                                            Some(format!("Couldn't write {}: {e}", props_path.display()));
                                    } else {
                                        self.juice_launch_error = None;
                                    }
                                }
                            }
                        }
                        if !path_set {
                            ui.label("(choose the executable first)");
                        }
                    });

                    if ui
                        .add_enabled(
                            self.radioberry_juice_path.is_some(),
                            egui::Button::new("Launch Radioberry Juice"),
                        )
                        .on_hover_text(
                            "Starts juice in the background. Once it's up and has \
                             loaded the FPGA gateware, use Refresh above to discover \
                             the board like any other radio.",
                        )
                        .clicked()
                    {
                        if let Some(exe) = &self.radioberry_juice_path {
                            match crate::radioberry_juice::launch(std::path::Path::new(exe)) {
                                Ok(handle) => {
                                    self.juice_launch_error = None;
                                    self.juice_console = Some(handle);
                                    self.juice_refresh_at = Some(Instant::now() + JUICE_REFRESH_DELAY);
                                    ui.ctx().request_repaint_after(JUICE_REFRESH_DELAY);
                                }
                                Err(e) => self.juice_launch_error = Some(format!("Couldn't launch juice: {e}")),
                            }
                        }
                    }

                    if let Some(handle) = &self.juice_console {
                        ui.horizontal(|ui| {
                            let running = handle.is_running();
                            ui.label(if running { "Status: running" } else { "Status: stopped" });
                            if ui.add_enabled(running, egui::Button::new("Stop")).clicked() {
                                handle.stop();
                            }
                            if ui
                                .button("Restart")
                                .on_hover_text(
                                    "Kills juice if it's stuck or unresponsive and starts it \
                                     again -- an alternative to unplugging the USB cable. Note: \
                                     if a radio is currently connected through this juice \
                                     instance, restarting it will drop that connection; you'll \
                                     need to reconnect from Discover once juice is back up.",
                                )
                                .clicked()
                            {
                                if let Err(e) = handle.restart() {
                                    self.juice_launch_error = Some(format!("Couldn't restart juice: {e}"));
                                } else {
                                    self.juice_launch_error = None;
                                    self.juice_refresh_at = Some(Instant::now() + JUICE_REFRESH_DELAY);
                                    ui.ctx().request_repaint_after(JUICE_REFRESH_DELAY);
                                }
                            }
                            if ui
                                .button("Reset USB & Restart")
                                .on_hover_text(if cfg!(windows) {
                                    "For when a plain Restart doesn't unstick it. Disables and \
                                     re-enables the Radioberry's USB device in Windows -- the \
                                     same effect as unplugging and replugging the cable, without \
                                     touching it -- then relaunches juice. Requires running \
                                     hpsdr-rs as Administrator."
                                } else {
                                    "For when a plain Restart doesn't unstick it. Issues a USB \
                                     port reset to the Radioberry -- the same effect as \
                                     unplugging and replugging the cable, without touching it -- \
                                     then relaunches juice. Uses the same USB device permissions \
                                     juice itself already needs, no extra privileges required."
                                })
                                .clicked()
                            {
                                if let Err(e) = handle.reset_usb_and_restart() {
                                    self.juice_launch_error = Some(format!("Couldn't reset USB device: {e}"));
                                } else {
                                    self.juice_launch_error = None;
                                    self.juice_refresh_at = Some(Instant::now() + JUICE_REFRESH_DELAY);
                                    ui.ctx().request_repaint_after(JUICE_REFRESH_DELAY);
                                }
                            }
                            // Windows-only: elevation (UAC/"Run as
                            // Administrator") is a Windows-specific
                            // concept -- relaunch_elevated/is_elevated
                            // are both no-ops elsewhere (see their own
                            // doc comments), and Reset USB above needs
                            // no special privileges on Linux/macOS in
                            // the first place, so there's nothing this
                            // button could ever fix on those platforms.
                            if cfg!(windows) {
                                // Quiet, always-available option rather than an alarmist banner --
                                // the graceful shutdown path above needs no special privileges at
                                // all (a process closing itself never does), so most people will
                                // never actually need this. It only matters for the rare fallback
                                // case (a stuck juice that has to be force-killed, or Reset USB),
                                // where Windows can silently refuse without it -- the console
                                // already says so, reactively, exactly if/when that happens.
                                if ui
                                    .button("Run as Administrator")
                                    .on_hover_text(
                                        "Only needed if Stop/Reset USB ever fails with a permissions \
                                         error -- most people never hit this.",
                                    )
                                    .clicked()
                                {
                                    if let Err(e) = crate::radioberry_juice::relaunch_elevated() {
                                        self.juice_launch_error = Some(format!("Couldn't relaunch elevated: {e}"));
                                    } else {
                                        std::process::exit(0);
                                    }
                                }
                            }
                        });
                    }

                    if let Some(err) = &self.juice_launch_error {
                        ui.colored_label(egui::Color32::RED, err);
                    }

                    if let Some(console) = &self.juice_console {
                        ui.add_space(4.0);
                        ui.label("Juice output:");
                        let log_text = console.snapshot().join("\n");
                        if kiosk {
                            egui::Frame::NONE
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(95)))
                                .corner_radius(6.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    touch_scroll(ui, "juice_log", Some(120.0), true, &mut |ui: &mut egui::Ui| {
                                        // The log keeps the original size, not the larger one of the rest of this screen.
                                        if let Some(f) = ui.style_mut().text_styles.get_mut(&egui::TextStyle::Monospace) {
                                            f.size -= 3.0;
                                        }
                                        ui.add(
                                            egui::TextEdit::multiline(&mut log_text.as_str())
                                                .desired_width(f32::INFINITY)
                                                .font(egui::TextStyle::Monospace)
                                                .frame(egui::Frame::NONE)
                                                .interactive(false),
                                        );
                                    });
                                });
                        } else {
                            egui::ScrollArea::vertical().max_height(120.0).stick_to_bottom(true).show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut log_text.clone())
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace)
                                        .interactive(false),
                                );
                            });
                        }
                        // Keeps this panel live-updating while juice is
                        // producing output, without needing the reader
                        // threads themselves to know about egui at all
                        // -- a plain periodic repaint is simpler and
                        // cheap enough for a console view like this.
                        ui.ctx().request_repaint_after(Duration::from_millis(300));
                    }
                });
                // The help is a tooltip on the header (it used to be three lines under it, in the way every time).
                juice_header.header_response.on_hover_text(
                    "Radioberry boards are driven by a separate program, \"Juice\"\n\
                     (radioberry-juice), which talks to the board over USB and then\n\
                     exposes it here over normal openHPSDR discovery, same as any\n\
                     Metis/Hermes-family board once it's running.\n\
                     \n\
                     Point this at your built juice executable, launch it, and pick\n\
                     the FPGA variant fitted to your board.",
                );

                egui::CollapsingHeader::new("RX-888 USB setup").show(ui, |ui| {
                    ui.label(
                        "RX-888 Mk2 needs its own Cypress FX3 RAM image \
                         (SDDC_FX3.img) to connect -- bundled with hpsdr-rs \
                         (MIT-licensed, see assets/rx888/PROVENANCE.md), so \
                         no setup is needed unless you want to point it at \
                         your own copy instead.",
                    );
                    ui.horizontal(|ui| {
                        ui.label("FX3 firmware (.img):");
                        ui.label(effective_path_label(&self.rx888_firmware_path, crate::rx888::default_firmware_path));
                        if ui.button("Choose...").clicked() {
                            if let Some(path) =
                                rfd::FileDialog::new().add_filter("RX-888 FX3 firmware", &["img"]).pick_file()
                            {
                                self.rx888_firmware_path = Some(path.display().to_string());
                                self.save_rx888_path();
                            }
                        }
                    });
                });

                // Built-in hardware emulator -- see hpsdrsim.rs's own
                // module doc comment. A real request: lets the RX/
                // discovery/connect pipeline be tested with no radio
                // attached at all, similar in spirit to piHPSDR's
                // separate hpsdrsim command-line tool, but built in
                // here instead of a standalone program.
                //
                // Hidden entirely (not even this collapsible header)
                // unless HPSDR_SIM=1 -- see hpsdrsim_enabled's own doc
                // comment. A real request: this is purely a development/
                // testing aid, unrelated to and must not visibly
                // intrude on this app's normal operation with real
                // hardware -- same opt-in-only treatment as kiosk mode's
                // own HPSDR_LCD_1024X600.
                if crate::hpsdrsim::hpsdrsim_enabled() {
                egui::CollapsingHeader::new("hpsdrsim").show(ui, |ui| {
                    ui.label(
                        "Runs an in-process fake radio that answers \
                         discovery and streams a synthetic RX signal (a \
                         fixed tone + light noise), so the app can be \
                         connected to and tested with no real hardware \
                         attached. Choose which board it pretends to be \
                         -- the wire commands genuinely differ between \
                         them -- then Start.",
                    );
                    ui.horizontal(|ui| {
                        let running = self.sim_handle.as_ref().is_some_and(|h| h.is_running());
                        ui.add_enabled_ui(!running, |ui| {
                            ui.radio_value(&mut self.sim_board, crate::hpsdrsim::SimBoard::Metis, "Metis");
                            ui.radio_value(
                                &mut self.sim_board,
                                crate::hpsdrsim::SimBoard::HermesLite2,
                                "HermesLite2",
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        let running = self.sim_handle.as_ref().is_some_and(|h| h.is_running());
                        if ui.add_enabled(!running, egui::Button::new("Start")).clicked() {
                            self.sim_launch_error = None;
                            match crate::hpsdrsim::SimHandle::start(self.sim_board) {
                                Ok(handle) => self.sim_handle = Some(handle),
                                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                                    // WSAEACCES (os error 10013) on this
                                    // exact port -- a real conflict seen
                                    // this session: radioberry-juice-x64
                                    // (the Radioberry bridge) already
                                    // holds UDP 1024 exclusively whenever
                                    // it's running, since it's a real P1
                                    // "device" on the wire in exactly the
                                    // same way this emulator is. Only one
                                    // of them can own the port at a time.
                                    self.sim_launch_error = Some(
                                        "couldn't start hpsdrsim: UDP port 1024 is already in \
                                         use -- likely by Radioberry Juice (or another hpsdr-rs \
                                         instance) already running. Stop that first, then Start \
                                         again."
                                            .to_string(),
                                    );
                                }
                                Err(e) => {
                                    self.sim_launch_error =
                                        Some(format!("couldn't start hpsdrsim: {e}"));
                                }
                            }
                        }
                        if ui.add_enabled(running, egui::Button::new("Stop")).clicked() {
                            self.sim_handle = None;
                        }
                        ui.label(if running { "Status: running" } else { "Status: stopped" });
                    });
                    if let Some(err) = &self.sim_launch_error {
                        ui.colored_label(egui::Color32::RED, err);
                    }
                });
                }
                };
                if kiosk {
                    // Touch: everything under the device list scrolls, with a wide bar and up/down buttons on the right.
                    touch_scroll(ui, "discovery_page", None, false, &mut rest);
                } else {
                    rest(ui);
                }
                });
            },
        );
        self.open = still_open;

        if !still_open {
            action = DiscoveryAction::Cancelled;
        }

        action
    }
}
