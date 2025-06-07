// CLASSIFICATION: COMMUNITY
// Filename: cohrole.rs v0.1
// Date Modified: 2025-07-04
// Author: Lukas Bower

//! Display the current Cohesix runtime role.

use std::env;
use std::fs;

fn main() {
    let role = fs::read_to_string("/srv/cohrole")
        .ok()
        .or_else(|| env::var("COHROLE").ok())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("{}", role.trim());
}
