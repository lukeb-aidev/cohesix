// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! seL4 root task hook for the Queen role.
//! Loads the boot namespace and emits early log messages.

use std::fs::{self, OpenOptions};
use std::io::Write;
use ureq::Agent;

use crate::boot::plan9_ns::load_namespace;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/dev/log") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Entry point for the Queen root task.
pub fn start() {
    match load_namespace("/srv/bootns") {
        Ok(ns) => log(&format!(
            "[queen] loaded {} namespace entries",
            ns.actions().len()
        )),
        Err(e) => log(&format!("[queen] failed to load namespace: {e}")),
    }

    if let Ok(url) = fs::read_to_string("/srv/cloudinit") {
        if let Ok(resp) = Agent::new().get(url.trim()).call() {
            if let Ok(body) = resp.into_string() {
                fs::create_dir_all("/srv/agents").ok();
                let _ = fs::write("/srv/agents/config.json", body);
            }
        }
    }

    fs::create_dir_all("/srv/bootstatus").ok();
    let _ = fs::write("/srv/bootstatus/queen", "ok");
}
