use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use tiny_skia::*;

use crate::project::{Direction, Project};

const OUTPUT_FPS: f64 = 60.0;

pub struct RenderConfig {
    pub nvidia: bool,
    pub cq: u8,
    pub overlay_only: bool,
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
        let source_duration = self.total_frames as f64 / self.fps;
        let overlay_frames = (source_duration * OUTPUT_FPS).ceil() as u64;

        let mut ffmpeg = if self.render_cfg.overlay_only {
            spawn_ffmpeg_alpha(
                &self.project.output,
                self.video_width,
                self.video_height,
            )?
        } else {
            spawn_ffmpeg_overlay(
                &self.input_video,
                &self.project.output,
                self.video_width,
                self.video_height,
                &self.render_cfg,
            )?
        };

        let stdin = ffmpeg.stdin.as_mut().context("no ffmpeg stdin")?;

        let bar = &self.project.bar;
        let bar_y_px = bar.bar_y * self.video_height as f32;
        let half_h = bar.bar_height / 2.0;
        let end_x_px = bar.end_circle_x * self.video_width as f32;

        for frame_idx in 0..overlay_frames {
            let t = frame_idx as f64 / OUTPUT_FPS;
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

fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, paint: &Paint) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn radial_gradient(cx: f32, cy: f32, inner_r: f32, outer_r: f32, stops: Vec<GradientStop>) -> Option<Shader<'static>> {
    RadialGradient::new(
        Point::from_xy(cx, cy), inner_r,
        Point::from_xy(cx, cy), outer_r,
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )
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
    let fx = &project.fx;
    let travel = bar.dot_travel_seconds as f64;
    let video_w = w as f32;

