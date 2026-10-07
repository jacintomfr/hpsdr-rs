//! Bottom toolbar of the 1024x600 kiosk panel: eight boxes under the spectrum, modelled on
//! piHPSDR's toolbar -- 8 layers of 8 assignable function boxes. The layer step is an ordinary
//! function (`FNC`, `MidiAction::ToolbarFuncNext`: tap = next layer, long press = previous), as are
//! `FNC-` and `FNC's` (quick jump list); every box can be assigned any function from the list below.
//!
//! The list is what hpsdr-rs can do today: every MIDI key action (so the toolbar and a MIDI
//! controller share one dispatcher in `main.rs`), plus Two Tone and the zoom/pan controls that
//! used to be a slider row.

use crate::midi::{MidiAction, KEY_ACTIONS};

/// Assignable function boxes per layer.
pub const BUTTONS: usize = 8;
/// Number of layers `FNC` steps through.
pub const LAYERS: usize = 8;

pub type Layers = [[ToolbarFn; BUTTONS]; LAYERS];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarFn {
    None,
    Midi(MidiAction),
    TwoTone,
    /// Turns RADE on or off in one press: what Digital -> RADE -> Hide does with the mouse.
    Rade,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    PanLeft,
    PanRight,
}

impl ToolbarFn {
    /// Every assignable function, `None` first.
    pub fn all() -> Vec<ToolbarFn> {
        let mut v = vec![ToolbarFn::TwoTone, ToolbarFn::Rade];
        // The Toolbar (F1-F8) MIDI actions press toolbar boxes, so they make no sense inside one.
        v.extend(KEY_ACTIONS.iter().copied().filter(|a| (!is_toolbar_action(*a) || *a == MidiAction::ToolbarFuncRev) && *a != MidiAction::Rade).map(ToolbarFn::Midi));
        v.extend([
            ToolbarFn::ZoomIn,
            ToolbarFn::ZoomOut,
            ToolbarFn::ZoomReset,
            ToolbarFn::PanLeft,
            ToolbarFn::PanRight,
        ]);
        // Alphabetical, numbers in natural order (Band 6m before Band 10m); None stays first.
        v.sort_by(|a, b| natural_cmp(&a.long_label(), &b.long_label()));
        v.insert(0, ToolbarFn::None);
        v
    }

    /// Stable name used in the saved config.
    pub fn key(self) -> String {
        match self {
            ToolbarFn::None => "None".to_string(),
            ToolbarFn::Midi(a) => format!("{a:?}"),
            ToolbarFn::TwoTone => "TwoTone".to_string(),
            ToolbarFn::Rade => "Rade".to_string(),
            ToolbarFn::ZoomIn => "ZoomIn".to_string(),
            ToolbarFn::ZoomOut => "ZoomOut".to_string(),
            ToolbarFn::ZoomReset => "ZoomReset".to_string(),
            ToolbarFn::PanLeft => "PanLeft".to_string(),
            ToolbarFn::PanRight => "PanRight".to_string(),
        }
    }

    pub fn from_key(key: &str) -> ToolbarFn {
        Self::all().into_iter().find(|f| f.key() == key).unwrap_or(ToolbarFn::None)
    }

