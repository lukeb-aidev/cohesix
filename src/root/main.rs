// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-11-27

//! Cohesix root task for seL4.
//! Initializes runtime, loads Plan9 namespace, and launches shell.


fn real_main() {
    println!("[root] booting...");
    cohesix::runtime::env::init::initialize_runtime_env();
    println!("[root] runtime initialized");
    if let Err(e) = cohesix::rc::init::run() {
        eprintln!("[root] init failed: {e}");
    }
    for dir in ["/usr/cli", "/usr/bin"] {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let count = entries.count();
                println!("[root] {dir} contains {count} entries");
            }
            Err(_) => eprintln!("[root] {dir} missing"),
        }
    }
    println!("[root] launching shell loop");
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
