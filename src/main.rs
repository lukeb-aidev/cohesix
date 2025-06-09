// CLASSIFICATION: COMMUNITY
// Filename: main.rs v1.2
// Date Modified: 2025-06-08
// Author: Lukas Bower
// Status: 🟢 Hydrated

//! Entry point for the Coh_CC compiler binary.

extern crate cohesix;
use cohesix::cli;
use cohesix::telemetry::trace::init_panic_hook;
use env_logger;

fn main() {
    env_logger::init();
    let boot_start = std::time::Instant::now();
    init_panic_hook();
    let result = std::panic::catch_unwind(|| {
        if let Err(err) = cli::run() {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    });

    use std::fs::{self, OpenOptions};
    use std::io::Write;

    match result {
        Ok(_) => {
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/log/sandbox_boot.log")
                .or_else(|_| {
                    fs::create_dir_all("/log").ok();
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/log/sandbox_boot.log")
                })
                .expect("log file");
            let _ = writeln!(log, "startup complete");
            cohesix::sandbox::validate();
        }
        Err(e) => {
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/log/sandbox_boot.log")
                .or_else(|_| {
                    fs::create_dir_all("/log").ok();
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/log/sandbox_boot.log")
                })
                .expect("log file");
            let _ = writeln!(log, "panic captured: {:?}", e);
            if let Err(err) = cli::run() {
                let _ = writeln!(log, "recovery failed: {}", err);
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
            let _ = writeln!(log, "recovered successfully");
            cohesix::sandbox::validate();
        }
    }
    let boot_time = boot_start.elapsed().as_millis();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/boot_time.log")
    {
        let _ = writeln!(f, "main {}ms", boot_time);
    }
}
