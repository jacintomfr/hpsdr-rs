/*
    Persists user-adjustable settings (mode, filter width, gain, AGC
    mode + tuning, spectrum display range, waterfall palette, last
    tuned frequency) across restarts as a small JSON file.

    One config file per radio, named after its MAC address, so
    different physical radios keep independent saved settings rather
    than sharing/overwriting one config.
*/

use crate::spectrum::{Agc, EqualizerParams, Mode, NoiseBlanker, NoiseReduction};
use crate::{BandSettings, MeterStyle, Palette};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub frequency_hz: Option<u32>,
    pub sample_rate: Option<u32>,
    pub mode: Option<Mode>,
    pub width_hz: Option<f64>,
    pub gain: Option<f32>,
    /// Squelch (slider 0..=100, enabled) per mode label, like piHPSDR's per-mode RXTXprofile squelch.
    #[serde(default)]
    pub squelch_memory: std::collections::HashMap<String, (f32, bool)>,
    /// DUP (duplex) switch, like deskHPSDR's "duplex" property.
    pub duplex: Option<bool>,
    /// Digital Modes (RTTY) center/baud/shift/reverse/afc -- see
    /// rtty_link::RttySettings's own doc comment. None/missing (configs
    /// saved before this existed) falls back to RttySettings::default().
    pub rtty: Option<crate::rtty_link::RttySettings>,
    /// The callsign RADE V1 transmits in its End-of-Over frame -- see
    /// rade_link.rs's own doc comment. None/missing/empty means no
    /// callsign is sent (a well-formed but identity-less over).
    #[serde(default)]
    pub rade_callsign: Option<String>,
    /// Your Callsign / Your Locator (Settings -> SDR Device): one value for RADE, SSTV, RTTY... Old configs carry the
    /// callsign in `rade_callsign` (read as a fallback).
    #[serde(default)]
    pub own_callsign: Option<String>,
    #[serde(default)]
    pub own_locator: Option<String>,
    /// RADE's own mic conditioning (Noise Reduction/Leveler/Compressor,
    /// render_rade_panel) -- see tx::TxParams::rade_denoiser_enabled's
    /// doc comment for why these are separate from tx_denoiser_enabled/
    /// tx_leveler_enabled/tx_compressor_enabled above.
    #[serde(default)]
    pub rade_denoiser_enabled: Option<bool>,
    #[serde(default)]
    pub rade_leveler_enabled: Option<bool>,
    #[serde(default)]
    pub rade_compressor_enabled: Option<bool>,
    /// RADE mic Bass/Mid/Treble/Vol equalizer -- see
    /// tx::TxParams::rade_eq_enabled's doc comment.
    #[serde(default)]
    pub rade_eq_enabled: Option<bool>,
    #[serde(default)]
    pub rade_eq: Option<crate::rade_mic_agc::RadeEqParams>,
    /// RADE panel's "Mute start/end" -- see rade_link::RadeHandle::mute_edges.
    #[serde(default)]
    pub rade_mute_edges: Option<bool>,
    /// Digital window state (all were reset on every launch): RADE "Mute
    /// Analog" and the "Fit Filter" width toggle, the RTTY/SSTV/RADE tab
    /// (0/1/2), RTTY "Send on Return", and the SSTV RX/TX options
    /// (`sstv_rx_mode`: index into SstvMode::ALL, -1 = Auto).
    #[serde(default)]
    pub rade_mute_analog: Option<bool>,
    #[serde(default)]
    pub rade_filter_wide: Option<bool>,
    /// FreeDV Reporter (qso.freedv.org) client enabled; default off.
    #[serde(default)]
    pub freedv_reporter_enabled: Option<bool>,
    #[serde(default)]
    pub digital_mode: Option<u8>,
    #[serde(default)]
    pub rtty_send_on_return: Option<bool>,
    #[serde(default)]
    pub sstv_rx_mode: Option<i32>,
    #[serde(default)]
    pub sstv_rx_auto_save: Option<bool>,
    #[serde(default)]
    pub sstv_tx_mode: Option<usize>,
    #[serde(default)]
    pub sstv_tx_banner: Option<bool>,
    #[serde(default)]
    pub sstv_tx_ppm: Option<f32>,
    #[serde(default)]
    pub sstv_tx_fsk_id: Option<bool>,
    #[serde(default)]
    pub sstv_tx_lead_ms: Option<u32>,
    /// Output device for local RX audio playback (Settings -> Audio's
    /// "Output device" picker), by name -- e.g. "CABLE Input (VB-Audio
    /// Virtual Cable)" to feed a decoder instead of/alongside real
    /// speakers. `None`/missing (configs saved before this existed)
    /// falls back to the system default, same as this always did before
    /// device selection existed. An unrecognized name (the saved device
    /// no longer present on this machine) also falls back to the
    /// default rather than erroring -- see AudioOutput::start's doc
    /// comment.
    #[serde(default)]
    pub audio_output_device: Option<String>,
    /// Input device for TX mic audio (Settings -> Audio's "Input device"
    /// picker), by name -- e.g. "CABLE Output (VB-Audio Virtual Cable)"
    /// to feed TX audio from a virtual cable instead of/alongside a real
    /// mic. Same `None`/unrecognized-name fallback-to-default contract
    /// as `audio_output_device` above -- see MicInput::start's doc
    /// comment.
    #[serde(default)]
    pub mic_input_device: Option<String>,
    /// See spectrum::cw_pitch_hz's doc comment -- a single global
    /// setting (Settings -> CW), not per-receiver. `None` (a config
    /// saved before this existed) falls back to the 600Hz default.
    #[serde(default)]
    pub cw_pitch_hz: Option<f64>,
    pub agc: Option<Agc>,
    pub agc_attack_ms: Option<i32>,
    pub agc_decay_ms: Option<i32>,
    pub agc_hang_ms: Option<i32>,
    #[serde(default)]
    pub agc_hang_threshold: Option<i32>,
    pub agc_top_db: Option<f64>,
    pub agc_slope_db: Option<i32>,
    /// See spectrum::DemodParams::meter_calibration_db's own doc
    /// comment. `#[serde(default)]` since older saved configs predate
    /// this field -- missing = 0.0/uncalibrated, the pre-existing
    /// behavior, not a guessed correction.
    #[serde(default)]
    pub meter_calibration_db: Option<f64>,
    pub db_low: Option<f32>,
    pub db_high: Option<f32>,
    /// "Auto" mode for db_low -- see ConnectedState::db_low_auto's doc
    /// comment (main.rs). Missing (a config saved before this existed)
    /// falls back to on -- the default for this feature.
    #[serde(default)]
    pub db_low_auto: Option<bool>,
    pub waterfall_db_low: Option<f32>,
    pub waterfall_db_high: Option<f32>,
    /// "Auto" mode for the Waterfall Low slider -- see
    /// ConnectedState::waterfall_db_low_auto's doc comment. Missing
    /// (a pre-existing config saved before this was added) defaults to
    /// off, not on, so nobody's waterfall behaviour changes on upgrade
    /// without them opting in.
    pub waterfall_db_low_auto: Option<bool>,
    /// "AGC Auto" -- see ConnectedState::agc_auto's own doc comment.
    /// Missing defaults to off, matching deskHPSDR's own default.
    #[serde(default)]
    pub agc_auto: Option<bool>,
    #[serde(default)]
    pub agc_auto_offset_db: Option<f32>,
    pub waterfall_palette: Option<Palette>,
    /// See ConnectedState::meter_style's own doc comment (main.rs).
    /// Missing (a config saved before this existed) falls back to
    /// Analog, this project's original S-meter style.
    #[serde(default)]
    pub meter_style: Option<MeterStyle>,
    /// S-meter reading (peak / average) and TX ALC reading (peak / average / gain), deskHPSDR's meter menu.
    #[serde(default)]
    pub smeter_mode: Option<crate::SMeterMode>,
    #[serde(default)]
    pub alc_mode: Option<crate::AlcMode>,
    /// Spectrum's share (0.0-1.0) of the combined spectrum+waterfall
    /// height -- draggable via the divider between them. Missing falls
    /// back to their old fixed 150/350 proportions.
    pub spectrum_waterfall_ratio: Option<f32>,
    /// Whether the waterfall is drawn at all (Settings -> Spectrum) --
    /// see ConnectedState::waterfall_enabled's doc comment. Missing
    /// (a config saved before this existed) falls back to on, this
    /// feature's default.
    #[serde(default)]
    pub waterfall_enabled: Option<bool>,
    /// Spectrum/waterfall zoom/pan -- see ConnectedState::spectrum_zoom/
    /// spectrum_pan's doc comments (main.rs). Missing (a config saved
    /// before this existed) falls back to zoom 1 / pan 0.0, i.e. the
    /// full sample-rate span, unzoomed -- this feature's own "off" state.
    #[serde(default)]
    pub spectrum_zoom: Option<i32>,
    #[serde(default)]
    pub spectrum_pan: Option<f32>,
    pub adc: Option<u8>,
    /// Legacy single global antenna value, from before per-band RX/TX
    /// antenna existed (see antenna_settings below) -- kept only as a
    /// one-time migration source (see its load site in main.rs) for
    /// configs saved before that. No longer written on save.
    pub antenna: Option<u8>,
    pub rigctl_addr: Option<String>,
    pub tci_addr: Option<String>,
    /// Kenwood TS-2000 CAT emulation address -- see cat.rs's module doc
    /// comment. Same `#[serde(default)]`/missing-means-off treatment as
    /// rigctl_running/tci_running below (this field predates their
    /// addition, so it can't share their attribute, but the same
    /// reasoning applies: a config saved before CAT existed has no
    /// value here).
    #[serde(default)]
    pub cat_addr: Option<String>,
    /// "Mute local audio output while TCI is running" (Settings ->
    /// Network) -- see radio::RadioSession::mute_local_audio_for_tci's
    /// doc comment. Missing/false means unchanged behavior (both local
    /// audio and TCI stream, as before this option existed).
    #[serde(default)]
    pub mute_local_audio_during_tci: Option<bool>,
    /// Whether rigctl/TCI/CAT were running at last save -- since starting
    /// them is now a manual action (Settings -> Network) rather than
    /// automatic, this is what lets a reconnect restore "was running"
    /// state instead of always coming back stopped. `None`/missing
    /// (e.g. configs saved before this existed) is treated as "was
    /// not running", matching the old default-off behavior.
    #[serde(default)]
    pub rigctl_running: Option<bool>,
    #[serde(default)]
    pub tci_running: Option<bool>,
    #[serde(default)]
    pub cat_running: Option<bool>,
    /// Debug logging to rigctl_log.txt/tci_log.txt/cat_log.txt (Settings
    /// -> Network) -- see debug_log.rs's own doc comment. Off by default
    /// (missing = `false`), same reasoning as every other Option<bool>
    /// here: a config saved before this existed shouldn't suddenly start
    /// logging.
    #[serde(default)]
    pub rigctl_logging_enabled: Option<bool>,
    #[serde(default)]
    pub tci_logging_enabled: Option<bool>,
    #[serde(default)]
    pub cat_logging_enabled: Option<bool>,
    /// Noise blanker (NB/NB2, mutually exclusive) and noise reduction
    /// (NR/NR2, mutually exclusive) state -- see the field docs on
    /// spectrum::DemodParams and the NoiseBlanker/NoiseReduction enums
    /// for what each one actually does.
    pub noise_blanker: Option<NoiseBlanker>,
    pub nb_threshold: Option<f64>,
    pub noise_reduction: Option<NoiseReduction>,
    /// See spectrum::DemodParams::nnr_mask_floor_db's doc comment.
    /// Missing falls back to WDSP's own documented default (-25.0).
    #[serde(default)]
    pub nnr_mask_floor_db: Option<f64>,
    /// See spectrum::DemodParams::nnr_premium's doc comment.
    #[serde(default)]
    pub nnr_premium: Option<bool>,
    /// SNB ("Spectral Noise Blanker") -- independent of noise_reduction
    /// above, see spectrum::DemodParams::snb's doc comment for why.
    pub snb: Option<bool>,
    /// ANF ("Automatic Notch Filter") -- see spectrum::DemodParams::anf's
    /// doc comment.
    pub anf: Option<bool>,
    /// Binaural ("phasing") RX audio -- see spectrum::DemodParams::
    /// binaural's doc comment.
    pub binaural: Option<bool>,
    /// Main receiver's graphic EQ -- see spectrum::EqualizerParams's doc
    /// comment. Each extra receiver window persists its own copy
    /// separately, see ExtraReceiverConfig::eq.
    pub rx_eq: Option<EqualizerParams>,
    /// TX mic gain. Deliberately one of very few TX settings persisted
    /// -- whether TX was armed (tx_enabled) is intentionally NOT saved
    /// (see main.rs's auto-arm-on-connect comment).
    pub mic_gain: Option<f32>,
    /// TX graphic EQ -- see spectrum::EqualizerParams's doc comment.
    pub tx_eq: Option<EqualizerParams>,
    /// Per-mode-group RX EQ (key = eq_profiles::eq_mode_group); `rx_eq` stays the live copy.
    /// Settings -> Noise (deskHPSDR noise menu): NR2/NB/NR4/notch parameters.
    #[serde(default)]
    pub noise_extra: Option<crate::spectrum::NoiseExtra>,
    #[serde(default)]
    pub rx_eq_by_mode: std::collections::HashMap<String, EqualizerParams>,
    /// Per-mode-group TX EQ (key = eq_profiles::eq_mode_group); `tx_eq` stays the live copy.
    #[serde(default)]
    pub tx_eq_by_mode: std::collections::HashMap<String, EqualizerParams>,
    /// Active mic-profile slot, 0..=2 (deskHPSDR `mic_prof.nr`; file `audio_profile_<n>.prop`, see
    /// eq_profiles). Values > 2 (old 1..=3 numbering is NOT converted) are ignored, see `active_mic_profile()`.
    #[serde(default)]
    pub mic_profile_nr: Option<usize>,
    /// Descriptions of the 3 mic-profile slots (deskHPSDR: the mic input device description at Save time).
    #[serde(default)]
    pub mic_profile_descs: Vec<String>,
    /// UI-only / persisted-live TX options of the Settings -> TX window.
    #[serde(default)]
    pub tx_ui: TxUiExtra,
    /// Filter menu: edited Var1/Var2 edges ([low1, high1, low2, high2]) and the chosen filter (0..14 fixed, 15 Var1, 16 Var2) per mode.
    #[serde(default)]
    pub filter_vars: std::collections::HashMap<String, [i32; 4]>,
    #[serde(default)]
    pub filter_sel: std::collections::HashMap<String, usize>,
    /// Settings -> RX Menu options (see RxUiExtra).
    #[serde(default)]
    pub rx_ui: RxUiExtra,
    /// WDSP Leveler/Compressor ("PROC") on-off and gain -- see
    /// tx::TxParams::leveler_enabled/compressor_enabled's doc comments.
    #[serde(default)]
    pub tx_leveler_enabled: Option<bool>,
    #[serde(default)]
    pub tx_leveler_gain_db: Option<f32>,
    #[serde(default)]
    pub tx_leveler_decay_ms: Option<i32>,
    #[serde(default)]
    pub tx_compressor_enabled: Option<bool>,
    #[serde(default)]
    pub tx_compressor_gain_db: Option<f32>,
    #[serde(default)]
    pub tx_cfc_enabled: Option<bool>,
    /// RNNoise TX noise reduction -- see tx::TxParams::tx_denoiser_enabled's
    /// doc comment. Ordinary analog voice only, separate from RADE's own
    /// (rade_denoiser_enabled below).
    #[serde(default)]
    pub tx_denoiser_enabled: Option<bool>,
    /// Gain applied specifically to TX audio received from a TCI
    /// client (WSJT-X, TCI Remote, etc.), independent of mic_gain
    /// above -- see radio::RadioSession::tci_tx_gain's doc comment for
    /// why a real test needed these decoupled (WSJT-X's own TCI audio
    /// arrived roughly 700x quieter than mic_gain's range is
    /// calibrated for). Defaults to 1.0 (unchanged behavior) when unset.
    #[serde(default)]
    pub tci_tx_gain: Option<f32>,
    /// TX power target in watts, both protocols -- see
    /// radio::drive_byte_for_watts for how this becomes each protocol's
    /// actual wire-level drive byte. See mic_gain's note -- same
    /// reasoning for persisting this despite TX arming itself not being
    /// saved.
    pub tx_power_watts: Option<u32>,
    /// CW keyer settings (Settings -> CW) for the radio's own built-in/
    /// internal keyer -- see radio::CwKeyerAtomics's doc comment for
    /// defaults/ranges. `None` (a config saved before this existed)
    /// falls back to those same defaults.
    #[serde(default)]
    pub cw_keyer_mode: Option<u32>,
    #[serde(default)]
    pub cw_keyer_speed_wpm: Option<u32>,
    #[serde(default)]
    pub cw_keyer_weight: Option<u32>,
    #[serde(default)]
    pub cw_keyer_sidetone_volume: Option<u32>,
    #[serde(default)]
    pub cw_keyer_sidetone_freq_hz: Option<u32>,
    #[serde(default)]
    pub cw_keyer_hang_time_ms: Option<u32>,
    /// See audio::CwSidetone's doc comment -- separate, additive PC-
    /// side software sidetone (opt-in, off by default) alongside the
    /// radio's own internal-keyer sidetone above. `None`/missing
    /// (a config saved before this existed) falls back to off.
    #[serde(default)]
    pub cw_pc_sidetone_enabled: Option<bool>,
    /// Up to 5 saved CW text messages (Settings -> CW), sent via the
    /// main window's Send CW control at whatever speed/weight is
    /// currently set above -- see tx::TxHandle::send_cw_text's doc
    /// comment for how sending actually works. `None`/missing (a
    /// config saved before this existed) falls back to an empty
    /// string, same as a never-filled-in slot.
    #[serde(default)]
    pub cw_text_messages: [Option<String>; 5],
    /// Which of cw_text_messages the main window's dropdown last had
    /// selected -- purely a UI convenience (which slot to preselect on
    /// reconnect), never affects what actually gets sent (that's
    /// always read fresh from cw_text_messages at Send time). `None`/
    /// out-of-range falls back to 0.
    #[serde(default)]
    pub cw_text_selected: Option<usize>,
    /// Per-band PA gain (dB) entered via the PA Calibration sliders
    /// (Settings -> TX), keyed by band name (see main.rs's BANDS).
    /// Feeds radio::drive_byte_for_watts in place of the flat
    /// radio::DEFAULT_PA_GAIN_DB fallback -- a band with no entry here
    /// just uses that fallback, same as an out-of-the-box,
    /// never-calibrated install would.
    #[serde(default)]
    pub pa_calibration: std::collections::HashMap<String, f32>,
    /// Per-band drive-level linearization curve (Settings -> TX -> PA
    /// Calibration), 9 dB-adjustment points at 10%/20%/.../90% of the
    /// band's calibrated power, subtracted from pa_calibration's flat
    /// per-band gain -- see main.rs's interpolate_drive_adjust doc
    /// comment for the interpolation and radio::drive_byte_for_watts
    /// for how the result feeds the wire-level drive byte. Ports
    /// Thetis's own per-band drive-linearization table (PAProfile::
    /// _gainAdjust/GetGainForBand, Project Files/Source/Console/
    /// setup.cs) -- confirmed necessary via a real ANAN-8000DLE report:
    /// a single flat gain_db (piHPSDR's own simplified model, which
    /// this project mirrored) can't correct real PA gain non-
    /// uniformity across the drive range -- calibrating exactly at
    /// 100W left 50W/75W commanded measuring only 16W/63W actual. A
    /// missing entry (or all-zero) means no adjustment anywhere,
    /// identical to pre-feature behavior.
    #[serde(default)]
    pub pa_drive_adjust: std::collections::HashMap<String, [f32; 9]>,
    /// Upper bound (watts) for the main panel's TX Power slider --
    /// see main.rs's ConnectedState::max_tx_power_watts doc comment for
    /// why this has to be a per-radio (per-MAC) override rather than a
    /// fixed board-type default: the discovery protocol can't tell a
    /// 100W ANAN-100D and a 200W ANAN-8000DLE apart (both report as
    /// Orion2). Missing (e.g. never set, or configs saved before this
    /// existed) falls back to main.rs's default_max_tx_power_watts.
    pub max_tx_power_watts: Option<u32>,
    /// TX Power used while the Tune button is active, as a percentage
    /// of whatever the TX Power slider was set to at the moment TUNE
    /// was pressed (main.rs's pre_tune_power_watts) -- scales with
    /// your normal operating power rather than being a fixed ceiling.
    /// Missing falls back to a conservative 20%.
    pub tune_power_percent: Option<u32>,
    /// SWR threshold (e.g. 3.0 = 3:1) above which the TX power meter's
    /// needle/readout turn red -- see main.rs's draw_power_meter. Missing
    /// falls back to 3.0.
    pub max_swr: Option<f32>,
    /// Classic Ozy hardware only (see radio.rs's start_protocol1_ozy_usb)
    /// -- paths to the user-supplied FX2 firmware (.hex) and FPGA
    /// bitstream (.rbf) files, set via Settings' file pickers. Not
    /// bundled in this repo (matches piHPSDR's own approach) -- see
    /// README's Ozy USB section for where to get them.
    pub ozy_firmware_path: Option<String>,
    pub ozy_fpga_path: Option<String>,
    /// Radioberry "Juice" host program (a separate executable -- see
    /// radioberry_juice.rs) -- path to that executable, set via the
    /// Discover window's "Radioberry Juice setup" section, same idiom
    /// as the two Ozy paths above. The FPGA choice (CL016/CL025) is
    /// juice's own setting, kept here only so the UI can show the
    /// last-picked value without re-reading radioberry.props on every
    /// frame; radioberry.props next to the executable remains the
    /// actual source of truth juice itself reads at startup.
    pub radioberry_juice_path: Option<String>,
    pub radioberry_juice_fpga: Option<crate::radioberry_juice::Fpga>,
    /// Fixed correction folded into the S-meter/panadapter dBm reading,
    /// set via Settings -> RX's "RX Gain Cal" control -- matches
    /// piHPSDR's rx_gain_calibration (its Radio settings dialog's own
    /// "RX Gain Calibr. (dB)" spin box, -50..50). NOT the live RX
    /// Gain/Attenuation value (rx_attenuation below) -- this is a
    /// fixed offset against a known reference signal, set once and
    /// rarely touched. Missing (not yet set) falls back to 0, same as
    /// piHPSDR's own uncalibrated default.
    pub rx_gain_calibration_db: Option<i32>,
    /// HermesLite/HermesLite2-only -- see radio::RadioSession::lna_tx_db's
    /// doc comment.
    #[serde(default)]
    pub lna_tx_db: Option<i32>,
    /// RX-888 Mk2 only (see radio.rs's start_rx888_usb) -- path to the
    /// user-supplied Cypress FX3 RAM image (`SDDC_FX3.img`), set via the
    /// Discover window's "RX-888 USB setup" file picker. Deliberately
    /// NOT bundled (unlike Ozy's own firmware, sourced from the user's
    /// own same-license piHPSDR repo) -- this is a different, unverified
    /// third-party upstream, see rx888.rs's module doc comment.
    #[serde(default)]
    pub rx888_firmware_path: Option<String>,
    /// Spectrum/waterfall display range while transmitting -- separate
    /// from db_low/db_high/waterfall_db_low/waterfall_db_high (which
    /// are for receiving) because a locally-picked-up TX signal is
    /// typically far stronger than the weak RX signals those are
    /// normally tuned for. Defaults (when unset) are derived from the
    /// RX range plus headroom, not independent hardcoded values -- see
    /// where these are read in main.rs.
    pub tx_db_low: Option<f32>,
    pub tx_db_high: Option<f32>,
    pub tx_waterfall_db_low: Option<f32>,
    pub tx_waterfall_db_high: Option<f32>,
    /// Spacing (dB) between the spectrum's power-level gridlines/labels
    /// on the left -- matches piHPSDR's own Display menu "Panadapter
    /// Step" (display_menu.c), which offers 5/10/15/20 per RX and per
    /// TX independently. `#[serde(default)]` (i.e. `None`) so a config
    /// saved before this existed keeps the previous hardcoded-10dB
    /// behavior -- see its read site in main.rs for the actual fallback
    /// value.
    #[serde(default)]
    pub panadapter_step_db: Option<f32>,
    #[serde(default)]
    pub tx_panadapter_step_db: Option<f32>,
    /// Spectrum/waterfall redraw rate (Hz) -- matches piHPSDR's own
    /// Display menu "Frames/sec" (per RX and per TX, 1-64), which
    /// controls how often the panadapter/waterfall actually repaints,
    /// independent of anything else in the UI. `#[serde(default)]` so a
    /// config saved before this existed keeps the previous fixed ~30Hz
    /// behavior -- see its read site in main.rs for the actual fallback
    /// value.
    #[serde(default)]
    pub spectrum_fps: Option<u32>,
    #[serde(default)]
    pub tx_spectrum_fps: Option<u32>,
    /// Spectrum trace style (Settings -> Spectrum) -- see main.rs's
    /// ConnectedState::spectrum_filled/spectrum_gradient doc comment.
    /// `#[serde(default)]` so a config saved before this existed keeps
    /// the previous plain-line look (both default to `false`).
    #[serde(default)]
    pub spectrum_filled: Option<bool>,
    #[serde(default)]
    pub spectrum_gradient: Option<bool>,
    /// "Smooth trace" (Settings -> Display): the old 5-tap smoothing + spline of the panadapter trace. Default off =
    /// deskHPSDR's raw per-pixel polyline.
    #[serde(default)]
    pub spectrum_smooth_trace: Option<bool>,
    /// Settings -> Display (display_window.rs, deskHPSDR display_menu.c): noise-floor margin of Panadapter Automatic
    /// (-20..10, default -5), panadapter detector (0 Peak, 1 Rosenfell, 2 Average, 3 Sample; default 2), averaging mode
    /// (0 None, 1 Recursive, 2 Time Window, 3 Log Recursive; default 3), averaging time in ms (default 250) and
    /// "Display Panadapter" (default on). Missing in older configs: the defaults keep the previous behaviour.
    #[serde(default)]
    pub panadapter_noise_margin: Option<i32>,
    #[serde(default)]
    pub display_detector: Option<u8>,
    #[serde(default)]
    pub display_average_mode: Option<u8>,
    #[serde(default)]
    pub display_average_time_ms: Option<u32>,
    #[serde(default)]
    pub display_panadapter: Option<bool>,
    #[serde(default)]
    pub band_settings: std::collections::HashMap<String, BandSettings>,
    /// Last frequency (IF) and mode of each transverter slot, by its name: selecting the slot again returns there
    /// instead of to the start of its range.
    #[serde(default)]
    pub xvtr_settings: std::collections::HashMap<String, BandSettings>,
    /// Last filter width used per mode, keyed by Mode::label() (e.g.
    /// "USB") -- see main.rs's width_for_mode. A mode with no entry
    /// here (never used yet, or a config saved before this existed)
    /// falls back to spectrum::default_width_hz(mode), same as if this
    /// map didn't exist at all.
    #[serde(default)]
    pub width_memory: std::collections::HashMap<String, f64>,
    /// Extra receivers (beyond the primary one above), P2 only. On
    /// reconnect these are automatically recreated with their saved
    /// settings, matching however many were active last time.
    #[serde(default)]
    pub extra_receivers: Vec<ExtraReceiverConfig>,
    /// PureSignal (experimental, Phase 1 -- protocol-level feedback
    /// plumbing only, no WDSP predistortion engine wired up yet). Only
    /// takes effect on the NEXT connect -- see radio::RadioSettings's
    /// matching field doc comment for why this can't be a live toggle.
    #[serde(default)]
    pub puresignal_enabled: Option<bool>,
    /// TX Settings' "Allow TX outside ham bands" checkbox -- real
    /// request, a safety default against accidentally transmitting
    /// outside the ham bands (e.g. while parked on "Gen"/general
    /// coverage). `None`/`false` = the default, ham-band-only safety
    /// check is enforced (main.rs's tx_frequency_allowed); `Some(true)`
    /// = the operator has explicitly opted into out-of-band TX (e.g.
    /// MARS/CAP or other authorized use). Live -- takes effect
    /// immediately, no reconnect needed, since it's read fresh from
    /// ConnectedState::allow_out_of_band_tx by every PTT path each time.
    #[serde(default)]
    pub allow_out_of_band_tx: Option<bool>,
    /// Diversity reception (2-ADC boards only, Settings -> Diversity) --
    /// see radio::RadioSettings's matching field doc comment. Only takes
    /// effect on the next connect, same as puresignal_enabled (and
    /// mutually exclusive with it) -- see main.rs's Settings UI.
    #[serde(default)]
    pub diversity_enabled: Option<bool>,
    /// Diversity gain (dB, -27.0..27.0) / phase (degrees, -180.0..180.0)
    /// -- unlike diversity_enabled these ARE live-adjustable without a
    /// reconnect (see RadioSession::diversity_gain_db/diversity_phase_deg's
    /// doc comments); saved here purely so the last-tuned values survive
    /// a restart.
    #[serde(default)]
    pub diversity_gain_db: Option<f32>,
    #[serde(default)]
    pub diversity_phase_deg: Option<f32>,
    /// Protocol 1 RX step attenuator (0-31 dB), standard (non-HermesLite)
    /// boards only -- see radio::RadioSession::rx_attenuation's doc
    /// comment. Missing (e.g. configs saved before this existed)
    /// falls back to RadioSession::start's own default rather than the
    /// old hardcoded 0dB.
    pub rx_attenuation: Option<u32>,
    /// Protocol 1 ALEX front-end attenuator relay (0=0dB, 1=10dB,
    /// 2=20dB, 3=30dB) -- see radio::RadioSession::alex_attenuation's
    /// doc comment. A separate physical stage from rx_attenuation
    /// above, present on different (and sometimes overlapping) boards.
    /// Missing falls back to RadioSettings::default's own 0dB.
    #[serde(default)]
    pub alex_attenuation: Option<u8>,
    /// Protocol 1 Metis/Ozy front-end preamp -- see
    /// radio::RadioSession::preamp_enabled's doc comment. Missing falls
    /// back to RadioSettings::default's own off.
    #[serde(default)]
    pub preamp_enabled: Option<bool>,
    /// PureSignal calibration values (Settings -> PureSignal) -- see
    /// tx::PsParams's field docs for what each one means. Missing
    /// (e.g. configs saved before Phase 3 existed) falls back to the
    /// same reference defaults tx::PsParams::default uses.
    #[serde(default)]
    pub ps_hw_peak: Option<f64>,
    #[serde(default)]
    pub ps_mox_delay: Option<f64>,
    #[serde(default)]
    pub ps_loop_delay: Option<f64>,
    #[serde(default)]
    pub ps_tx_delay_ns: Option<f64>,
    /// PureSignal feedback TX-time step attenuator (0-31 dB, Protocol 1
    /// standard boards only) -- see radio::RadioSession::ps_tx_attenuation's
    /// doc comment. Missing falls back to RadioSettings::default's own
    /// 0dB (no attenuation, matching the old unconditional hardcoded
    /// behavior before this control existed).
    #[serde(default)]
    pub ps_tx_attenuation: Option<u32>,
    /// See radio::RadioSession::send_rx_audio_to_radio's doc comment
    /// (Settings -> RX). Missing/never set falls back to off, same as
    /// the live default.
    #[serde(default)]
    pub send_rx_audio_to_radio: Option<bool>,
    /// Settings -> SDR Device (deskHPSDR's radio menu): external TxInhibit / AutoTune inputs, HL2 CL1 10 MHz reference clock, HL2 ATU TUNE support.
    #[serde(default)]
    pub tx_inhibit_enabled: Option<bool>,
    #[serde(default)]
    pub auto_tune_enabled: Option<bool>,
    #[serde(default)]
    pub hl2_cl1_input: Option<bool>,
    #[serde(default)]
    pub hl2_atu_gateware: Option<bool>,
    /// Settings -> SDR Device: REC (report recorder) maximum length in seconds and the IARU region (1..3).
    #[serde(default)]
    pub report_capture_secs: Option<u32>,
    #[serde(default)]
    pub iaru_region: Option<u8>,
    /// See radio::RadioSession::hl2_ak4951_codec's doc comment
    /// (Settings -> RX, HermesLite2 + Protocol 1 only). Missing/never
    /// set falls back to off, same as the live default.
    #[serde(default)]
    pub hl2_ak4951_codec: Option<bool>,
    /// See radio::RadioSession::new_pa_board's doc comment (Settings ->
    /// Antenna, Hermes/Angelia/Orion boards only). Missing/never set
    /// falls back to the "old PA board" default, same as the live
    /// default.
    #[serde(default)]
    pub new_pa_board: Option<bool>,
    /// See radio::RadioSession::tx_audio_source's doc comment (Settings
    /// -> TX) -- one of radio::TX_AUDIO_SOURCE_AUTO/RADIO_MIC/LOCAL_MIC.
    /// Missing/never set falls back to Auto, same as the live default.
    /// Renamed from the old `use_radio_mic: Option<bool>` when a third
    /// value was added -- an old saved config with that field just
    /// falls back to Auto once, same as never having been set.
    #[serde(default)]
    pub tx_audio_source: Option<u8>,
    /// See radio::RadioSession::mic_ptt_enabled/mic_bias_enabled/
    /// mic_ptt_on_tip's doc comments (Settings -> TX, standard boards
    /// only). Missing/never set falls back to off/off/"PTT on Ring",
    /// same as the live defaults.
    #[serde(default)]
    pub mic_ptt_enabled: Option<bool>,
    #[serde(default)]
    pub mic_bias_enabled: Option<bool>,
    #[serde(default)]
    pub mic_ptt_on_tip: Option<bool>,
    /// Main window's position/size as last seen for THIS radio -- keyed
    /// per-MAC (like the rest of this file) rather than globally, so
    /// each physical radio can reopen its window wherever it was last
    /// used, independent of any other radio's window. Applied once, via
    /// an explicit ViewportCommand right after connecting (see main.rs)
    /// -- the main window already exists by then (it's also the
    /// Discovery screen), so unlike a fresh viewport's ViewportBuilder
    /// this can't just be an initial hint.
    #[serde(default)]
    pub window_geometry: Option<WindowGeometry>,
    /// The Digital Modes window's own position/size as last seen for
    /// THIS radio -- a real request, same "reopen wherever it was last
    /// used" reasoning as window_geometry just above, but for that
    /// secondary window specifically (it currently always opens at a
    /// fixed default position/size, see main.rs's ViewportBuilder call
    /// site). Unlike the main window, this one's a fresh viewport each
    /// time it opens, so it's a plain ViewportBuilder::with_position/
    /// with_inner_size seed rather than needing a post-hoc
    /// ViewportCommand.
    #[serde(default)]
    pub digital_window_geometry: Option<WindowGeometry>,
    /// CTUN ("Click to Tune") state for the main receiver -- see
    /// ConnectedState::ctun's doc comment (main.rs) for what this
    /// actually does. `ctun_frequency_hz` is only meaningful/restored
    /// when `ctun` is `Some(true)`; otherwise a fresh connect just uses
    /// the dial frequency (`frequency_hz` above) for both, same as
    /// CTUN's own live "off" behavior.
    #[serde(default)]
    pub ctun: Option<bool>,
    #[serde(default)]
    pub ctun_frequency_hz: Option<u32>,
    /// See ConnectedState::tune_step_hz's own doc comment. Missing (a
    /// config saved before this existed) falls back to 1000 (1kHz),
    /// this project's original hardcoded default.
    #[serde(default)]
    pub tune_step_hz: Option<i64>,
    /// RIT/XIT scroll step in Hz (1, 10 or 100) -- set in the VFO window.
    #[serde(default)]
    pub rit_step_hz: Option<i32>,
    /// Kiosk bottom toolbar (see toolbar.rs): the function assigned to each button, one list of
    /// names per layer, and the layer FNC left selected. Missing/unknown entries keep the factory
    /// assignment.
    #[serde(default)]
    pub toolbar_layers: Option<Vec<Vec<String>>>,
    #[serde(default)]
    pub toolbar_layer: Option<usize>,
    /// VFO B's own step (the step of VFO A is tune_step_hz).
    pub vfo_b_step_hz: Option<i64>,
    /// Last position of the duplex TX window (x, y), moved by touch; None = default (below the start of the spectrum).
    pub tx_window_pos: Option<[f32; 2]>,
    /// "PA enable" (piHPSDR radio menu): off = TX on the low-power output, TR relay stays in RX (duplex).
    pub pa_enabled: Option<bool>,
    /// Frequency calibration in ppm (-100..100), deskHPSDR's ppm_factor.
    pub freq_cal_ppm: Option<f64>,
    /// VOX (deskHPSDR's vox_menu): enable, threshold 0..1, hang in ms, side channel filter and its cut-offs in Hz.
    pub vox_enabled: Option<bool>,
    pub vox_threshold: Option<f64>,
    pub vox_hang_ms: Option<f64>,
    pub vox_filter: Option<bool>,
    pub vox_filter_low_hz: Option<f64>,
    pub vox_filter_high_hz: Option<f64>,
    /// VFO step per mode label (deskHPSDR/piHPSDR keep the step of each mode).
    #[serde(default)]
    pub step_memory: std::collections::HashMap<String, i64>,
    /// Encoder ticks per VFO step (piHPSDR's "VFO encoder divisor").
    pub vfo_encoder_divisor: Option<f32>,
    /// Diagnostic values shown in the line above the spectrum (kiosk), picked in Settings -> Diagnostic.
    #[serde(default)]
    pub diag_items: Vec<String>,
    /// VFO B / Split -- see ConnectedState::vfo_b_frequency_hz/split's
    /// doc comments (main.rs). `None`/missing falls back to A's
    /// frequency and Split off, respectively -- same "never leave a
    /// frequency field at a meaningless 0, and a config saved before
    /// this existed shouldn't suddenly start in Split" reasoning as
    /// ctun/ctun_frequency_hz above.
    #[serde(default)]
    pub vfo_b_frequency_hz: Option<u32>,
    #[serde(default)]
    pub split: Option<bool>,
    /// See ConnectedState::cw_decode_enabled's doc comment. `None`
    /// (a config saved before this existed) falls back to true, same
    /// as a fresh connect.
    #[serde(default)]
    pub cw_decode_enabled: Option<bool>,
    /// RIT / XIT -- see ConnectedState::rit_enabled/xit_enabled's doc
    /// comments (main.rs). `None`/missing falls back to off with a
    /// zero offset, same "a config saved before this existed shouldn't
    /// suddenly start in RIT/XIT" reasoning as ctun/split above.
    #[serde(default)]
    pub rit_enabled: Option<bool>,
    #[serde(default)]
    pub rit_offset_hz: Option<f64>,
    #[serde(default)]
    pub xit_enabled: Option<bool>,
    #[serde(default)]
    pub xit_offset_hz: Option<f64>,
    /// Configured transverters (up to 8, see main.rs's MAX_XVTRS) -- see
    /// main.rs's Xvtr struct doc comment. An empty `name` marks an unused
    /// slot. `#[serde(default)]` so configs saved before this existed just
    /// load with no transverters defined, same as every other feature
    /// added to this struct.
    #[serde(default)]
    pub xvtrs: Vec<crate::Xvtr>,
    /// Name of the XVTR slot that was active/displayed-through at last
    /// disconnect, if any -- see main.rs's ConnectedState::active_xvtr
    /// doc comment. `#[serde(default)]` so configs saved before this
    /// existed just start with no transverter active, same as every
    /// other feature added to this struct. Restoring this is safe (won't
    /// reintroduce the ambiguity active_xvtr itself exists to avoid)
    /// because it's restored verbatim as explicit state, not re-derived
    /// from the restored frequency -- and if the named slot no longer
    /// exists, or the restored frequency no longer falls within its
    /// range (e.g. its settings changed since), the very first frame's
    /// own auto-clear check (see the per-frame reconciliation block)
    /// clears it right back to None.
    #[serde(default)]
    pub active_xvtr: Option<String>,
    /// Per-band (or XVTR) Open Collector Rx/Tx masks -- see main.rs's
    /// OcMask struct doc comment. Keyed by band/XVTR name, same pattern
    /// as pa_calibration above. `#[serde(default)]` so configs saved
    /// before this existed just load with no OC outputs configured.
    #[serde(default)]
    pub oc_settings: std::collections::HashMap<String, crate::OcMask>,
    /// Global Open Collector mask ORed into the active band's Tx mask
    /// while TUNE is active -- see main.rs's ConnectedState::oc_tune
    /// doc comment.
    #[serde(default)]
    pub oc_tune: u8,
    /// "HL2 ADC Auto Gain RxPGA" -- see main.rs's
    /// ConnectedState::autogain_enabled/autogain_time_enabled own doc
    /// comments.
    #[serde(default)]
    pub autogain_enabled: Option<bool>,
    #[serde(default)]
    pub autogain_time_enabled: Option<bool>,
    /// Per-band (or XVTR) RX/TX antenna port selection -- see main.rs's
    /// AntennaMask struct doc comment. Keyed by band/XVTR name, same
    /// pattern as oc_settings above. `#[serde(default)]` so configs saved
    /// before this existed just load with every band defaulting to ANT1
    /// (matching the single global antenna's old default).
    #[serde(default)]
    pub antenna_settings: std::collections::HashMap<String, crate::AntennaMask>,
    /// Whether the MIDI worker should actually open a device -- see
    /// main.rs's MidiWorker::enabled (a live toggle, no reconnect needed).
    /// `#[serde(default)]` so configs saved before this existed just
    /// start with MIDI disabled.
    #[serde(default)]
    pub midi_enabled: Option<bool>,
    /// Target MIDI input port name, matched by name (see midi.rs's
    /// `connect()` doc comment for why name rather than a backend-
    /// specific port id). Superseded by `midi_device_names` (plural,
    /// below) -- kept only so a config saved before multi-device support
    /// existed still has something to migrate from on load (see
    /// connect_to_device's own migration comment); no longer written to
    /// by a save from this version onward.
    #[serde(default)]
    pub midi_device_name: Option<String>,
    /// Target MIDI input port names -- unlike the old single-device
    /// `midi_device_name` above, every one of these is connected
    /// simultaneously and feeds the same binding table (see midi.rs's
    /// `MidiWorker` doc comment for why: e.g. a button box and a
    /// separate jog-wheel controller can both be used at once, matching
    /// piHPSDR's own multi-device model).
    #[serde(default)]
    pub midi_device_names: Vec<String>,
    /// User-configured note/CC-to-action bindings (Settings -> MIDI's
    /// learn mode) -- see crate::midi::MidiBinding. A naturally repeated/
    /// keyed list, same precedent as `xvtrs` above, not a flat field.
    #[serde(default)]
    pub midi_bindings: Vec<crate::midi::MidiBinding>,
}

