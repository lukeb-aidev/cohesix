// CLASSIFICATION: COMMUNITY
// Filename: boot_path_test.rs v0.1
// Date Modified: 2025-07-07
// Author: Lukas Bower

use cohesix::kernel::boot::bootloader::BootAgent;

#[test]
fn boot_preflight_runs() {
    BootAgent::preflight_checks();
}
