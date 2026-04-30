use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use tiny_skia::*;

use crate::project::{Direction, Project};

pub struct RenderConfig {
    pub nvidia: bool,
    pub cq: u8,
}

pub struct RenderJob {
    pub input_video: String,
    pub project: Project,
    pub video_width: u32,
    pub video_height: u32,
    pub fps: f64,
    pub total_frames: u64,
    pub render_cfg: RenderConfig,
}

impl RenderJob {
    pub fn render(&self) -> Result<()> {
        self.run()
    }

    fn run(&self) -> Result<()> {
        // FFmpeg overlay pipeline:
        // We generate raw RGBA frames on stdout and pipe them as a second input.
        // FFmpeg overlays them onto the source video.
        let mut ffmpeg = spawn_ffmpeg_overlay(
            &self.input_video,
            &self.project.output,
            self.video_width,
            self.video_height,
            self.fps,
            &self.render_cfg,
        )?;

        let stdin = ffmpeg.stdin.as_mut().context("no ffmpeg stdin")?;

        let bar = &self.project.bar;
        let bar_y_px = (bar.bar_y * self.video_height as f32) as f32;
        let half_h = bar.bar_height / 2.0;
        let end_x_px = bar.end_circle_x * self.video_width as f32;

        for frame_idx in 0..self.total_frames {
            let t = frame_idx as f64 / self.fps;
            let pixels = draw_frame(
                self.video_width,
                self.video_height,
                t,
                bar_y_px,
                half_h,
                end_x_px,
                &self.project,
            );
            stdin.write_all(&pixels)?;
        }

        drop(ffmpeg.stdin.take());
        ffmpeg.wait()?;
        Ok(())
    }
}

fn draw_frame(
    w: u32,
    h: u32,
    t: f64,
    bar_y: f32,
    half_h: f32,
    end_x: f32,
    project: &Project,
) -> Vec<u8> {
    let mut pixmap = Pixmap::new(w, h).unwrap();
    let bar = &project.bar;

    // Draw bar background
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::from_rgba8(0, 0, 0, bar.bar_opacity));
    paint.anti_alias = false;
    let bar_rect = Rect::from_xywh(0.0, bar_y - half_h, w as f32, bar.bar_height).unwrap();
    pixmap.fill_rect(bar_rect, &paint, Transform::identity(), None);

    // Draw end circle (ring) — outer edge at end_circle_radius, inner hole smaller than dot so dots overlap
    let mut paint = Paint::default();
    paint.set_color(bar.end_circle_color.to_skia());
    paint.anti_alias = true;
    let inner_r = bar.dot_radius * 0.6;
    let ring_width = (bar.end_circle_radius - inner_r).max(2.0);
    let ring_radius = (bar.end_circle_radius + inner_r) / 2.0;
    let path = {
        let mut pb = PathBuilder::new();
        pb.push_circle(end_x, bar_y, ring_radius);
        pb.finish().unwrap()
    };
    let stroke = Stroke { width: ring_width, ..Default::default() };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    // Draw dots
    let travel = bar.dot_travel_seconds as f64;
    let dot_color = bar.dot_color.to_skia();
    let video_w = w as f32;

    for dot in &project.dots {
        let hit_time = project.beat32_to_secs(dot.beat_32);
        let start_time = hit_time - travel;
        let end_time = hit_time;

        if t < start_time || t > end_time + 0.1 {
            continue;
        }

        let progress = ((t - start_time) / travel).clamp(0.0, 1.0) as f32;

        let xs: [f32; 2] = match bar.direction {
            Direction::LeftToRight    => [end_x * progress, 0.0],
            Direction::RightToLeft    => [video_w + (end_x - video_w) * progress, 0.0],
            Direction::BothDirections => [
                end_x * progress,
                video_w + (end_x - video_w) * progress,
            ],
        };
        let n = if bar.direction == Direction::BothDirections { 2 } else { 1 };

        let mut paint = Paint::default();
        paint.set_color(dot_color);
        paint.anti_alias = true;

        for &dot_x in &xs[..n] {
            let mut pb = PathBuilder::new();
            pb.push_circle(dot_x, bar_y, bar.dot_radius);
            if let Some(path) = pb.finish() {
                pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
    }

    pixmap.take()
}

fn spawn_ffmpeg_overlay(
    input: &str,
    output: &str,
    w: u32,
    h: u32,
    fps: f64,
    cfg: &RenderConfig,
) -> Result<Child> {
    let cq = cfg.cq.to_string();
    let (codec, preset, quality_flag) = if cfg.nvidia {
        ("h264_nvenc", "p4", "-cq")
    } else {
        ("libx264", "medium", "-crf")
    };

    let child = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input,
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &format!("{w}x{h}"),
            "-r", &fps.to_string(),
            "-i", "pipe:0",
            "-filter_complex", "[0:v][1:v]overlay=0:0[out]",
            "-map", "[out]",
            "-map", "0:a?",
            "-c:v", codec,
            "-preset", preset,
            quality_flag, &cq,
            "-c:a", "copy",
            output,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn ffmpeg")?;
    Ok(child)
}

pub fn probe_video(path: &str) -> Result<(u32, u32, f64, u64)> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            path,
        ])
        .output()
        .context("ffprobe failed")?;

    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = json["streams"].as_array().context("no streams")?;
    let video = streams.iter()
        .find(|s| s["codec_type"] == "video")
        .context("no video stream")?;

    let w = video["width"].as_u64().context("no width")? as u32;
    let h = video["height"].as_u64().context("no height")? as u32;

    // fps as fraction string like "30000/1001"
    let fps_str = video["r_frame_rate"].as_str().unwrap_or("30/1");
    let fps = parse_fraction(fps_str).unwrap_or(30.0);

    let nb_frames: u64 = video["nb_frames"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            let dur = video["duration"].as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            (dur * fps) as u64
        });

    Ok((w, h, fps, nb_frames))
}

fn parse_fraction(s: &str) -> Option<f64> {
    let mut parts = s.splitn(2, '/');
    let num: f64 = parts.next()?.parse().ok()?;
    let den: f64 = parts.next()?.parse().ok()?;
    if den == 0.0 { return None; }
    Some(num / den)
}