fn default_nb_threshold() -> f64 {
    20.0
}

fn default_nnr_mask_floor_db() -> f64 {
    -25.0
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ExtraReceiverConfig {
    pub frequency_hz: u32,
    pub sample_rate_hz: u32,
    pub adc: u8,
    #[serde(default)]
    pub band_settings: std::collections::HashMap<String, BandSettings>,
    /// See Config::width_memory's doc comment -- same thing, per extra
    /// receiver instead of shared across the session.
    #[serde(default)]
    pub width_memory: std::collections::HashMap<String, f64>,
    pub mode: Mode,
    pub width_hz: f64,
    pub gain: f32,
    /// See Config::audio_output_device's doc comment -- same thing, this
    /// receiver's own independent output device selection.
    #[serde(default)]
    pub audio_output_device: Option<String>,
    pub agc: Agc,
    pub agc_attack_ms: i32,
    pub agc_decay_ms: i32,
    pub agc_hang_ms: i32,
    pub agc_top_db: f64,
    pub agc_slope_db: i32,
    /// See spectrum::DemodParams::meter_calibration_db's own doc
    /// comment. `#[serde(default)]` -- same reasoning as every other
    /// field added to this struct after the ones above it.
    #[serde(default)]
    pub meter_calibration_db: f64,
    pub db_low: f32,
    pub db_high: f32,
    pub waterfall_db_low: f32,
    pub waterfall_db_high: f32,
    pub waterfall_palette: Palette,
    // Added after the fields above -- #[serde(default)] so extra
    // receivers saved by an older build (without these) still load
    // instead of failing the whole Config and losing every setting.
    #[serde(default)]
    pub noise_blanker: NoiseBlanker,
    #[serde(default = "default_nb_threshold")]
    pub nb_threshold: f64,
    #[serde(default)]
    pub noise_reduction: NoiseReduction,
    /// See Config::nnr_mask_floor_db's doc comment.
    #[serde(default = "default_nnr_mask_floor_db")]
    pub nnr_mask_floor_db: f64,
    /// See Config::nnr_premium's doc comment.
    #[serde(default)]
    pub nnr_premium: bool,
    /// See Config::snb's doc comment.
    #[serde(default)]
    pub snb: bool,
    /// See Config::anf's doc comment.
    #[serde(default)]
    pub anf: bool,
    /// See Config::binaural's doc comment.
    #[serde(default)]
    pub binaural: bool,
    /// See Config::spectrum_waterfall_ratio's doc comment.
    #[serde(default = "default_spectrum_waterfall_ratio")]
    pub spectrum_waterfall_ratio: f32,
    /// See Config::waterfall_enabled's doc comment.
    #[serde(default = "default_waterfall_enabled")]
    pub waterfall_enabled: bool,
    /// See Config::rx_eq's doc comment -- same type, this receiver's own
    /// independent copy.
    #[serde(default)]
    pub eq: EqualizerParams,
    /// See Config::window_geometry's doc comment -- same thing, this
    /// receiver's own window. Unlike the main window, an extra
    /// receiver's viewport doesn't exist yet when this is read, so it's
    /// seeded straight into that viewport's initial ViewportBuilder
    /// (see main.rs's spawn_extra_receiver/show_viewport_deferred).
    #[serde(default)]
    pub window_geometry: Option<WindowGeometry>,
    /// See Config::ctun's doc comment -- same thing, this receiver's own.
    #[serde(default)]
    pub ctun: bool,
    #[serde(default)]
    pub ctun_frequency_hz: u32,
    /// See Config::vfo_b_frequency_hz's doc comment -- same thing, this
    /// receiver's own. No Split here -- extra receivers never transmit.
    #[serde(default)]
    pub vfo_b_frequency_hz: Option<u32>,
    /// See Config::cw_decode_enabled's doc comment -- same thing, this
    /// receiver's own.
    #[serde(default)]
    pub cw_decode_enabled: Option<bool>,
    /// See Config::spectrum_zoom/spectrum_pan's doc comments -- same
    /// thing, this receiver's own.
    #[serde(default = "default_spectrum_zoom")]
    pub spectrum_zoom: i32,
    #[serde(default)]
    pub spectrum_pan: f32,
    /// See Config::db_low_auto's doc comment -- same thing, this
    /// receiver's own.
    #[serde(default = "default_db_low_auto")]
    pub db_low_auto: bool,
    /// See Config::rit_enabled's doc comment -- same thing, this
    /// receiver's own. No XIT here -- extra receivers never transmit.
    #[serde(default)]
    pub rit_enabled: bool,
    #[serde(default)]
    pub rit_offset_hz: f64,
}

fn default_db_low_auto() -> bool {
    false
}

fn default_spectrum_zoom() -> i32 {
    1
}

fn default_spectrum_waterfall_ratio() -> f32 {
    0.70
}

fn default_waterfall_enabled() -> bool {
    true
}

/// Per-platform settings directory, created if it doesn't exist yet.
/// `config_path`/`ps_corr_path` both build on this rather than each
/// duplicating their own copy of the same platform logic.
///
/// BUG FIX: this used to build `$HOME/.config/hpsdr-rs` unconditionally
/// on every platform -- correct for Linux (matches the XDG convention),
/// but `$HOME` isn't normally set outside an MSYS2 shell on Windows
/// (confirmed via a real Windows/MSVC build+run session: the app ran
/// fine, but had no way to persist settings between runs), and even
/// where it is set, `.config` isn't the native Windows convention
/// anyway. Now branches by `target_os`: Windows uses `%APPDATA%\
/// hpsdr-rs` (the standard per-user roaming-settings location); macOS
/// uses `~/Library/Application Support/hpsdr-rs` (matching WDSP's own
/// C source, which already has a real `__APPLE__` code path -- see
/// build.rs's doc comment -- even though macOS isn't a built/tested
/// target yet); everything else (Linux, BSDs) keeps the original
/// `$HOME/.config/hpsdr-rs` behavior unchanged.
pub(crate) fn settings_dir() -> Option<PathBuf> {
    let mut path = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        let mut p = PathBuf::from(std::env::var_os("HOME")?);
        p.push("Library");
        p.push("Application Support");
        p
    } else {
        let mut p = PathBuf::from(std::env::var_os("HOME")?);
        p.push(".config");
        p
    };
    path.push("hpsdr-rs");
    std::fs::create_dir_all(&path).ok()?;
    Some(path)
}

