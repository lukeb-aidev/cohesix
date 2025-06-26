// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-27

//! Cohesix root task for seL4.
//! Initializes runtime, loads Plan9 namespace, and launches shell.


fn real_main() {
    cohesix::runtime::env::init::initialize_runtime_env();
    if let Err(e) = cohesix::rc::init::run() {
        eprintln!("[root] init failed: {e}");
    }
    cohesix::sh_loop::run();
}

fn main() {
    real_main();
}

#[no_mangle]
pub extern "C" fn _sel4_start() -> ! {
    real_main();
    loop {
        core::hint::spin_loop();
    }
}
