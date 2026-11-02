use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const DEFAULT_CONFIG: &str = include_str!("../config.example.toml");
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Config {
   pub temp_min: f64,
   pub temp_max: f64,
   pub ema_alpha: f64,
   pub hysteresis_buffer: f64,
   pub poll_interval: u64,
   pub banner_update_ms: u64,
   pub zone_keywords: Vec<String>

}
// Default values for the configuration parameters
impl Default for Config {
    fn default() -> Self {
        Self {
            temp_min: 35.0,
            temp_max: 65.0,
            ema_alpha: 0.2,
            hysteresis_buffer: 1.0,
            poll_interval: 2,
            banner_update_ms: 500,
            zone_keywords: vec![
                "cpu".into(),
                "gpu".into(),
                "soc".into(),
                "core".into(),
                "tsadc".into(),
            ],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.temp_min >= self.temp_max {
            return Err("TEMP_MIN must be less than TEMP_MAX".into());
        }
        if self.ema_alpha <= 0.0 || self.ema_alpha >= 1.0 {
            return Err("EMA_ALPHA must be between 0 and 1".into());
        }
        if self.hysteresis_buffer < 0.0 {
            return Err("HYSTERESIS_BUFFER must be non-negative".into());
        }
        if self.poll_interval == 0 {
            return Err("POLL_INTERVAL must be greater than 0".into());
        }
        if self.banner_update_ms == 0 {
            return Err("BANNER_UPDATE_MS must be greater than 0".into());
        }
        Ok(())
    }
}

/// Write the default config file to `path`, creating parent folders if needed.
pub fn create_default(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, DEFAULT_CONFIG)
}

/// Read, parse and validate the config at `path`.
pub fn load(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    let config: Config = toml::from_str(&fs::read_to_string(path)?)?;
    config.validate()?;
    Ok(config)
}