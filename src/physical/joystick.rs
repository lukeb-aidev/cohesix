// CLASSIFICATION: COMMUNITY
// Filename: joystick.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-13

//! Joystick input via SDL2.
#![cfg(feature = "joystick")]
//!
//! Reads joystick axes and button states and logs them to `/srv/telemetry`
//! and `/srv/agent_trace/<id>`. Uses SDL2's joystick subsystem and falls back
//! to `None` if no controller is detected.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::recorder;

pub struct JoystickState {
    pub axes: Vec<i16>,
    pub buttons: Vec<bool>,
}

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

/// Read joystick state. Returns `None` if no joystick is available.
pub fn read_state(agent: &str) -> Option<JoystickState> {
    let sdl = sdl2::init().ok()?;
    let js = sdl.joystick().ok()?;
    if js.num_joysticks().ok()? <= 0 {
        return None;
    }
    let mut joy = js.open(0).ok()?;
    let axes = (0..joy.num_axes() as i32)
        .map(|i| joy.axis(i as u32).unwrap_or(0))
        .collect::<Vec<_>>();
    let buttons = (0..joy.num_buttons() as i32)
        .map(|i| joy.button(i as u32).unwrap_or(false))
        .collect::<Vec<_>>();

    fs::create_dir_all("/srv").ok();
    let detail = format!("axes {:?} buttons {:?}", axes, buttons);
    log("/srv/telemetry", &format!("{} joystick {}", ts(), detail));
    log(&format!("/srv/agent_trace/{agent}"), &format!("joystick {}", detail));
    recorder::event(agent, "joystick", &detail);

    Some(JoystickState { axes, buttons })
}
