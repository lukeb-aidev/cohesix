// CLASSIFICATION: COMMUNITY
// Filename: busybox.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-22

//! Cohesix in-kernel BusyBox implementation.
//! Provides minimal command handlers for embedded shell and diagnostics.

use std::os::unix::fs::PermissionsExt;

/// Available BusyBox commands.
#[derive(Debug)]
pub enum BusyBoxCommand {
    Echo,
    Ls,
    Uname,
    Reboot,
    Cat,
    Touch,
    Mv,
    Cp,
    Ps,
    Kill,
    Chmod,
    Mount,
    Df,
    Uptime,
    Who,
    Unknown,
}

/// Dispatch a BusyBox command with optional arguments.
pub fn run_command(cmd: &str, args: &[&str]) {
    let command = match cmd {
        "echo" => BusyBoxCommand::Echo,
        "ls" => BusyBoxCommand::Ls,
        "uname" => BusyBoxCommand::Uname,
        "reboot" => BusyBoxCommand::Reboot,
        "cat" => BusyBoxCommand::Cat,
        "touch" => BusyBoxCommand::Touch,
        "mv" => BusyBoxCommand::Mv,
        "cp" => BusyBoxCommand::Cp,
        "ps" => BusyBoxCommand::Ps,
        "kill" => BusyBoxCommand::Kill,
        "chmod" => BusyBoxCommand::Chmod,
        "mount" => BusyBoxCommand::Mount,
        "df" => BusyBoxCommand::Df,
        "uptime" => BusyBoxCommand::Uptime,
        "who" => BusyBoxCommand::Who,
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
        BusyBoxCommand::Cat => {
            for path in args {
                match std::fs::read_to_string(path) {
                    Ok(content) => print!("{}", content),
                    Err(_) => println!("cat: {}: not found", path),
                }
            }
        }
        BusyBoxCommand::Touch => {
            for path in args {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path);
            }
        }
        BusyBoxCommand::Mv => {
            if args.len() == 2 {
                let _ = std::fs::rename(args[0], args[1]);
            } else {
                println!("mv: missing operand");
            }
        }
        BusyBoxCommand::Cp => {
            if args.len() == 2 {
                let _ = std::fs::copy(args[0], args[1]);
            } else {
                println!("cp: missing operand");
            }
        }
        BusyBoxCommand::Ps => {
            println!("  PID CMD");
            println!("    1 shell");
        }
        BusyBoxCommand::Kill => {
            if let Some(pid) = args.first() {
                println!("killed {}", pid);
            } else {
                println!("kill: missing pid");
            }
        }
        BusyBoxCommand::Chmod => {
            if args.len() == 2 {
                if let Ok(mode) = u32::from_str_radix(args[0], 8) {
                    use std::fs;
                    let _ = fs::set_permissions(args[1], fs::Permissions::from_mode(mode));
                } else {
                    println!("chmod: invalid mode");
                }
            } else {
                println!("chmod: usage chmod MODE FILE");
            }
        }
        BusyBoxCommand::Mount => {
            if args.len() >= 2 {
                println!("mounted {} on {}", args[0], args[1]);
            } else {
                println!("mount: missing args");
            }
        }
        BusyBoxCommand::Df => {
            println!("Filesystem     1K-blocks  Used Available Use% Mounted on");
            println!("/dev/root            2048  1024      1024 50% /");
        }
        BusyBoxCommand::Uptime => {
            println!("uptime: 0 days, 0:00");
        }
        BusyBoxCommand::Who => {
            println!("root console");
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
