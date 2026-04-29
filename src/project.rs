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
            bar_y: 0.9,
            bar_height: 60.0,
            bar_opacity: 180,
            dot_color: Color { r: 255, g: 182, b: 193, a: 255 }, // pastel pink
            end_circle_color: Color { r: 255, g: 255, b: 255, a: 220 },
            end_circle_x: 0.85,
            direction: Direction::LeftToRight,
            dot_radius: 14.0,
            end_circle_radius: 22.0,
            dot_travel_seconds: 2.0,
        }
    }
}

/// A single dot event. `beat_32` is the index in 32nd-note units from time=0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotEvent {
    pub beat_32: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub bpm: f64,
    pub bar: BarConfig,
    pub dots: Vec<DotEvent>,
    /// Output file path (relative or absolute)
    pub output: String,
}

impl Project {
    /// Convert a 32nd-note index to seconds.
    pub fn beat32_to_secs(&self, beat_32: u32) -> f64 {
        let beats = beat_32 as f64 / 8.0; // 8 thirty-second notes per quarter note
        beats * 60.0 / self.bpm
    }
}
