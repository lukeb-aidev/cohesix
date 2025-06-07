// CLASSIFICATION: COMMUNITY
// Filename: namespace.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-20

//! Dynamic Plan 9 namespace loader for Cohesix.
//!
//! Parses namespace descriptions from the `BOOT_NS` environment variable or
//! `/boot/plan9.cfg`. Supported operations are `bind`, `mount`, `srv` and
//! `unmount`. During tests, namespace actions are emulated by creating files
//! under `/srv`.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};

/// Namespace operation extracted from configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum NsOp {
    Bind { src: String, dst: String, after: bool },
    Mount { srv: String, dst: String },
    Srv { path: String },
    Unmount { dst: String },
}

/// Loaded namespace description with ordered operations.
#[derive(Debug, Default, Clone)]
pub struct Namespace {
    pub ops: Vec<NsOp>,
}

impl Namespace {
    /// Add an operation to the namespace.
    pub fn add_op(&mut self, op: NsOp) {
        self.ops.push(op);
    }
}

/// Loader handling boot-time and runtime namespace files.
pub struct NamespaceLoader;

impl NamespaceLoader {
    /// Load namespace from `BOOT_NS` env var or `/boot/plan9.cfg`.
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
            if tokens.is_empty() || tokens[0].starts_with('#') {
                continue;
            }
            match tokens.as_slice() {
                ["bind", "-a", src, dst] => ns.add_op(NsOp::Bind { src: src.to_string(), dst: dst.to_string(), after: true }),
                ["bind", src, dst] => ns.add_op(NsOp::Bind { src: src.to_string(), dst: dst.to_string(), after: false }),
                ["mount", srv, dst] => ns.add_op(NsOp::Mount { srv: srv.to_string(), dst: dst.to_string() }),
                ["srv", path] => ns.add_op(NsOp::Srv { path: path.to_string() }),
                ["unmount", dst] => ns.add_op(NsOp::Unmount { dst: dst.to_string() }),
                _ => println!("[namespace] ignoring malformed line: {}", line),
            }
        }
        ns
    }

    /// Apply namespace operations. Creates placeholder files under `/srv`.
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
                    println!("[namespace] {:?}", op);
                }
            }
        }
        Ok(())
    }
}

/// Convenience helper to parse the default config and expose it.
pub fn init_boot_namespace() -> io::Result<Namespace> {
    let ns = NamespaceLoader::load()?;
    ns.expose()?;
    Ok(ns)
}

impl Namespace {
    /// Write the namespace operations to `/srv/bootns` for inspection.
    pub fn expose(&self) -> io::Result<()> {
        fs::create_dir_all("/srv").ok();
        let mut f = File::create("/srv/bootns")?;
        for op in &self.ops {
            match op {
                NsOp::Bind { src, dst, after } => {
                    if *after {
                        writeln!(f, "bind -a {} {}", src, dst)?;
                    } else {
                        writeln!(f, "bind {} {}", src, dst)?;
                    }
                }
                NsOp::Mount { srv, dst } => writeln!(f, "mount {} {}", srv, dst)?,
                NsOp::Srv { path } => writeln!(f, "srv {}", path)?,
                NsOp::Unmount { dst } => writeln!(f, "unmount {}", dst)?,
            }
        }
        Ok(())
    }
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
