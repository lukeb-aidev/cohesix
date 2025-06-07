// CLASSIFICATION: COMMUNITY
// Filename: namespace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

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