    /// Text on the button (the boxes are ~120 px wide).
    pub fn short_label(self) -> &'static str {
        match self {
            ToolbarFn::None => "",
            ToolbarFn::TwoTone => "2TONE",
            ToolbarFn::Rade => "RADE",
            ToolbarFn::ZoomIn => "ZOOM+",
            ToolbarFn::ZoomOut => "ZOOM-",
            ToolbarFn::ZoomReset => "ZOOM 1X",
            ToolbarFn::PanLeft => "PAN <",
            ToolbarFn::PanRight => "PAN >",
            ToolbarFn::Midi(a) => match a {
                MidiAction::Mox => "MOX",
                MidiAction::SquelchToggle => "SQL",
                MidiAction::Vox => "VOX",
                MidiAction::VoxMenu => "VOX SET",
                MidiAction::EqMenu => "EQ",
                MidiAction::ToolbarFuncNext => "FNC",
                MidiAction::ToolbarFuncRev => "FNC-",
                MidiAction::ToolbarFuncList => "FNC's",
                MidiAction::NewMenu => "NEW MENU",
                MidiAction::AgcMenu => "AGC SET",
                MidiAction::NoiseMenu => "NOISE",
                MidiAction::TxMenu => "TX SET",
                MidiAction::RxMenu => "RX SET",
                MidiAction::SdrMenu => "SDR SET",
                MidiAction::ReportRec => "REC",
                MidiAction::ReportPlay => "PLAY",
                MidiAction::RecordWav => "RECORD",
                MidiAction::Tune => "TUNE",
                MidiAction::Split => "SPLIT",
                MidiAction::RitToggle => "RIT",
                MidiAction::RitClear => "RIT CLR",
                MidiAction::XitToggle => "XIT",
                MidiAction::XitClear => "XIT CLR",
                MidiAction::VfoAtoB => "A>B",
                MidiAction::VfoBtoA => "B>A",
                MidiAction::VfoSwap => "A<>B",
                MidiAction::ModeUp => "MODE+",
                MidiAction::ModeDown => "MODE-",
                MidiAction::BandUp => "BAND+",
                MidiAction::BandDown => "BAND-",
                MidiAction::BandMenu => "BAND",
                MidiAction::DigitalMenu => "DIGITAL",
                MidiAction::ModeMenu => "MODE",
                MidiAction::FilterMenu => "FILTER",
                MidiAction::Rade => "RADE",
                MidiAction::FilterWidthUp => "FILT+",
                MidiAction::FilterWidthDown => "FILT-",
                MidiAction::VfoStepUp => "STEP+",
                MidiAction::VfoStepDown => "STEP-",
                MidiAction::NoiseBlankerCycle => "NB",
                MidiAction::NoiseReductionCycle => "NR",
                MidiAction::AgcCycle => "AGC",
                MidiAction::CtunToggle => "CTUN",
                MidiAction::RxEqToggle => "RX EQ",
                MidiAction::DiversityToggle => "DIV",
                MidiAction::BinauralToggle => "BIN",
                MidiAction::SnbToggle => "SNB",
                MidiAction::Band160m => "160m",
                MidiAction::Band80m => "80m",
                MidiAction::Band40m => "40m",
                MidiAction::Band30m => "30m",
                MidiAction::Band20m => "20m",
                MidiAction::Band17m => "17m",
                MidiAction::Band15m => "15m",
                MidiAction::Band12m => "12m",
                MidiAction::Band10m => "10m",
                MidiAction::Band6m => "6m",
                MidiAction::NoiseBlankerOff => "NB OFF",
                MidiAction::NoiseBlankerNb => "NB 1",
                MidiAction::NoiseBlankerNb2 => "NB 2",
                MidiAction::NoiseReductionOff => "NR OFF",
                MidiAction::NoiseReductionNr => "NR 1",
                MidiAction::NoiseReductionNr2 => "NR 2",
                MidiAction::NoiseReductionNr3 => "NNR",
                MidiAction::PureSignalRunningToggle => "PS",
                MidiAction::CwMacro1 => "CW 1",
                MidiAction::CwMacro2 => "CW 2",
                MidiAction::CwMacro3 => "CW 3",
                MidiAction::CwMacro4 => "CW 4",
                MidiAction::CwMacro5 => "CW 5",
                // Not valid for a key binding, so never offered; kept for completeness.
                _ => "",
            },
        }
    }

    /// Full name for the chooser list.
    pub fn long_label(self) -> String {
        match self {
            ToolbarFn::None => "None".to_string(),
            ToolbarFn::Midi(a) => a.label().to_string(),
            ToolbarFn::TwoTone => "Two Tone".to_string(),
            ToolbarFn::Rade => "RADE On/Off".to_string(),
            ToolbarFn::ZoomIn => "Zoom In".to_string(),
            ToolbarFn::ZoomOut => "Zoom Out".to_string(),
            ToolbarFn::ZoomReset => "Zoom 1x, Pan centered".to_string(),
            ToolbarFn::PanLeft => "Pan Left".to_string(),
            ToolbarFn::PanRight => "Pan Right".to_string(),
        }
    }
}

