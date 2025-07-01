// CLASSIFICATION: COMMUNITY
// Filename: cohrole.rs v0.3
// Date Modified: 2025-07-22
// Author: Lukas Bower

use crate::prelude::*;
//! Display the current Cohesix runtime role.

use cohesix::telemetry::trace::init_panic_hook;
use cohesix::binlib::role_main::current_role;

fn main() {
    init_panic_hook();
    let role = current_role();
    println!("{}", role.trim());
}
