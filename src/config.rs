use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub https_port: u16,
    pub http_onboarding_port: u16,
    pub pairing_token: String,
    pub sensitivity: f32,
    pub scroll_speed: f32,
    pub preferred_ip: Option<String>,
    pub firewall_prompted: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            https_port: 8443,
            http_onboarding_port: 8442,
            pairing_token: generate_token(),
            sensitivity: 1.0,
            scroll_speed: 1.0,
            preferred_ip: None,
            firewall_prompted: false,
        }
    }
}

pub fn generate_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

pub fn app_data_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("AirPad")
}

pub fn config_file_path() -> PathBuf {
    app_data_dir().join("config.json")
}

impl Config {
    pub fn load_or_create() -> Self {
        let path = config_file_path();
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(mut cfg) = serde_json::from_str::<Config>(&raw) {
                if cfg.pairing_token.trim().is_empty() {
                    cfg.pairing_token = generate_token();
                    let _ = cfg.save();
                }
                return cfg;
            }
        }
        let cfg = Config::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, json)
    }
}
