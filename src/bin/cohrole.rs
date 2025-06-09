// CLASSIFICATION: COMMUNITY
// Filename: cohrole.rs v0.2
// Date Modified: 2025-07-21
// Author: Lukas Bower

//! Display the current Cohesix runtime role.

use std::env;
use std::fs;
use cohesix::telemetry::trace::init_panic_hook;

fn main() {
    init_panic_hook();
    let role = fs::read_to_string("/srv/cohrole")
        .ok()
        .or_else(|| env::var("COHROLE").ok())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("{}", role.trim());
}
