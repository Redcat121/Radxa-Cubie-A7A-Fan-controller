//! # Decoupled Thermal Controller (EMA Smoothing)
//!
//! Monitors the `/sys/class/thermal` interface on a Radxa Cubie A7A board and
//! drives the PWM fan control devices to keep the hottest hot-spot within a
//! configurable temperature window.
//!
//! Design:
//!   * **Decoupled** – the control loop only ever writes the Linux sysfs fan
//!     interface. It never sends `ioctl` messages to the kernel, so it does not
//!     fight with `cpufreq` scaling, `congestionctl`, or any other tool that
//!     also manages `/sys/class/thermal`.
//!   * **EMA smoothing** – raw sensor noise is filtered with an exponential
//!     moving average so fan speed changes are gradual instead of jittery.
//!   * **Hysteresis** – the effective temperature the fan is driven by only
//!     updates when it moves past a small buffer, preventing rapid on/off fan
//!     oscillation (DCM2588 PWM fans have only ~12 discrete steps).
//!   * **Emergency panic hook** – if the binary panics, all fans are maxed so
//!     the hardware never overheats while the SoC is dead.
//!   * **Fail-safe on blind sensors** – if no thermal zone can be read, the
//!     fans run at maximum until readings return.
//!   * **Patient startup** – the hardware scan is retried for a while, since
//!     the `pwm_fan` driver can finish loading after the service starts.
//!
//! Build:
//!   ```sh
//!   cargo build --release
//!   sudo ./target/release/thermal-controller
//!   ```
mod config;
use config::Config;
use std::path::Path;
use std::fs;
use std::panic;
use std::println;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

// ---------------------------------------------------------------------------
// Configuration config
// ---------------------------------------------------------------------------

const CONFIG_PATH: &str = "/etc/thermal-controller/config.toml";
const DISCOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const DISCOVERY_ATTEMPTS: u32 = 10;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ThermalZone {
    name: String,
    path: String,
}

#[derive(Debug, Clone)]
struct CoolingDevice {
    name: String,
    path: String,
    max_state: i32,
}

/// Result of one hardware scan: `(zones, fans, warnings)`.
type Discovery = (Vec<ThermalZone>, Vec<CoolingDevice>, Vec<String>);

// ---------------------------------------------------------------------------
// System discovery
// ---------------------------------------------------------------------------

/// Scan the sysfs thermal interface for CPU/GPU zones and PWM fan devices.
///
/// The scan never fails on a single bad device – unreadable files and non-fan
/// cooling devices are skipped and reported as warnings so the controller
/// still starts with whatever is discoverable.
fn discover_system(config: &Config) -> Result<(Vec<ThermalZone>, Vec<CoolingDevice>, Vec<String>), String> {
    let root = "/sys/class/thermal";
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            return Err(format!("cannot read {}: {e}", root));
        }
    };

    let mut zones = Vec::new();
    let mut fans = Vec::new();
    let mut warnings = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if !name.starts_with("thermal_zone") && !name.starts_with("cooling_device") {
            continue;
        }

        let kind = match fs::read_to_string(path.join("type")) {
            Ok(t) => t.trim().to_lowercase(),
            Err(e) => {
                warnings.push(format!("cannot read type of {name}: {e}"));
                continue;
            }
        };

        if name.starts_with("thermal_zone") {
            let is_hotspot = config.zone_keywords.iter().any(|kw| kind.contains(kw));
            if !is_hotspot {
                continue;
            }
            let temp_path = path.join("temp");
            if fs::read_to_string(&temp_path).is_err() {
                warnings.push(format!("zone {name} exists but has no temp file"));
                continue;
            }
            zones.push(ThermalZone {
                name: kind,
                path: temp_path.to_string_lossy().into_owned(),
            });
        } else if name.starts_with("cooling_device") {
            // Only real PWM fans; skip `cpufreq` throttling devices.
            let is_fan = kind.contains("fan") || kind.contains("pwm");
            if !is_fan {
                continue;
            }
            let max_path = path.join("max_state");
            if fs::read_to_string(&max_path).is_err() {
                warnings.push(format!("fan {name} exists but has no max_state"));
                continue;
            }
            let max_state = match fs::read_to_string(&max_path)
                .ok()
                .and_then(|t| t.trim().parse::<i32>().ok())
            {
                Some(v) => v,
                None => {
                    warnings.push(format!("fan {name} has a non-numeric max_state"));
                    continue;
                }
            };
            let cur_path = path.join("cur_state");
            fans.push(CoolingDevice {
                name: kind,
                path: cur_path.to_string_lossy().into_owned(),
                max_state,
            });
        }
    }

    Ok((zones, fans, warnings))
}

