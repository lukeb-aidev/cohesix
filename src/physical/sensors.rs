// CLASSIFICATION: COMMUNITY
// Filename: sensors.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-13

//! Physical sensor interface.
//!
//! Reads normalized sensor data from `/srv/sensors/` and logs values both to
//! `/srv/telemetry` and per-agent traces in `/srv/agent_trace/<id>`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::recorder;
use serde_json;

fn log(path: &str, msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", msg);
    }
}

fn ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn read_temperature(agent: &str) -> f32 {
    let value = read_hw_temperature()
        .or_else(|| read_sensor_value("/srv/sensors/temperature.json"))
        .unwrap_or(42.0);
    fs::create_dir_all("/srv").ok();
    log("/srv/telemetry", &format!("{} temp {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("temp {}", value));
    recorder::event(agent, "sensor-triggered-action", &format!("temp:{}", value));
    value
}

pub fn read_tilt(agent: &str) -> f32 {
    let value = read_hw_accel()
        .or_else(|| read_sensor_value("/srv/sensors/accelerometer.json"))
        .unwrap_or(0.0);
    log("/srv/telemetry", &format!("{} tilt {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("tilt {}", value));
    recorder::event(agent, "sensor-triggered-action", &format!("tilt:{}", value));
    value
}

pub fn read_motion(agent: &str) -> bool {
    let value = read_hw_motion().unwrap_or(false);
    log("/srv/telemetry", &format!("{} motion {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("motion {}", value));
    recorder::event(agent, "sensor-triggered-action", &format!("motion:{}", value));
    value
}

fn read_sensor_value(path: &str) -> Option<f32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("value").and_then(|f| f.as_f64()))
        .map(|v| v as f32)
}

fn read_hw_temperature() -> Option<f32> {
    let env = std::env::var("MOCK_TEMP").ok().and_then(|v| v.parse().ok());
    if env.is_some() {
        return env;
    }
    let paths = ["/sys/class/thermal/thermal_zone0/temp", "/tmp/ina226_mock"];
    for p in paths.iter() {
        if let Ok(contents) = std::fs::read_to_string(p) {
            if let Ok(v) = contents.trim().parse::<f32>() {
                return Some(v / 1000.0);
            }
        }
    }
    None
}

fn read_hw_accel() -> Option<f32> {
    let env = std::env::var("MOCK_ACCEL").ok().and_then(|v| v.parse().ok());
    if env.is_some() {
        return env;
    }
    let paths = ["/sys/bus/iio/devices/iio:device0/in_accel_x_raw", "/tmp/accel_mock"];
    for p in paths.iter() {
        if let Ok(contents) = std::fs::read_to_string(p) {
            if let Ok(v) = contents.trim().parse::<f32>() {
                return Some(v);
            }
        }
    }
    None
}

fn read_hw_motion() -> Option<bool> {
    let env = std::env::var("MOCK_MOTION").ok().and_then(|v| v.parse().ok());
    if env.is_some() {
        return env;
    }
    let path = "/tmp/motion_mock";
    if let Ok(contents) = std::fs::read_to_string(path) {
        return Some(contents.trim() == "1");
    }
    None
}