    // Bar background
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0, 0, 0, bar.bar_opacity));
    paint.anti_alias = false;
    let bar_rect = Rect::from_xywh(0.0, bar_y - half_h, w as f32, bar.bar_height).unwrap();
    pixmap.fill_rect(bar_rect, &paint, Transform::identity(), None);

    // Hit glow (drawn behind the end circle)
    if fx.hit_glow {
        let max_glow = project.dots.iter()
            .filter_map(|dot| {
                let age = t - (project.beat32_to_secs(dot.beat_32) + fx.beat_offset);
                if age >= 0.0 && age < fx.hit_glow_dur {
                    Some((1.0 - age / fx.hit_glow_dur) as f32)
                } else {
                    None
                }
            })
            .fold(0.0f32, f32::max);

        if max_glow > 0.0 {
            let ec = &bar.end_circle_color;
            let (r, g, b) = (ec.r as f32 / 255.0, ec.g as f32 / 255.0, ec.b as f32 / 255.0);
            let base_a = ec.a as f32 / 255.0;
            let outer_r = bar.end_circle_radius * (1.0 + max_glow * 1.6);
            let inner_r = bar.end_circle_radius * 0.4;
            let stops = vec![
                GradientStop::new(0.0, Color::from_rgba(r, g, b, base_a * max_glow * 0.85).unwrap_or(Color::TRANSPARENT)),
                GradientStop::new(1.0, Color::from_rgba(r, g, b, 0.0).unwrap_or(Color::TRANSPARENT)),
            ];
            if let Some(shader) = radial_gradient(end_x, bar_y, inner_r, outer_r, stops) {
                let mut paint = Paint::default();
                paint.shader = shader;
                paint.anti_alias = true;
                fill_circle(&mut pixmap, end_x, bar_y, outer_r, &paint);
            }
        }
    }

    // End circle ring
    {
        let inner_r = bar.dot_radius * 0.6;
        let ring_width = (bar.end_circle_radius - inner_r).max(2.0);
        let ring_radius = (bar.end_circle_radius + inner_r) / 2.0;
        let mut pb = PathBuilder::new();
        pb.push_circle(end_x, bar_y, ring_radius);
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(bar.end_circle_color.to_skia());
            paint.anti_alias = true;
            let stroke = Stroke { width: ring_width, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }

    // Dots
    for dot in &project.dots {
        let hit_time = project.beat32_to_secs(dot.beat_32);
        let effect_start = hit_time + fx.beat_offset;
        let start_time = hit_time - travel;
        let post_effect = t - effect_start;

        let in_shrink = fx.dot_shrink && post_effect >= 0.0 && post_effect < fx.shrink_dur;
        let in_brief  = !fx.dot_shrink && t >= hit_time && t < hit_time + 0.1;
        let in_travel = t >= start_time && !in_shrink && t < hit_time;

        if !in_travel && !in_shrink && !in_brief { continue; }

        let (progress, dot_r) = if in_shrink {
            let shrink = (1.0 - post_effect / fx.shrink_dur).max(0.0) as f32;
            (1.0f32, bar.dot_radius * shrink)
        } else {
            let linear = ((t - start_time) / travel).clamp(0.0, 1.0) as f32;
            let p = if fx.speed_up && fx.speed_power > 1.0 { linear.powf(fx.speed_power) } else { linear };
            (p, bar.dot_radius)
        };

        if dot_r <= 0.0 { continue; }

        let xs: [f32; 2] = match bar.direction {
            Direction::LeftToRight    => [end_x * progress, 0.0],
            Direction::RightToLeft    => [video_w + (end_x - video_w) * progress, 0.0],
            Direction::BothDirections => [end_x * progress, video_w + (end_x - video_w) * progress],
        };
        let n = if bar.direction == Direction::BothDirections { 2 } else { 1 };

        let dc = &bar.dot_color;
        let (r, g, b) = (dc.r as f32 / 255.0, dc.g as f32 / 255.0, dc.b as f32 / 255.0);
        let base_a = dc.a as f32 / 255.0;

        for &dot_x in &xs[..n] {
            if fx.dot_glow && fx.dot_glow_int > 0.0 {
                let glow_r = dot_r * (1.0 + fx.dot_glow_int * 2.5);
                // Gradient covers only the visible ring (dot_r → glow_r), so there is no
                // high-alpha region hidden beneath the solid dot that can bleed through its
                // anti-aliased edge.
                let stops = vec![
                    GradientStop::new(0.0, Color::from_rgba(r, g, b, base_a * fx.dot_glow_int).unwrap_or(Color::TRANSPARENT)),
                    GradientStop::new(1.0, Color::from_rgba(r, g, b, 0.0).unwrap_or(Color::TRANSPARENT)),
                ];
                if let Some(shader) = radial_gradient(dot_x, bar_y, dot_r, glow_r, stops) {
                    let mut paint = Paint::default();
                    paint.shader = shader;
                    paint.anti_alias = true;
                    fill_circle(&mut pixmap, dot_x, bar_y, glow_r, &paint);
                }
            }

            let mut paint = Paint::default();
            paint.set_color(bar.dot_color.to_skia());
            paint.anti_alias = true;
            fill_circle(&mut pixmap, dot_x, bar_y, dot_r, &paint);
        }
    }

    pixmap.take_demultiplied()
}

fn spawn_ffmpeg_alpha(output: &str, w: u32, h: u32) -> Result<Child> {
    let fps_str = format!("{OUTPUT_FPS}");
    let size_str = format!("{w}x{h}");

    let child = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &size_str,
            "-r", &fps_str,
            "-i", "pipe:0",
            "-c:v", "prores_ks",
            "-profile:v", "4444",
            "-pix_fmt", "yuva444p10le",
            output,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn ffmpeg")?;
    Ok(child)
}

fn spawn_ffmpeg_overlay(
    input: &str,
    output: &str,
    w: u32,
    h: u32,
    cfg: &RenderConfig,
) -> Result<Child> {
    let cq = cfg.cq.to_string();
    let fps_str = format!("{OUTPUT_FPS}");
    let size_str = format!("{w}x{h}");
    let (codec, preset, quality_flag) = if cfg.nvidia {
        ("h264_nvenc", "p4", "-cq")
    } else {
        ("libx264", "medium", "-crf")
    };

    // Upsample source to OUTPUT_FPS. Composite in RGB space (format=rgb) so gradient
    // alpha blending isn't degraded by YUV 4:2:0 chroma subsampling.
    let filter = format!("[0:v]fps={OUTPUT_FPS}[src];[src][1:v]overlay=0:0:format=rgb[out]");

    let child = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input,
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "-s", &size_str,
            "-r", &fps_str,
            "-i", "pipe:0",
            "-filter_complex", &filter,
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
