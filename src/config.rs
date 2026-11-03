use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const DEFAULT_CONFIG: &str = include_str!("../config.example.toml");
#[derive(Debug, Clone, Serialize, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub temp_min: f64,
    pub temp_max: f64,
    pub ema_alpha: f64,
    pub hysteresis_buffer: f64,
    pub poll_interval_secs: u64,          
    pub banner_update_ms: u64,
    pub zone_keywords: Vec<String>,
}
// Default values for the configuration parameters
impl Default for Config {
    fn default() -> Self {
        Self {
            temp_min: 35.0,
            temp_max: 65.0,
            ema_alpha: 0.2,
            hysteresis_buffer: 1.0,
            poll_interval_secs: 2,
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
        if self.poll_interval_secs == 0 {
            return Err("POLL_INTERVAL_SECS must be greater than 0".into());
        }
        if self.banner_update_ms == 0 {
            return Err("BANNER_UPDATE_MS must be greater than 0".into());
        }
        if self.zone_keywords.is_empty() {
            return Err("zone_keywords must not be empty".into());
        }
        if self.zone_keywords.iter().any(|kw| kw.is_empty()) {
            return Err("zone_keywords must not contain empty strings".into());
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
    let mut config: Config = toml::from_str(&fs::read_to_string(path)?)?;
    config.zone_keywords = config
        .zone_keywords
        .iter()
        .map(|kw| kw.trim().to_lowercase())
        .collect();
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_example_matches_defaults_and_validates() {
        let parsed: Config =
            toml::from_str(DEFAULT_CONFIG).expect("config.example.toml must parse");
        parsed.validate().expect("example config must be valid");
        assert_eq!(parsed, Config::default(), "template and Config::default() have drifted apart");
    }
    #[test]
    fn load_lowercases_and_trims_keywords() {
        let path = std::env::temp_dir().join(format!("tc-config-{}.toml", std::process::id()));
        fs::write(&path, "zone_keywords = [\"CPU\", \" Gpu \"]\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.zone_keywords, vec!["cpu", "gpu"]);
    }

    #[test]
    fn empty_keyword_is_rejected() {
        let cfg = Config {
            zone_keywords: vec![String::new()],
            ..Config::default()
        };
        assert!(cfg.validate().is_err());
    }

}