/// The 1024x600 LCD kiosk mode's UI scale (see main()'s HPSDR_LCD_1024X600
/// doc comment) -- a MACHINE/panel preference, not a per-radio one, so it
/// deliberately lives in its own small file rather than inside the
/// per-MAC `Config` this module otherwise deals in (there's no single
/// radio to key it by: the same panel might be used with several radios
/// over time, and the scale should stay the same regardless of which one
/// is currently connected). Read once at startup, before any radio is
/// even discovered -- see main()'s own use of this -- since the kiosk
/// window's fixed logical size has to be picked before eframe creates
/// the window at all; there's no live "resize while running" path for
/// it (changing pixels_per_point after the window exists wouldn't also
/// shrink/grow the OS window to compensate, so the two would drift out
/// of sync with the physical 1024x600 panel). 1.0 (100%) is the
/// original, un-scaled kiosk size.
// 150%/175%/200% added at a user's request (text on a Raspberry Pi panel read
// ~25% smaller than the Windows kiosk build). Only text sizes scale (see
// main()'s use of this), not fixed-width widgets, so the largest steps can
// clip rows that were laid out for 100%.
// 160% added after trying 100%-180% in 5% steps on the Pi panel: it is the size that reads best at 1024x600.
const KIOSK_SCALE_PRESETS: [f32; 7] = [1.0, 1.10, 1.25, 1.5, 1.6, 1.75, 2.0];

