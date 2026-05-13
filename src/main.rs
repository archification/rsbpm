mod config;
mod project;
mod renderer;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

static EDITOR_HTML: &str = include_str!("editor.html");

#[derive(Clone)]
struct AppState {
    cfg: Arc<config::Config>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Arc::new(config::load()?);

    let upload_dir = PathBuf::from(&cfg.server.uploads_dir);
    std::fs::create_dir_all(&upload_dir)?;

    let state = AppState { cfg: cfg.clone() };

    let app = Router::new()
        .route("/", get(serve_editor))
        .route("/upload/video", post(upload_video))
        .route("/render", post(render))
        .route("/identify", post(identify_song))
        .layer(DefaultBodyLimit::disable())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = &cfg.server.bind;
    println!("rsbpm editor → http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_editor() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        EDITOR_HTML,
    )
        .into_response()
}

#[derive(Serialize)]
struct UploadResponse {
    path: String,
}

async fn upload_video(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or("video.mp4").to_string();
        let safe_name = sanitize_filename(&filename);
        let dest = format!("{}/{safe_name}", state.cfg.server.uploads_dir);
        let bytes = field.bytes().await?;
        std::fs::write(&dest, &bytes)?;
        return Ok(Json(UploadResponse { path: dest }));
    }
    Err(AppError(anyhow::anyhow!("no file field in upload")))
}

#[derive(Deserialize)]
struct RenderRequest {
    video_path: String,
    project: project::Project,
    overlay_only: Option<bool>,
}

async fn render(
    State(state): State<AppState>,
    Json(req): Json<RenderRequest>,
) -> Result<StatusCode, AppError> {
    let video_path = req.video_path.clone();
    let project = req.project;
    let render_cfg = renderer::RenderConfig {
        nvidia: state.cfg.render.nvidia,
        cq: state.cfg.render.cq,
        overlay_only: req.overlay_only.unwrap_or(false),
    };

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (w, h, fps, frames) = renderer::probe_video(&video_path)?;
        let job = renderer::RenderJob {
            input_video: video_path,
            project,
            video_width: w,
            video_height: h,
            fps,
            total_frames: frames,
            render_cfg,
        };
        job.render()
    })
    .await??;

    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct IdentifyRequest {
    video_path: String,
    timestamp: f64,
}

async fn identify_song(
    State(state): State<AppState>,
    Json(req): Json<IdentifyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = state.cfg.server.audd_token.clone();
    if token.is_empty() {
        return Err(AppError(anyhow::anyhow!(
            "No audd_token set in config.toml — add audd_token = \"your-token\" under [server]"
        )));
    }

    let start = (req.timestamp - 5.0).max(0.0);

    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-ss", &start.to_string(),
            "-i", &req.video_path,
            "-t", "10",
            "-vn",
            "-acodec", "mp3",
            "-f", "mp3",
            "-loglevel", "error",
            "pipe:1",
        ])
        .output()
        .await
        .map_err(|e| AppError(anyhow::anyhow!("ffmpeg not found: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError(anyhow::anyhow!("ffmpeg failed: {stderr}")));
    }

    let form = reqwest::multipart::Form::new()
        .text("api_token", token)
        .part(
            "file",
            reqwest::multipart::Part::bytes(output.stdout)
                .file_name("audio.mp3")
                .mime_str("audio/mpeg")?,
        );

    let resp = reqwest::Client::new()
        .post("https://api.audd.io/")
        .multipart(form)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    Ok(Json(resp))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}
