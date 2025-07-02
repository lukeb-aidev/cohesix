// CLASSIFICATION: COMMUNITY
// Filename: sensor.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17

/// SensorRelay role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/srv/devlog") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Start sensor relay.
pub fn start() {
    fs::create_dir_all("/srv").ok();
    let _ = fs::write("/srv/netrelay", "ready");
    log("[sensor] started");
}