fn kiosk_scale_path() -> Option<PathBuf> {
    let mut path = settings_dir()?;
    path.push("kiosk_scale.json");
    Some(path)
}

/// Loads the saved kiosk UI scale, falling back to 1.0 (100%, the
/// original size) if nothing was ever saved or the file can't be read --
/// same "missing means default" convention as the rest of this module.
pub fn load_kiosk_ui_scale() -> f32 {
    kiosk_scale_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|s| KIOSK_SCALE_PRESETS.contains(s))
        .unwrap_or(1.0)
}

/// Saves the kiosk UI scale for the next launch to pick up -- see
/// load_kiosk_ui_scale's doc comment for why this can't take effect
/// live. A plain float-as-text file, not JSON: this is the only value
/// in it, so the extra structure would be pure overhead.
pub fn save_kiosk_ui_scale(scale: f32) {
    if let Some(path) = kiosk_scale_path() {
        let _ = std::fs::write(path, scale.to_string());
    }
}

/// The fixed choices `save_kiosk_ui_scale` accepts -- see
/// KIOSK_SCALE_PRESETS' own doc comment for why these are discrete
/// presets rather than a free-form slider (a real request: "not every
/// size, just a little room to grow"). Exposed for main.rs's Settings ->
/// Screen tab to build its picker from, rather than duplicating the list.
pub fn kiosk_ui_scale_presets() -> &'static [f32] {
    &KIOSK_SCALE_PRESETS
}

