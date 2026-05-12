use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn to_skia(&self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    LeftToRight,
    RightToLeft,
    BothDirections,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarConfig {
    /// 0.0–1.0 fractional Y position of bar center in video
    pub bar_y: f32,
    /// Height of bar in pixels
    pub bar_height: f32,
    /// Background bar opacity 0–255
    pub bar_opacity: u8,
    pub dot_color: Color,
    pub end_circle_color: Color,
    /// 0.0–1.0 fractional X position of the end circle
    pub end_circle_x: f32,
    pub direction: Direction,
    /// Radius of traveling dots in pixels
    pub dot_radius: f32,
    /// Radius of end circle in pixels
    pub end_circle_radius: f32,
    /// How many seconds before beat-time a dot enters from the opposite edge
    pub dot_travel_seconds: f32,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            bar_y: 0.98,
            bar_height: 60.0,
            bar_opacity: 35,
            dot_color: Color { r: 255, g: 182, b: 193, a: 255 }, // pastel pink
            end_circle_color: Color { r: 255, g: 255, b: 255, a: 220 },
            end_circle_x: 0.5,
            direction: Direction::BothDirections,
            dot_radius: 14.0,
            end_circle_radius: 22.0,
            dot_travel_seconds: 3.0,
        }
    }
}

/// A single dot event. `beat_32` is the position in 32nd-note units from time=0.
/// Fractional values are used in triplet sections (multiples of 4/3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotEvent {
    pub beat_32: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BeatChangeType {
    None,
    Half,
    Normal,
    Double,
    Triplet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatChange {
    pub beat_32: u32,
    #[serde(rename = "type")]
    pub change_type: BeatChangeType,
    /// Only meaningful when change_type == Normal; sets BPM from this point forward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fx {
    #[serde(rename = "speedUp", default = "default_true")]
    pub speed_up: bool,
    #[serde(rename = "speedPower", default = "default_speed_power")]
    pub speed_power: f32,
    #[serde(rename = "hitGlow", default = "default_true")]
    pub hit_glow: bool,
    #[serde(rename = "hitGlowDur", default = "default_hit_glow_dur")]
    pub hit_glow_dur: f64,
    #[serde(rename = "dotShrink", default = "default_true")]
    pub dot_shrink: bool,
    #[serde(rename = "shrinkDur", default = "default_shrink_dur")]
    pub shrink_dur: f64,
    #[serde(rename = "dotGlow", default = "default_true")]
    pub dot_glow: bool,
    #[serde(rename = "dotGlowInt", default = "default_dot_glow_int")]
    pub dot_glow_int: f32,
    #[serde(rename = "beatOffset", default)]
    pub beat_offset: f64,
}

fn default_true() -> bool { true }
fn default_speed_power() -> f32 { 2.5 }
fn default_hit_glow_dur() -> f64 { 0.25 }
fn default_shrink_dur() -> f64 { 0.12 }
fn default_dot_glow_int() -> f32 { 0.7 }

impl Default for Fx {
    fn default() -> Self {
        Self {
            speed_up: true,
            speed_power: 2.5,
            hit_glow: true,
            hit_glow_dur: 0.25,
            dot_shrink: true,
            shrink_dur: 0.12,
            dot_glow: true,
            dot_glow_int: 0.7,
            beat_offset: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub bpm: f64,
    pub bar: BarConfig,
    #[serde(default)]
    pub fx: Fx,
    pub dots: Vec<DotEvent>,
    #[serde(default)]
    pub beat_changes: Vec<BeatChange>,
    /// Output file path (relative or absolute)
    pub output: String,
}

impl Project {
    /// Convert a 32nd-note position to seconds, respecting any tempo changes.
    /// Accepts fractional values for triplet positions.
    pub fn beat32_to_secs(&self, beat_32: f64) -> f64 {
        let mut tempo: Vec<(f64, f64)> = vec![(0.0, self.bpm)];
        let mut changes = self.beat_changes.clone();
        changes.sort_by(|a, b| a.beat_32.cmp(&b.beat_32));
        for bc in &changes {
            if bc.change_type == BeatChangeType::Normal {
                if let Some(bpm) = bc.bpm {
                    let b = bc.beat_32 as f64;
                    if b == 0.0 { tempo[0].1 = bpm; } else { tempo.push((b, bpm)); }
                }
            }
        }

        let mut secs = 0.0f64;
        let mut i = 0;
        while i + 1 < tempo.len() && tempo[i + 1].0 < beat_32 {
            let (b0, bpm0) = tempo[i];
            let b1 = tempo[i + 1].0;
            secs += (b1 - b0) / 8.0 * 60.0 / bpm0;
            i += 1;
        }
        let (b0, bpm0) = tempo[i];
        secs += (beat_32 - b0) / 8.0 * 60.0 / bpm0;
        secs
    }
}
