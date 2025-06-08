// CLASSIFICATION: COMMUNITY
// Filename: kiosk.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-12

//! KioskInteractive role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use serde_json::{json, Value};
use chrono::Utc;
use crate::runtime::env::init::detect_cohrole;

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
    if detect_cohrole() != "KioskInteractive" {
        log_denied("kiosk_start");
        return;
    }

    if let Err(e) = deploy_bundle("/srv/ui_bundle/kiosk_v1", "/mnt/kiosk_ui") {
        log(&format!("bundle deploy failed: {e}"));
        return;
    }
    append_event(json!({
        "timestamp": Utc::now().timestamp(),
        "event": "bundle_deployed",
        "version": "kiosk_v1"
    }));
    log("[kiosk] bundle deployed");
}

/// Emit a kiosk event to the federation log.
pub fn emit_event(event: &str, user: Option<&str>) {
    if detect_cohrole() != "KioskInteractive" {
        log_denied(event);
        return;
    }
    let mut obj = json!({
        "timestamp": Utc::now().timestamp(),
        "event": event
    });
    if let Some(u) = user {
        if let Value::Object(map) = &mut obj {
            map.insert("user_id".into(), Value::String(u.to_string()));
        }
    }
    append_event(obj);
}

fn append_event(ev: Value) {
    fs::create_dir_all("/srv").ok();
    let path = Path::new("/srv/kiosk_federation.json");
    let mut events: Vec<Value> = if path.exists() {
        serde_json::from_str(&fs::read_to_string(path).unwrap_or_default()).unwrap_or_default()
    } else {
        Vec::new()
    };
    events.push(ev);
    let _ = fs::write(path, serde_json::to_string_pretty(&events).unwrap());
}

fn deploy_bundle(src: &str, dst: &str) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = Path::new(dst).join(entry.file_name());
        if ty.is_dir() {
            deploy_bundle(entry.path().to_str().unwrap(), dest_path.to_str().unwrap())?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

fn log_denied(action: &str) {
    fs::create_dir_all("/srv/logs").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/logs/kiosk_access.log")
    {
        let _ = writeln!(f, "[{}] denied {action}", Utc::now().to_rfc3339());
    }
}
