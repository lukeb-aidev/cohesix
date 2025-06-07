// CLASSIFICATION: COMMUNITY
// Filename: namespace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-07

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
//! Dynamic Plan 9 namespace loader for Cohesix.
//!
//! This module parses namespace descriptions either from a boot argument
//! (`BOOT_NS` environment variable) or from `/boot/plan9.cfg` when the
//! environment variable is not present.  It supports the common Plan 9
//! commands `bind`, `mount`, `srv` and `unmount`.  Namespace entries are
//! executed at runtime by touching files under `/srv/` to simulate
//! service registration in tests.

use std::fs;
use std::io::{self, Read};

/// Namespace operation extracted from configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum NsOp {
    Bind {
        src: String,
        dst: String,
        after: bool,
    },
    Mount {
        srv: String,
        dst: String,
    },
    Srv {
        path: String,
    },
    Unmount {
        dst: String,
    },
}

/// Loaded namespace description with ordered operations.
#[derive(Debug, Default, Clone)]
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
=======
    pub fn add_op(&mut self, op: NsOp) {
        self.ops.push(op);
    }
}

/// Loader handling boot-time and runtime namespace files.
pub struct NamespaceLoader;

impl NamespaceLoader {
    /// Load namespace from BOOT_NS env var or `/boot/plan9.cfg`.
    pub fn load() -> io::Result<Namespace> {
        let text = if let Ok(cfg) = std::env::var("BOOT_NS") {
            cfg
        } else {
            fs::read_to_string("/boot/plan9.cfg").unwrap_or_default()
        };
        Ok(Self::parse(&text))
    }

    /// Parse textual namespace description.
    pub fn parse(text: &str) -> Namespace {
        let mut ns = Namespace::default();
        for line in text.lines() {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            match tokens.as_slice() {
                ["bind", "-a", src, dst] => ns.add_op(NsOp::Bind {
                    src: src.to_string(),
                    dst: dst.to_string(),
                    after: true,
                }),
                ["bind", src, dst] => ns.add_op(NsOp::Bind {
                    src: src.to_string(),
                    dst: dst.to_string(),
                    after: false,
                }),
                ["mount", srv, dst] => ns.add_op(NsOp::Mount {
                    srv: srv.to_string(),
                    dst: dst.to_string(),
                }),
                ["srv", path] => ns.add_op(NsOp::Srv {
                    path: path.to_string(),
                }),
                ["unmount", dst] => ns.add_op(NsOp::Unmount {
                    dst: dst.to_string(),
                }),
                _ => {
                    if !line.trim().is_empty() {
                        println!("[namespace] ignoring malformed line: {}", line);
                    }
                }
            }
        }
        ns
    }

    /// Apply namespace operations.  For the current simulation
    /// we simply create placeholder files under `/srv` to emulate
    /// the mounted services.
    pub fn apply(ns: &Namespace) -> io::Result<()> {
        fs::create_dir_all("/srv")?;
        for op in &ns.ops {
            match op {
                NsOp::Srv { path } => {
                    fs::write(path, b"srv")?;
                }
                NsOp::Mount { srv, dst } => {
                    let name = dst.trim_start_matches('/');
                    let file = format!("/srv/{}", name.replace('/', "_"));
                    fs::write(file, srv.as_bytes())?;
                }
                NsOp::Bind { .. } | NsOp::Unmount { .. } => {
                    // For simulation we just log the operation
                    println!("[namespace] {:?}", op);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let text = "bind / /\nmount tcp!1.2.3.4 /srv\nsrv /srv/foo";
        let ns = NamespaceLoader::parse(text);
        assert_eq!(ns.ops.len(), 3);
    }
}
