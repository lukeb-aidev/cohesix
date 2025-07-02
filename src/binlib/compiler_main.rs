// CLASSIFICATION: COMMUNITY
// Filename: compiler_main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::cli;
use crate::prelude::*;
use crate::telemetry::trace::init_panic_hook;
use env_logger;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::Instant;

/// Boot the Cohesix compiler binary.
pub fn run() {
    env_logger::init();
    let boot_start = Instant::now();
    init_panic_hook();
    let result = std::panic::catch_unwind(|| {
        if let Err(err) = cli::run() {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    });

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
            crate::sandbox::validate();
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
            crate::sandbox::validate();
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compile_path_exists() {
        // this test simply ensures module compiles and run() is callable
        let _ = Instant::now();
    }
}