/// Scan for zones and fans, retrying while the hardware is not ready.
///
/// `pwm_fan` can finish loading a few seconds after the service starts, so a
/// scan that finds no zone or no fan is repeated every
/// `DISCOVERY_RETRY_INTERVAL` for up to `DISCOVERY_ATTEMPTS` scans. Returns
/// the last scan, which may still be incomplete; the caller decides what to do
/// about that. Stops early if a shutdown signal arrives.
fn discover_with_retry(shutdown: &AtomicBool, config: &Config) -> Result<Discovery, String> {
    let mut attempt = 1;
    loop {
        let found = discover_system(config)?;
        let ready = !found.0.is_empty() && !found.1.is_empty();
        if ready || attempt >= DISCOVERY_ATTEMPTS || shutdown.load(Ordering::Relaxed) {
            return Ok(found);
        }
        println!("⏳ Waiting for a thermal zone and a fan (scan {attempt}/{DISCOVERY_ATTEMPTS})...");
        attempt += 1;
        sleep_unless_shutdown(DISCOVERY_RETRY_INTERVAL, shutdown);
    }
}

// ---------------------------------------------------------------------------
// Control logic
// ---------------------------------------------------------------------------

/// Read every zone and return the hottest valid reading as `(°C, zone name)`.
///
/// Unreadable, unparsable and non-finite readings are skipped. Returns `None`
/// when no zone produced a usable reading.
fn read_hottest(zones: &[ThermalZone]) -> Option<(f64, String)> {
    zones
        .iter()
        .filter_map(|zone| {
            let raw = fs::read_to_string(&zone.path).ok()?;
            let milli_c = raw.trim().parse::<f64>().ok()?; // sysfs reports milli-degrees
            let temp = milli_c / 1000.0;
            temp.is_finite().then(|| (temp, zone.name.clone()))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// EMA smoothing followed by a hysteresis gate.
///
/// Turns noisy raw readings into the temperature the fans should be driven by:
/// the effective value only moves once the smoothed reading differs from the
/// last applied value by at least `HYSTERESIS_BUFFER`.
struct TempFilter {
    ema: Option<f64>,
    applied: f64,
    ema_alpha: f64,
    hysteresis_buffer: f64,
}

impl TempFilter {
    fn new(config: &Config) -> Self {
        Self {
            ema: None,
            applied: config.temp_min,
            ema_alpha: config.ema_alpha,
            hysteresis_buffer: config.hysteresis_buffer,
        }
    }

    /// Feed one raw reading (°C) and return the effective temperature.
    fn update(&mut self, raw: f64) -> f64 {
        let smoothed = match self.ema {
            Some(prev) => self.ema_alpha * raw + (1.0 - self.ema_alpha) * prev,
            None => raw,
        };
        self.ema = Some(smoothed);

        if (smoothed - self.applied).abs() < self.hysteresis_buffer {
            self.applied
        } else {
            self.applied = smoothed;
            smoothed
        }
    }
}

/// Compute the fan target step (0..=max_state) for a temperature.
///
/// * `>= temp_max` → full speed
/// * `<= TEMP_MIN` → off
/// * in between    → linear mapping, rounded *up* (never under-cool)
fn compute_target(config: &Config, temp_c: f64, max_state: i32) -> i32 {
    if max_state <= 0 {
        return 0;
    }
    if temp_c >= config.temp_max {
        return max_state;
    }
    if temp_c <= config.temp_min {
        return 0;
    }
    let step = (config.temp_max - config.temp_min) / max_state as f64;
        let raw = (temp_c - config.temp_min) / step;
    raw.ceil() as i32
}

/// Apply a fan target to a device. Returns an error describing the failure if
/// the sysfs write is rejected, so callers never report a fan as set when it
/// was not.
fn set_fan(fan: &CoolingDevice, target: i32) -> Result<(), String> {
    fs::write(&fan.path, target.to_string())
        .map_err(|e| format!("cannot write {}: {e}", fan.path))
}
/// Drive every fan to its maximum step. Returns one message per failed write.
fn max_all_fans(fans: &[CoolingDevice]) -> Vec<String> {
    fans.iter()
        .filter_map(|fan| set_fan(fan, fan.max_state).err())
        .collect()
}
/// Milliseconds since the Unix epoch, or 0 if the clock is unavailable.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn sleep_unless_shutdown(total: Duration, shutdown: &AtomicBool) {
    const SLICE: Duration = Duration::from_millis(100);
    let mut remaining = total;
    while !remaining.is_zero() && !shutdown.load(Ordering::Relaxed) {
        let step = remaining.min(SLICE);
        thread::sleep(step);
        remaining -= step;
    }
}
// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Initializing Decoupled Thermal Controller (EMA Smoothing Edition)...");
    let path = Path::new(CONFIG_PATH);
    if !path.exists() {
        if let Err(e) = config::create_default(path) {
            eprintln!("⚠️  Could not create {CONFIG_PATH}: {e}");
        }
    }

    let config = match config::load(path) {
        Ok(c) => {
            println!("✅ Loaded config from {CONFIG_PATH}: {c:?}");
            c
        }
        Err(e) => {
            eprintln!("⚠️  Config error ({e}); using built-in defaults.");
            Config::default()
        }
    };
    let poll_interval_secs = Duration::from_secs(config.poll_interval_secs);
    // --- Signal traps -----------------------------------------------------
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, Arc::clone(&shutdown_signal))?;
    flag::register(SIGTERM, Arc::clone(&shutdown_signal))?;

    // --- Hardware scan ----------------------------------------------------
    let (zones, fans, warnings) = discover_with_retry(&shutdown_signal, &config)?;

    println!("✅ System scan complete.");
    println!("📊 Thermal zones found: {}", zones.len());
    println!("💨 Fan devices found:   {}", fans.len());

    if !warnings.is_empty() {
        println!("⚠️  Skipped {} device(s) during scan.", warnings.len());
        for w in &warnings {
            println!("   - {w}");
        }
    }

    if zones.is_empty() || fans.is_empty() {
        // Asked to stop while still waiting for the hardware: nothing to max.
        // Without sensors we cannot regulate, so do not leave a fan idle.
        for e in max_all_fans(&fans) {
            eprintln!("❌ {e}");
        }        
        // Asked to stop while still waiting for the hardware.
        if shutdown_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
        eprintln!("❌ Error: missing critical components (need ≥1 zone and ≥1 fan).");
        eprintln!("   Check your board exposes /sys/class/thermal and a PWM fan;");
        eprintln!("   you may need `modprobe pwm_fan` or a PWM fan driver loaded.");
        // Non-zero exit so systemd treats this as a failure and restarts the
        // service.
        std::process::exit(1);
    }

    // --- Emergency panic hook --------------------------------------------
    let fans_for_panic = fans.clone();
    panic::set_hook(Box::new(move |_panic_info| {
        eprintln!("\n🚨 Emergency! Application is panicking. Maxing all fans!");
        for fan in &fans_for_panic {
            let _ = fs::write(&fan.path, fan.max_state.to_string());
        }
    }));

    // --- Main monitoring loop --------------------------------------------
    let mut filter = TempFilter::new(&config);
    let mut last_status = 0u64;
    let mut sensors_lost = false;

    while !shutdown_signal.load(Ordering::Relaxed) {
        // 1. Find the hottest hot-spot on the board. If no sensor is readable
        //    the temperature is unknown, so fail safe: run the fans at maximum
        //    and leave the smoothing state untouched.
        let Some((global_max_temp, hottest_zone_name)) = read_hottest(&zones) else {
            if !sensors_lost {
                eprintln!("⚠️  No thermal zone is readable; forcing fans to maximum until readings return.");
                sensors_lost = true;
            }
            for e in max_all_fans(&fans) {
                println!("❌ {e}");
            }
            sleep_unless_shutdown(poll_interval_secs, &shutdown_signal);
            continue;
        };
        if sensors_lost {
            println!("✅ Thermal readings restored; resuming normal control.");
            sensors_lost = false;
        }

        // 2. Smooth the reading and apply hysteresis.
        let effective_temp = filter.update(global_max_temp);

        // 3. Drive every fan, remembering what was applied for the banner.
        let mut fan_status = Vec::with_capacity(fans.len());
        for fan in &fans {
            let target = compute_target(&config, effective_temp, fan.max_state);
            match set_fan(fan, target) {
                Ok(()) => fan_status.push(format!("{} {} / {}", fan.name, target, fan.max_state)),
                Err(e) => println!("❌ Failed to set {} to {}: {e}", fan.name, target),
            }
        }

        // 4. Status banner: one line per refresh, covering every fan.
        let now = now_ms();
        if now.saturating_sub(last_status) >= config.banner_update_ms {
            println!(
                "Status: [🔥 Hotspot: {} @ {:.1}°C | Applied: {:.1}°C] | Fans: {}",
                hottest_zone_name,
                global_max_temp,
                effective_temp,
                fan_status.join(", ")
            );
            last_status = now;
        }

        sleep_unless_shutdown(poll_interval_secs, &shutdown_signal);
    }

    // --- Graceful shutdown: max fans -------------------------------------
    println!("\n🛑 Shutdown detected. Forcing all fans to maximum before exit...");
    let errors = max_all_fans(&fans);
    for e in &errors {
        println!("❌ {e}");
    }
    if errors.is_empty() {
        println!("✅ All fans set to maximum.");
        println!("👋 Thermal controller terminated safely.");
    } else {
        eprintln!(
            "⚠️  Thermal controller terminated, but {} fan(s) could not be set to maximum.",
            errors.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests for the pure control logic (no hardware required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- compute_target ---------------------------------------------------

    #[test]
    fn target_is_zero_below_min() {
        let cfg = Config::default();
        assert_eq!(compute_target(&cfg, 34.0,12), 0);
    }

    #[test]
    fn target_is_max_at_or_above_max() {
        let cfg = Config::default();
        assert_eq!(compute_target(&cfg, 65.0, 12), 12);
        assert_eq!(compute_target(&cfg, 70.0, 12), 12);
        assert_eq!(compute_target(&cfg, 100.0, 12), 12);
    }

    #[test]
    fn target_roughly_midpoint() {
        // midpoint of 35..65 is 50°C → about mid-range of a 12-step fan
        let cfg = Config::default();
        let t = compute_target(&cfg, 50.0, 12);
        assert!((4..=8).contains(&t), "got {t}");
    }

    #[test]
    fn target_rounds_up_never_undercools() {
        // 38°C → ceil(1.2) = 2 ; 42°C → ceil(2.4) = 3 (strictly higher step)
        let cfg = Config::default();
        assert!(compute_target(&cfg, 42.0, 12) > compute_target(&cfg, 38.0, 12));
        // and the highest in-range value never exceeds max
        assert!(compute_target(&cfg, 64.9, 12) <= 12);
    }

    #[test]
    fn target_zero_for_nonpositive_max_state() {
        let cfg = Config::default();

        assert_eq!(compute_target(&cfg,50.0, 0), 0);
    }

    // --- set_fan ----------------------------------------------------------

    #[test]
    fn set_fan_writes_the_target() {
        let path = std::env::temp_dir().join(format!("tc-test-{}-fan", std::process::id()));
        let fan = CoolingDevice {
            name: "fan".into(),
            path: path.to_string_lossy().into_owned(),
            max_state: 12,
        };
        set_fan(&fan, 7).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "7");
    }

    #[test]
    fn set_fan_reports_write_failures() {
        let fan = CoolingDevice {
            name: "fan".into(),
            path: "/nonexistent-dir/cur_state".into(),
            max_state: 12,
        };
        let err = set_fan(&fan, 3).unwrap_err();
        assert!(err.contains("/nonexistent-dir/cur_state"), "got {err}");
    }

    // --- read_hottest -----------------------------------------------------

    /// Build a zone whose `temp` file holds `contents` (`None` = file missing).
    fn temp_zone(tag: &str, contents: Option<&str>) -> ThermalZone {
        let path = std::env::temp_dir().join(format!("tc-test-{}-{tag}", std::process::id()));
        match contents {
            Some(c) => fs::write(&path, c).unwrap(),
            None => {
                let _ = fs::remove_file(&path);
            }
        }
        ThermalZone {
            name: tag.to_string(),
            path: path.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn read_hottest_is_none_when_no_zone_is_readable() {
        let zones = [
            temp_zone("missing", None),
            temp_zone("garbage", Some("not a number")),
            temp_zone("nan", Some("nan")),
        ];
        assert!(read_hottest(&zones).is_none());
    }

    #[test]
    fn read_hottest_picks_the_hottest_readable_zone() {
        let zones = [
            temp_zone("cool", Some("41000\n")),
            temp_zone("hot", Some("58500\n")),
            temp_zone("broken", None),
        ];
        let (temp, name) = read_hottest(&zones).unwrap();
        assert_eq!(name, "hot");
        assert!((temp - 58.5).abs() < 1e-9);
    }

    // --- TempFilter (EMA smoothing + hysteresis) --------------------------

    #[test]
    fn filter_converges_toward_a_sustained_reading() {
        let cfg = Config::default();
        let mut filter = TempFilter::new(&cfg);
        filter.update(35.0); // seed at the floor
        let mut effective = 0.0;
        for _ in 0..50 {
            effective = filter.update(65.0);
            assert!(effective <= cfg.temp_max);
        }
        assert!(effective > 63.0, "got {effective}");
    }

    #[test]
    fn filter_ignores_changes_below_the_buffer() {
        let cfg = Config::default();
        let mut filter = TempFilter::new(&cfg);
        assert_eq!(filter.update(40.0), 40.0);
        // smoothed value moves by only ~0.1°C, well inside the buffer
        assert_eq!(filter.update(40.5), 40.0);
    }

    #[test]
    fn filter_applies_changes_above_the_buffer() {
        let cfg = Config::default();
        let mut filter = TempFilter::new(&cfg);
        filter.update(40.0);
        // first step: 0.2 * 50 + 0.8 * 40 = 42, which is >= 1°C away
        let effective = filter.update(50.0);
        assert!(effective > 41.0, "got {effective}");
    }
    #[test]
    fn default_config_round_trips() {
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.temp_max, Config::default().temp_max);
}

    #[test]
    fn sleep_returns_early_on_shutdown() {
        let flag = AtomicBool::new(true);
        let start = std::time::Instant::now();
        sleep_unless_shutdown(Duration::from_secs(30), &flag);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn max_all_fans_reports_each_failure() {
        let ok_path = std::env::temp_dir().join(format!("tc-test-{}-maxfan", std::process::id()));
        let fans = [
            CoolingDevice {
                name: "ok".into(),
                path: ok_path.to_string_lossy().into_owned(),
                max_state: 4,
            },
            CoolingDevice {
                name: "bad".into(),
                path: "/nonexistent-dir/cur_state".into(),
                max_state: 4,
            },
        ];
        let errors = max_all_fans(&fans);
        assert_eq!(errors.len(), 1);
        assert_eq!(fs::read_to_string(&ok_path).unwrap(), "4");
    }

}