/// Case-insensitive comparison that orders digit runs by value, so "Band 6m" sorts before
/// "Band 10m" and "Toolbar (F2)" before "Toolbar (F10)".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n: u64 = 0;
                    while let Some(c) = it.peek().copied().filter(|c| c.is_ascii_digit()) {
                        n = n.saturating_mul(10).saturating_add(c as u64 - '0' as u64);
                        it.next();
                    }
                    n
                };
                let (na, nb) = (take(&mut ai), take(&mut bi));
                if na != nb {
                    return na.cmp(&nb);
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
                if lx != ly {
                    return lx.cmp(&ly);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// The MIDI actions that press a toolbar box (Toolbar (F1) .. Toolbar (F8)).
pub fn is_toolbar_action(a: MidiAction) -> bool {
    matches!(
        a,
        MidiAction::Toolbar1
            | MidiAction::Toolbar2
            | MidiAction::Toolbar3
            | MidiAction::Toolbar4
            | MidiAction::Toolbar5
            | MidiAction::Toolbar6
            | MidiAction::Toolbar7
            | MidiAction::Toolbar8
            | MidiAction::ToolbarFuncRev
    )
}

/// Factory assignment, laid out like piHPSDR's six layers.
pub fn default_layers() -> Layers {
    use MidiAction as M;
    use ToolbarFn::{Midi, PanLeft, PanRight, TwoTone, ZoomIn, ZoomOut, ZoomReset};
    let base: [[ToolbarFn; 7]; LAYERS] = [
        [
            Midi(M::Mox),
            Midi(M::Tune),
            TwoTone,
            Midi(M::NoiseReductionCycle),
            Midi(M::BandUp),
            Midi(M::ModeUp),
            Midi(M::FilterWidthUp),
        ],
        [
            Midi(M::Split),
            Midi(M::CtunToggle),
            Midi(M::RitToggle),
            Midi(M::RitClear),
            Midi(M::XitToggle),
            Midi(M::XitClear),
            Midi(M::VfoSwap),
        ],
        [ZoomIn, ZoomOut, ZoomReset, PanLeft, PanRight, Midi(M::VfoStepDown), Midi(M::VfoStepUp)],
        [
            Midi(M::BandDown),
            Midi(M::BandUp),
            Midi(M::ModeDown),
            Midi(M::ModeUp),
            Midi(M::FilterWidthDown),
            Midi(M::FilterWidthUp),
            Midi(M::AgcCycle),
        ],
        [
            Midi(M::NoiseBlankerCycle),
            Midi(M::NoiseReductionCycle),
            Midi(M::SnbToggle),
            Midi(M::BinauralToggle),
            Midi(M::RxEqToggle),
            Midi(M::VfoAtoB),
            Midi(M::VfoBtoA),
        ],
        [
            Midi(M::Band80m),
            Midi(M::Band40m),
            Midi(M::Band30m),
            Midi(M::Band20m),
            Midi(M::Band17m),
            Midi(M::Band15m),
            Midi(M::Band10m),
        ],
        // The layers added after the first six (FNC(6), FNC(7)) start empty (None): the user assigns them.
        [ToolbarFn::None; 7],
        [ToolbarFn::None; 7],
    ];
    // The eighth box of every layer is the FNC (next layer) function, as the old fixed button was.
    let mut layers = [[ToolbarFn::None; BUTTONS]; LAYERS];
    for (l, row) in base.iter().enumerate() {
        layers[l][..7].copy_from_slice(row);
        layers[l][7] = Midi(M::ToolbarFuncNext);
    }
    layers
}

/// Layers from the saved config; anything missing or unknown keeps the factory assignment.
/// Migration: rows saved by the old 7-box layout have no eighth entry, so the eighth box keeps its
/// factory value, the FNC (next layer) function; rows with eight entries load as they are.
pub fn layers_from_config(saved: Option<&Vec<Vec<String>>>) -> Layers {
    let mut layers = default_layers();
    if let Some(saved) = saved {
        for (l, row) in saved.iter().take(LAYERS).enumerate() {
            for (b, key) in row.iter().take(BUTTONS).enumerate() {
                layers[l][b] = ToolbarFn::from_key(key);
            }
        }
    }
    layers
}

pub fn layers_to_config(layers: &Layers) -> Vec<Vec<String>> {
    layers.iter().map(|row| row.iter().map(|f| f.key()).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for f in ToolbarFn::all() {
            assert_eq!(ToolbarFn::from_key(&f.key()), f, "{f:?}");
        }
    }

    #[test]
    fn config_round_trip_and_defaults() {
        let d = default_layers();
        assert_eq!(layers_from_config(Some(&layers_to_config(&d))), d);
        assert_eq!(layers_from_config(None), d);
        // A truncated or unknown saved layout falls back per slot.
        let odd = vec![vec!["Mox".to_string(), "NoSuchThing".to_string()]];
        let l = layers_from_config(Some(&odd));
        assert_eq!(l[0][0], ToolbarFn::Midi(MidiAction::Mox));
        assert_eq!(l[0][1], ToolbarFn::None);
        assert_eq!(l[1], d[1]);
        // A config saved with nine layers keeps the first eight; the ninth is dropped.
        let nine: Vec<Vec<String>> = (0..9).map(|_| vec!["Tune".to_string(); BUTTONS]).collect();
        let l = layers_from_config(Some(&nine));
        assert_eq!(l.len(), 8);
        assert_eq!(l[7][0], ToolbarFn::Midi(MidiAction::Tune));
    }

    #[test]
    fn old_seven_entry_rows_get_fnc_as_eighth() {
        let fnc = ToolbarFn::Midi(MidiAction::ToolbarFuncNext);
        assert_eq!(BUTTONS, 8);
        let old: Vec<Vec<String>> = (0..8).map(|_| vec!["Tune".to_string(); 7]).collect();
        let l = layers_from_config(Some(&old));
        for row in l.iter() {
            assert_eq!(row[6], ToolbarFn::Midi(MidiAction::Tune));
            assert_eq!(row[7], fnc);
        }
        assert!(default_layers().iter().all(|r| r[7] == fnc));
    }

    #[test]
    fn eight_entry_rows_round_trip() {
        let mut d = default_layers();
        d[3][7] = ToolbarFn::Midi(MidiAction::Tune);
        d[2][0] = ToolbarFn::Midi(MidiAction::ToolbarFuncNext);
        let saved = layers_to_config(&d);
        assert!(saved.iter().all(|r| r.len() == 8));
        assert_eq!(layers_from_config(Some(&saved)), d);
        assert!(ToolbarFn::all().contains(&ToolbarFn::Midi(MidiAction::ToolbarFuncNext)));
    }

    #[test]
    fn fnc_list_is_assignable() {
        assert_eq!(LAYERS, 8);
        assert!(ToolbarFn::all().contains(&ToolbarFn::Midi(MidiAction::ToolbarFuncList)));
        assert_eq!(ToolbarFn::Midi(MidiAction::ToolbarFuncList).short_label(), "FNC's");
        assert_eq!(ToolbarFn::from_key("ToolbarFuncList"), ToolbarFn::Midi(MidiAction::ToolbarFuncList));
    }

    #[test]
    fn lists_are_alphabetical_with_natural_numbers() {
        use std::cmp::Ordering::Less;
        assert_eq!(natural_cmp("Band 6m", "Band 10m"), Less);
        assert_eq!(natural_cmp("band 20m", "Band 160m"), Less);
        assert_eq!(natural_cmp("Toolbar (F2)", "Toolbar (F10)"), Less);
        let all = ToolbarFn::all();
        assert_eq!(all[0], ToolbarFn::None);
        let labels: Vec<String> = all.iter().skip(1).map(|f| f.long_label()).collect();
        assert!(labels.windows(2).all(|w| natural_cmp(&w[0], &w[1]) != std::cmp::Ordering::Greater));
    }

    #[test]
    fn every_offered_function_has_a_label() {
        for f in ToolbarFn::all() {
            if f != ToolbarFn::None {
                assert!(!f.short_label().is_empty(), "{f:?}");
            }
        }
    }
}
