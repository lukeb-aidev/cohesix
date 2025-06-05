// CLASSIFICATION: COMMUNITY
// Filename: busybox.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Cohesix in-kernel BusyBox implementation.
//! Provides minimal command handlers for embedded shell and diagnostics.

/// Available BusyBox commands.
#[derive(Debug)]
pub enum BusyBoxCommand {
    Echo,
    Ls,
    Uname,
    Reboot,
    Unknown,
}

/// Dispatch a BusyBox command with optional arguments.
pub fn run_command(cmd: &str, args: &[&str]) {
    let command = match cmd {
        "echo" => BusyBoxCommand::Echo,
        "ls" => BusyBoxCommand::Ls,
        "uname" => BusyBoxCommand::Uname,
        "reboot" => BusyBoxCommand::Reboot,
        _ => BusyBoxCommand::Unknown,
    };

    match command {
        BusyBoxCommand::Echo => {
            println!("{}", args.join(" "));
        }
        BusyBoxCommand::Ls => {
            use super::initfs;
            let files: Vec<_> = initfs::list_files().collect();
            if files.is_empty() {
                println!("[busybox] (empty)");
            } else {
                for f in files {
                    println!("{}", f);
                }
            }
        }
        BusyBoxCommand::Uname => {
            println!("Cohesix Kernel v0.1");
        }
        BusyBoxCommand::Reboot => {
            println!("[busybox] rebooting...");
            // In simulation just print a message; real implementation would
            // jump to the watchdog or hardware reset vector.
        }
        BusyBoxCommand::Unknown => {
            println!("Unknown command: {}", cmd);
        }
    }
}
