// CLASSIFICATION: COMMUNITY
// Filename: kiosk.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! KioskInteractive role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/dev/log") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Start kiosk environment.
pub fn start() {
    fs::create_dir_all("/srv").ok();
    let _ = fs::write("/srv/kiosk_log", "ready");
    let _ = fs::write("/srv/banner", "Welcome");
    log("[kiosk] ready");
}
