use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub render: RenderConfig,
}

#[derive(Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_uploads_dir")]
    pub uploads_dir: String,
    #[serde(default)]
    pub audd_token: String,
}

#[derive(Deserialize)]
pub struct RenderConfig {
    #[serde(default = "default_true")]
    pub nvidia: bool,
    #[serde(default = "default_cq")]
    pub cq: u8,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            uploads_dir: default_uploads_dir(),
            audd_token: String::new(),
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            nvidia: true,
            cq: default_cq(),
        }
    }
}

fn default_bind() -> String { "127.0.0.1:7979".to_string() }
fn default_uploads_dir() -> String { "uploads".to_string() }
fn default_true() -> bool { true }
fn default_cq() -> u8 { 18 }

const DEFAULT_CONFIG: &str = r#"[server]
# Address and port the editor web UI is served on
bind = "127.0.0.1:7979"
# Directory where uploaded source videos are stored
uploads_dir = "uploads"
# audd.io API token for song identification (https://dashboard.audd.io)
audd_token = ""

[render]
# Use NVIDIA hardware encoding (h264_nvenc). Set to false to use software libx264.
nvidia = true
# Encode quality: lower = better quality, larger file. 18 is a good default.
# Maps to -cq for nvenc, -crf for libx264.
cq = 18
"#;

pub fn load() -> Result<Config> {
    let path = "config.toml";
    if !std::path::Path::new(path).exists() {
        std::fs::write(path, DEFAULT_CONFIG)?;
        return Ok(Config {
            server: ServerConfig::default(),
            render: RenderConfig::default(),
        });
    }
    let text = std::fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&text)?;
    Ok(cfg)
}
