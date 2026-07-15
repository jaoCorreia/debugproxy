use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ScreensaverConfig {
    pub enabled: Option<bool>,
    #[serde(rename = "idleSeconds")]
    pub idle_seconds: Option<f64>,
    pub fps: Option<u32>,
    #[serde(rename = "cycleSeconds")]
    pub cycle_seconds: Option<f64>,
    #[serde(rename = "fadeSeconds")]
    pub fade_seconds: Option<f64>,
    pub theme: Option<String>,
    pub scenes: Option<String>,
    #[serde(rename = "wakeOnLog")]
    pub wake_on_log: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub port: Option<u16>,
    pub colors: HashMap<String, String>,
    pub screensaver: Option<ScreensaverConfig>,
}

pub fn base_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn load() -> Config {
    let path = base_dir().join("config.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&raw) {
            return cfg;
        }
    }
    Config::default()
}

pub fn resolve_port(config: &Config) -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .or(config.port)
        .unwrap_or(8888)
}