fn last_manual_ip_path() -> Option<PathBuf> {
    let mut path = settings_dir()?;
    path.push("last_manual_ip.txt");
    Some(path)
}

/// The last IP address successfully used via the Discovery window's own
/// "Manual IP" field -- same "own small machine-level file, not the
/// per-radio Config" reasoning as kiosk_scale.json above, since this
/// isn't a setting FOR a particular radio, it's about how to FIND one in
/// the first place (a real report: a HermesLite2 reachable only via a
/// direct USB3-LAN link, not a router, never answers this app's UDP
/// discovery broadcast on some setups -- likely a routing/ARP quirk of a
/// point-to-point link rather than a proper switched LAN segment -- so
/// the operator has to fall back to Manual IP every single launch, and
/// re-typing the same address by hand each time is exactly the kind of
/// friction a remembered default removes). Pre-fills the field rather
/// than auto-connecting outright, so a genuinely different radio at a
/// new address (or the same one having moved) is still just one click
/// away, not a forced retype.
pub fn load_last_manual_ip() -> Option<String> {
    last_manual_ip_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| s.trim().to_string())
}

pub fn save_last_manual_ip(ip: &str) {
    if let Some(path) = last_manual_ip_path() {
        let _ = std::fs::write(path, ip);
    }
}

/// Where an auto-saved SSTV RX picture goes -- same directory/naming
/// convention as audio_recorder::recording_path (own subdirectory under
/// settings_dir, epoch-seconds + a label in the filename so two images
/// completing in the same second can't collide) -- a real request:
/// "faz gravação das imagens que recebe... os programas de SSTV tipo
/// QSSTV fazem isso", matching that reference app's own default
/// behaviour of saving every decoded picture without the operator
/// having to manually export it.
pub fn sstv_image_path(mode_label: &str) -> Option<PathBuf> {
    let mut dir = sstv_image_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let epoch_secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    dir.push(format!("hpsdr-rs_{epoch_secs}_{mode_label}.png"));
    Some(dir)
}

