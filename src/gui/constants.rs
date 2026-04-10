// ── Color palette ─────────────────────────────────────────────────────────────

use eframe::egui::Color32;

pub const WIN: Color32 = Color32::from_rgb(13, 15, 18);
pub const SIDEBAR: Color32 = Color32::from_rgb(10, 12, 14);
pub const PANEL: Color32 = Color32::from_rgb(18, 21, 26);
pub const CARD: Color32 = Color32::from_rgb(24, 28, 34);
pub const CARD_ALT: Color32 = Color32::from_rgb(20, 24, 29);
pub const CHART_BG: Color32 = Color32::from_rgb(14, 17, 21);
pub const BORDER: Color32 = Color32::from_rgb(48, 54, 62);
pub const ACCENT: Color32 = Color32::from_rgb(68, 255, 161);
pub const ACCENT_2: Color32 = Color32::from_rgb(84, 186, 255);
pub const TEXT: Color32 = Color32::from_rgb(236, 240, 245);
pub const DIM: Color32 = Color32::from_rgb(144, 153, 166);
pub const SOFT: Color32 = Color32::from_rgb(104, 112, 124);
pub const OK: Color32 = Color32::from_rgb(68, 255, 161);
pub const WARN: Color32 = Color32::from_rgb(255, 198, 79);
pub const ERR: Color32 = Color32::from_rgb(255, 107, 107);

// ── Chart line colours (shared between the timeline chart and metric tiles) ───
//
// Using the same constants for both means the tile accent colour always matches
// the line drawn in the history chart for that metric.
pub const CH_GPU:   Color32 = Color32::from_rgb(68,  255, 161);  // green  – GPU utilization
pub const CH_TEMP:  Color32 = Color32::from_rgb(255, 147, 79);   // orange – GPU temperature
pub const CH_VRAM:  Color32 = Color32::from_rgb(199, 114, 255);  // purple – VRAM utilization
pub const CH_POWER: Color32 = Color32::from_rgb(80,  200, 255);  // cyan   – GPU power (TGP)

// ── Chart / layout constants ──────────────────────────────────────────────────

pub const HISTORY_LEN: usize = 20; // 20 samples × 3 s = 60 s
pub const EFFECT_COUNT: usize = 10;

// ── Effect tables ─────────────────────────────────────────────────────────────

/// Snake-case keys sent to the daemon via SetEffect.
pub const EFFECT_KEYS: &[&str] = &[
    "static",
    "static_gradient",
    "wave_gradient",
    "breathing_single",
    "breathing_dual",
    "spectrum_cycle",
    "rainbow_wave",
    "starlight",
    "ripple",
    "wheel",
];

/// Human-readable display labels shown in the UI.
pub const EFFECT_LABELS: &[&str] = &[
    "Static",
    "Static Gradient",
    "Wave Gradient",
    "Breathing Single",
    "Breathing Dual",
    "Spectrum Cycle",
    "Rainbow Wave",
    "Starlight",
    "Ripple",
    "Wheel",
];

pub const EFFECT_DESC: &[&str] = &[
    "Solid colour across the full keyboard.",
    "Static left-to-right gradient between two colours.",
    "Animated gradient wave moving across the board.",
    "Single-colour breathing pulse.",
    "Two-colour alternating breathing pulse.",
    "Cycles through the spectrum automatically.",
    "Rainbow wave with selectable direction.",
    "Random sparkles with density control.",
    "Reactive ripple with colour and speed.",
    "Rotating colour wheel with selectable direction.",
];

#[derive(Clone, Copy, Default)]
pub struct EffectFlags {
    pub c1: bool,
    pub c2: bool,
    pub spd: bool,
    pub dir: bool,
    pub den: bool,
    pub dur: bool,
}

#[rustfmt::skip]
pub const EFFECT_FLAGS: [EffectFlags; 10] = [
    EffectFlags { c1:true,  c2:false, spd:false, dir:false, den:false, dur:false },
    EffectFlags { c1:true,  c2:true,  spd:false, dir:false, den:false, dur:false },
    EffectFlags { c1:true,  c2:true,  spd:false, dir:false, den:false, dur:false },
    EffectFlags { c1:true,  c2:false, spd:false, dir:false, den:false, dur:true  },
    EffectFlags { c1:true,  c2:true,  spd:false, dir:false, den:false, dur:true  },
    EffectFlags { c1:false, c2:false, spd:true,  dir:false, den:false, dur:false },
    EffectFlags { c1:false, c2:false, spd:true,  dir:true,  den:false, dur:false },
    EffectFlags { c1:true,  c2:false, spd:false, dir:false, den:true,  dur:false },
    EffectFlags { c1:true,  c2:false, spd:true,  dir:false, den:false, dur:false },
    EffectFlags { c1:false, c2:false, spd:true,  dir:true,  den:false, dur:false },
];

// ── Effect helpers ────────────────────────────────────────────────────────────

pub fn normalize_effect_name(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

pub fn effect_index_from_name(name: &str) -> usize {
    let norm = normalize_effect_name(name);
    EFFECT_KEYS
        .iter()
        .position(|key| *key == norm)
        .or_else(|| {
            EFFECT_LABELS
                .iter()
                .position(|label| normalize_effect_name(label) == norm)
        })
        .unwrap_or(0)
}

pub fn effect_key(idx: usize) -> &'static str {
    EFFECT_KEYS[idx.min(EFFECT_KEYS.len() - 1)]
}

pub fn effect_label(idx: usize) -> &'static str {
    EFFECT_LABELS[idx.min(EFFECT_LABELS.len() - 1)]
}

pub fn effect_desc(idx: usize) -> &'static str {
    EFFECT_DESC[idx.min(EFFECT_DESC.len() - 1)]
}
