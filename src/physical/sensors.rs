// CLASSIFICATION: COMMUNITY
// Filename: sensors.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-21

//! Mock physical sensor model.
//!
//! Provides basic temperature, tilt and motion sensors which record values both
//! to `/srv/telemetry` and per-agent traces in `/srv/agent_trace/<id>`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::recorder;

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
    let value = 42.0; // mock value
    fs::create_dir_all("/srv/telemetry").ok();
    log("/srv/telemetry", &format!("{} temp {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("temp {}", value));
    recorder::event(agent, "sensor_temp", &format!("{}", value));
    value
}

pub fn read_tilt(agent: &str) -> f32 {
    let value = 0.0; // mock
    log("/srv/telemetry", &format!("{} tilt {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("tilt {}", value));
    recorder::event(agent, "sensor_tilt", &format!("{}", value));
    value
}

pub fn read_motion(agent: &str) -> bool {
    let value = false; // mock
    log("/srv/telemetry", &format!("{} motion {}", ts(), value));
    log(&format!("/srv/agent_trace/{agent}"), &format!("motion {}", value));
    recorder::event(agent, "sensor_motion", &format!("{}", value));
    value
}