/// Just the folder, no filename/directory-creation side effect -- for
/// showing the operator where auto-saved pictures go (a hover tooltip)
/// without generating a throwaway filename/touching the filesystem on
/// every frame it's shown.
pub fn sstv_image_dir() -> Option<PathBuf> {
    let mut dir = settings_dir()?;
    dir.push("sstv_images");
    Some(dir)
}

/// A window's on-screen position and content size, in egui points
/// (matches `egui::ViewportBuilder::with_position`/`with_inner_size`'s
/// units -- see Config::window_geometry/ExtraReceiverConfig::window_geometry).
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

fn config_path(mac: [u8; 6]) -> Option<PathBuf> {
    let mut path = settings_dir()?;
    let [a, b, c, d, e, f] = mac;
    path.push(format!("config-{a:02x}-{b:02x}-{c:02x}-{d:02x}-{e:02x}-{f:02x}.json"));
    Some(path)
}

/// Per-radio PureSignal correction-table file, same MAC-keyed directory
/// convention as `config_path` -- written/read via WDSP's own
/// `PSSaveCorr`/`PSRestoreCorr` (tx.rs), not this project's own
/// serialization, so the `.dat` extension and internal format are
/// whatever WDSP itself uses, not something to parse here.
pub fn ps_corr_path(mac: [u8; 6]) -> Option<PathBuf> {
    let mut path = settings_dir()?;
    let [a, b, c, d, e, f] = mac;
    path.push(format!("ps_corr-{a:02x}-{b:02x}-{c:02x}-{d:02x}-{e:02x}-{f:02x}.dat"));
    Some(path)
}

