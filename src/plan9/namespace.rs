// CLASSIFICATION: COMMUNITY
// Filename: namespace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Plan 9 style namespace parser and builder.
//! Reads /boot/plan9.cfg and exposes the resulting namespace at /srv/bootns.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};

/// Supported namespace operations.
#[derive(Debug)]
pub enum NsOp {
    Bind(String, String),
    Mount(String, String),
    Srv(String),
    Unmount(String),
}

/// Namespace represented as an ordered list of operations.
#[derive(Debug, Default)]
pub struct Namespace {
    pub ops: Vec<NsOp>,
}

impl Namespace {
    /// Parse a configuration file in the simple Plan 9 format.
    pub fn from_cfg(path: &str) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut ns = Namespace::default();
        for line in reader.lines() {
            let line = line?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() || parts[0].starts_with('#') {
                continue;
            }
            match parts[0] {
                "bind" if parts.len() >= 3 => ns.ops.push(NsOp::Bind(parts[1].into(), parts[2].into())),
                "mount" if parts.len() >= 3 => ns.ops.push(NsOp::Mount(parts[1].into(), parts[2].into())),
                "srv" if parts.len() >= 2 => ns.ops.push(NsOp::Srv(parts[1].into())),
                "unmount" if parts.len() >= 2 => ns.ops.push(NsOp::Unmount(parts[1].into())),
                _ => eprintln!("[namespace] ignoring invalid line: {}", line),
            }
        }
        Ok(ns)
    }

    /// Write the namespace operations to /srv/bootns for inspection.
    pub fn expose(&self) -> io::Result<()> {
        fs::create_dir_all("/srv").ok();
        let mut f = File::create("/srv/bootns")?;
        for op in &self.ops {
            match op {
                NsOp::Bind(src, dst) => writeln!(f, "bind {} {}", src, dst)?,
                NsOp::Mount(src, dst) => writeln!(f, "mount {} {}", src, dst)?,
                NsOp::Srv(name) => writeln!(f, "srv {}", name)?,
                NsOp::Unmount(path) => writeln!(f, "unmount {}", path)?,
            }
        }
        Ok(())
    }
}

/// Convenience helper to parse the default config and expose it.
pub fn init_boot_namespace() -> io::Result<Namespace> {
    let ns = Namespace::from_cfg("/boot/plan9.cfg")?;
    ns.expose().ok();
    Ok(ns)
}

