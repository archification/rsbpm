mod project;
mod renderer;

use std::path::PathBuf;

use anyhow::Result;
use axum::{
    Json,
    Router,
    extract::{DefaultBodyLimit, Multipart},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

static EDITOR_HTML: &str = include_str!("editor.html");

#[tokio::main]
async fn main() -> Result<()> {
    let upload_dir = PathBuf::from("uploads");
    std::fs::create_dir_all(&upload_dir)?;

    let app = Router::new()
        .route("/", get(serve_editor))
        .route("/upload/video", post(upload_video))
        .route("/render", post(render))
        .layer(DefaultBodyLimit::disable())
        .layer(CorsLayer::permissive());

    let addr = "127.0.0.1:7979";
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

async fn upload_video(mut multipart: Multipart) -> Result<Json<UploadResponse>, AppError> {
    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .unwrap_or("video.mp4")
            .to_string();
        let safe_name = sanitize_filename(&filename);
        let dest = format!("uploads/{safe_name}");
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
}

async fn render(Json(req): Json<RenderRequest>) -> Result<StatusCode, AppError> {
    let video_path = req.video_path.clone();
    let project = req.project;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (w, h, fps, frames) = renderer::probe_video(&video_path)?;
        let job = renderer::RenderJob {
            input_video: video_path,
            project,
            video_width: w,
            video_height: h,
            fps,
            total_frames: frames,
        };
        job.render()
    })
    .await??;

    Ok(StatusCode::OK)
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