/// The controls that belong to the operator's hardware, not to one radio: MIDI on/off, devices and bindings, and
/// the toolbar layout. They are kept in one shared file next to the per-radio configs, so switching radios (HL2,
/// Radioberry ...) does not start again from an empty MIDI/toolbar setup.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SharedControls {
    #[serde(default)]
    midi_enabled: Option<bool>,
    #[serde(default)]
    midi_device_names: Vec<String>,
    #[serde(default)]
    midi_bindings: Vec<crate::midi::MidiBinding>,
    #[serde(default)]
    toolbar_layers: Option<Vec<Vec<String>>>,
    #[serde(default)]
    toolbar_layer: Option<usize>,
}

/// UI-only (non-DSP) TX options of deskHPSDR's TX menu, plus the persisted live DSP options (`tx_extra`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TxUiExtra {
    pub tune_use_drive: bool,
    pub drive_per_band: bool,
    /// Tune drive step in %: 1, 5, 10, 20 or 25.
    pub tune_drive_step: u8,
    pub swr_protection: bool,
    pub swr_alarm: f64,
    pub max_digi_drive: i32,
    pub tx_display_filled: bool,
    pub peaks_on: bool,
    pub peaks_in_passband: bool,
    pub peaks_hide_noise: bool,
    pub peaks_num: i32,
    pub peaks_ignore_divider: i32,
    pub peaks_noise_percentile: i32,
    /// Settings -> Display (deskHPSDR): peak labels as S-meter values, and the Peaks & Hold page.
    pub peaks_as_smeter: bool,
    /// Separate TX parameter set of the peak labels (deskHPSDR transmitter.c defaults); `peaks_on` enables both.
    pub peaks_tx_num: i32,
    pub peaks_tx_divider: i32,
    pub peaks_tx_percentile: i32,
    pub peaks_tx_hide_noise: bool,
    pub peaks_tx_in_passband: bool,
    pub peak_hold_on: bool,
    /// 1 = Peaks hold, 2 = Peaks decay.
    pub peak_hold_mode: u8,
    pub peak_hold_sec: f32,
    pub peak_hold_drop_db: f32,
    pub peak_hold_tx: bool,
    /// RGBA 0..1: Peaks & Hold line, and the TX panadapter line/fill.
    pub peak_line_col: [f32; 4],
    pub tx_pan_col: [f32; 4],
    pub tx_extra: Option<crate::tx::TxExtra>,
    /// PureSignal menu: OneShot and PS Stability (0 Strict, 1 Medium, 2 Relaxed), saved like deskHPSDR (ps_oneshot, ps_tolerance_mode).
    pub ps_oneshot: bool,
    pub ps_stability: u8,
}

impl Default for TxUiExtra {
    fn default() -> Self {
        Self {
            tune_use_drive: false,
            drive_per_band: true,
            tune_drive_step: 1,
            swr_protection: false,
            swr_alarm: 3.0,
            max_digi_drive: 100,
            tx_display_filled: false,
            peaks_on: false,
            peaks_in_passband: false,
            peaks_hide_noise: true,
            peaks_num: 3,
            peaks_ignore_divider: 20,
            peaks_noise_percentile: 80,
            peaks_as_smeter: false,
            peaks_tx_num: 4,
            peaks_tx_divider: 24,
            peaks_tx_percentile: 50,
            peaks_tx_hide_noise: true,
            peaks_tx_in_passband: false,
            peak_hold_on: false,
            peak_hold_mode: 2,
            peak_hold_sec: 2.0,
            peak_hold_drop_db: 6.0,
            peak_hold_tx: false,
            peak_line_col: [0.70, 0.70, 0.70, 1.00],
            tx_pan_col: [0.0, 1.0, 0.0, 1.0],
            tx_extra: None,
            ps_oneshot: false,
            ps_stability: 2,
        }
    }
}

/// Settings -> RX Menu (deskHPSDR rx_menu.c): the options that live in the radio / audio layers, plus the persisted
/// live DSP options (`rx_extra`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RxUiExtra {
    pub adc_dither: bool,
    pub adc_random: bool,
    pub adc0_filter_bypass: bool,
    pub adc1_filter_bypass: bool,
    pub rx_reserve_enabled: bool,
    pub rx_reserve_ms: i32,
    pub p2_jitter_enabled: bool,
    pub p2_jitter_depth_ms: i32,
    pub latency_correction: bool,
    pub rx_extra: Option<crate::spectrum::RxExtra>,
}

impl Default for RxUiExtra {
    fn default() -> Self {
        Self {
            adc_dither: false,
            adc_random: false,
            adc0_filter_bypass: false,
            adc1_filter_bypass: false,
            rx_reserve_enabled: false,
            rx_reserve_ms: 100,
            p2_jitter_enabled: false,
            p2_jitter_depth_ms: 20,
            latency_correction: true,
            rx_extra: None,
        }
    }
}

/// Number of mic-profile slots (deskHPSDR `audio_profile_0..2.prop`).
pub const MIC_PROFILE_SLOTS: usize = 3;

impl Config {
    /// Active mic-profile slot, ignoring out-of-range values (0..=2 only).
    pub fn active_mic_profile(&self) -> Option<usize> {
        self.mic_profile_nr.filter(|&n| n < MIC_PROFILE_SLOTS)
    }

    /// The 3 slot descriptions, missing entries default to "NOMIC".
    pub fn mic_profile_descriptions(&self) -> [String; MIC_PROFILE_SLOTS] {
        std::array::from_fn(|i| {
            self.mic_profile_descs
                .get(i)
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "NOMIC".to_string())
        })
    }
}

fn shared_controls_path() -> Option<PathBuf> {
    let mut p = settings_dir()?;
    p.push("shared-controls.json");
    Some(p)
}

impl Config {
    /// Overlays the shared controls (if any) on a radio's own config; the first time (no shared file yet) the
    /// newest per-radio config that has MIDI bindings or a toolbar layout seeds it.
    fn apply_shared_controls(&mut self) {
        let Some(path) = shared_controls_path() else { return };
        let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<SharedControls>(&s).ok());
        let shared = read(&path).or_else(|| {
            // Seed from the most recently saved radio config that has something to share.
            let dir = settings_dir()?;
            let mut best: Option<(std::time::SystemTime, SharedControls)> = None;
            for entry in std::fs::read_dir(dir).ok()?.flatten() {
                let p = entry.path();
                let name = p.file_name()?.to_string_lossy().to_string();
                if !(name.starts_with("config-") && name.ends_with(".json")) {
                    continue;
                }
                let Some(cfg) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok()) else { continue };
                if cfg.midi_bindings.is_empty() && cfg.toolbar_layers.is_none() {
                    continue;
                }
                let t = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                    best = Some((
                        t,
                        SharedControls {
                            midi_enabled: cfg.midi_enabled,
                            midi_device_names: cfg.midi_device_names.clone(),
                            midi_bindings: cfg.midi_bindings.clone(),
                            toolbar_layers: cfg.toolbar_layers.clone(),
                            toolbar_layer: cfg.toolbar_layer,
                        },
                    ));
                }
            }
            best.map(|(_, s)| s)
        });
        if let Some(s) = shared {
            self.midi_enabled = s.midi_enabled;
            self.midi_device_names = s.midi_device_names;
            self.midi_bindings = s.midi_bindings;
            self.toolbar_layers = s.toolbar_layers;
            self.toolbar_layer = s.toolbar_layer;
        }
    }

    fn save_shared_controls(&self) {
        // Nothing worth sharing yet: do not create (or blank) the file from an unconfigured radio.
        if self.midi_bindings.is_empty() && self.toolbar_layers.is_none() {
            return;
        }
        let Some(path) = shared_controls_path() else { return };
        let s = SharedControls {
            midi_enabled: self.midi_enabled,
            midi_device_names: self.midi_device_names.clone(),
            midi_bindings: self.midi_bindings.clone(),
            toolbar_layers: self.toolbar_layers.clone(),
            toolbar_layer: self.toolbar_layer,
        };
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let tmp = path.with_extension("json.tmp");
            let _ = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
        }
    }
}

impl Config {
    /// Loads the saved config for this specific radio (by MAC address),
    /// or a blank/default one if there isn't one yet (first run for
    /// this radio) or it can't be read/parsed for any reason.
    pub fn load(mac: [u8; 6]) -> Config {
        let Some(path) = config_path(mac) else {
            return Config::default();
        };
        let parse = |p: &std::path::Path| {
            std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok())
        };
        // A missing/corrupt main file falls back to the copy taken at the start
        // of the previous run (see save) instead of silently resetting everything.
        let mut cfg = parse(&path)
            .or_else(|| parse(&path.with_extension("json.bak")))
            .unwrap_or_default();
        // MIDI and the toolbar are the operator's, shared by every radio (see SharedControls). Skipped for the
        // sentinel configs that hold no radio settings (Ozy / Radioberry Juice setup, RX-888).
        if mac != [0; 6] && mac != [0, 0, 0, 0, 0, 1] {
            cfg.apply_shared_controls();
        }
        cfg
    }

    pub fn save(&self, mac: [u8; 6]) {
        let Some(path) = config_path(mac) else {
            return;
        };
        if mac != [0; 6] && mac != [0, 0, 0, 0, 0, 1] {
            self.save_shared_controls();
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            // Once per run, keep the file as it was when the program started
            // (valid JSON only) as <config>.json.bak -- a safety net against any
            // later bug or crash wiping settings.
            static BACKED_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !BACKED_UP.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let valid = std::fs::read_to_string(&path)
                    .ok()
                    .is_some_and(|s| serde_json::from_str::<Config>(&s).is_ok());
                if valid {
                    let _ = std::fs::copy(&path, path.with_extension("json.bak"));
                }
            }
            // Write to a temp file then rename, so a crash mid-write can never
            // leave a truncated config.
            let tmp = path.with_extension("json.tmp");
            let result = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path));
            if let Err(e) = result {
                eprintln!("failed to save config to {}: {e}", path.display());
            }
        }
    }
}
