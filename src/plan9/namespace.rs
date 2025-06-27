// CLASSIFICATION: COMMUNITY
// Filename: namespace.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-09-30

//! Dynamic Plan 9 namespace loader for Cohesix.
//!
//! Parses namespace descriptions from the `BOOT_NS` environment variable or
//! `/boot/plan9.cfg`. Supported operations are `bind`, `mount`, `srv` and
//! `unmount`. During tests, namespace actions are emulated by creating files
//! under `/srv`.

use crate::cohesix_types::{Role, RoleManifest};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};

/// Namespace operation extracted from configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindFlags {
    pub after: bool,
    pub before: bool,
    pub create: bool,
}

#[derive(Debug, Clone)]
pub enum NsOp {
    Bind {
        src: String,
        dst: String,
        flags: BindFlags,
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

#[derive(Default, Clone, Debug)]
pub struct NamespaceNode {
    pub mounts: Vec<String>,
    pub children: HashMap<String, NamespaceNode>,
}

impl NamespaceNode {
    pub fn get_or_create(&mut self, path: &str) -> &mut Self {
        let mut node = self;
        for part in path.trim_matches('/').split('/') {
            node = node.children.entry(part.to_string()).or_default();
        }
        node
    }
}

/// Loaded namespace description with ordered operations.
#[derive(Debug, Default, Clone)]
pub struct Namespace {
    pub ops: Vec<NsOp>,
    pub private: bool,
    pub root: NamespaceNode,
}

impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (idx, op) in self.ops.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            match op {
                NsOp::Bind { src, dst, flags } => {
                    let mut flag = String::new();
                    if flags.before { flag.push('b'); }
                    if flags.after { flag.push('a'); }
                    if flags.create { flag.push('c'); }
                    if flag.is_empty() {
                        write!(f, "bind {} {}", src, dst)?;
                    } else {
                        write!(f, "bind -{} {} {}", flag, src, dst)?;
                    }
                }
                NsOp::Mount { srv, dst } => write!(f, "mount {} {}", srv, dst)?,
                NsOp::Srv { path } => write!(f, "srv {}", path)?,
                NsOp::Unmount { dst } => write!(f, "unmount {}", dst)?,
            }
        }
        Ok(())
    }
}

impl Namespace {
    /// Add an operation to the namespace.
    pub fn add_op(&mut self, op: NsOp) {
        self.ops.push(op);
    }
}

/// Loader handling boot-time and runtime namespace files.
pub struct NamespaceLoader;

fn srv_root() -> std::path::PathBuf {
    std::env::var("COHESIX_SRV_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/srv"))
}

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
        if std::env::var("NS_PRIVATE").as_deref() == Ok("1") {
            ns.private = true;
        }
        for line in text.lines() {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.is_empty() || tokens[0].starts_with('#') {
                continue;
            }
            match tokens.as_slice() {
                ["bind", flag, src, dst] if flag.starts_with('-') => {
                    let mut f = BindFlags::default();
                    if flag.contains('a') {
                        f.after = true;
                    }
                    if flag.contains('b') {
                        f.before = true;
                    }
                    if flag.contains('c') {
                        f.create = true;
                    }
                    ns.add_op(NsOp::Bind {
                        src: src.to_string(),
                        dst: dst.to_string(),
                        flags: f,
                    });
                }
                ["bind", src, dst] => {
                    ns.add_op(NsOp::Bind {
                        src: src.to_string(),
                        dst: dst.to_string(),
                        flags: BindFlags::default(),
                    });
                }
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
                _ => println!("[namespace] ignoring malformed line: {}", line),
            }
        }
        ns
    }

    /// Apply namespace operations building the in-memory tree and `/srv` files.
    pub fn apply(ns: &mut Namespace) -> io::Result<()> {
        let srv_dir = srv_root();
        fs::create_dir_all(&srv_dir)?;
        for op in &ns.ops {
            match op {
                NsOp::Srv { path } => {
                    fs::write(path, b"srv")?;
                }
                NsOp::Mount { srv, dst } => {
                    let node = ns.root.get_or_create(dst);
                    node.mounts.push(srv.clone());
                    let name = dst.trim_start_matches('/');
                    let file = srv_dir.join(name.replace('/', "_"));
                    fs::write(file, srv.as_bytes())?;
                }
                NsOp::Bind { src, dst, flags } => {
                    let dst_node = ns.root.get_or_create(dst);
                    if flags.before {
                        dst_node.mounts.insert(0, src.clone());
                    } else {
                        dst_node.mounts.push(src.clone());
                    }
                    if flags.create {
                        ns.root.get_or_create(src);
                    }
                }
                NsOp::Unmount { dst } => {
                    let node = ns.root.get_or_create(dst);
                    node.mounts.clear();
                }
            }
        }
        Ok(())
    }
}

/// Convenience helper to parse the default config and expose it.
pub fn init_boot_namespace() -> io::Result<Namespace> {
    let mut ns = NamespaceLoader::load()?;
    NamespaceLoader::apply(&mut ns)?;
    let agent = std::env::var("AGENT_ID").unwrap_or_else(|_| "default".into());
    ns.persist(&agent)?;
    let role = RoleManifest::current_role();
    let _ = ns.dump_proc_nsmap(&role);
    Ok(ns)
}

impl Namespace {
    /// Write the namespace operations to `/srv/bootns` for inspection.
    pub fn expose(&self) -> io::Result<()> {
        let srv_dir = srv_root();
        fs::create_dir_all(&srv_dir).ok();
        let mut f = File::create(srv_dir.join("bootns"))?;
        for op in &self.ops {
            match op {
                NsOp::Bind { src, dst, flags } => {
                    let mut flag = String::new();
                    if flags.before {
                        flag.push('b');
                    }
                    if flags.after {
                        flag.push('a');
                    }
                    if flags.create {
                        flag.push('c');
                    }
                    if flag.is_empty() {
                        writeln!(f, "bind {} {}", src, dst)?;
                    } else {
                        writeln!(f, "bind -{} {} {}", flag, src, dst)?;
                    }
                }
                NsOp::Mount { srv, dst } => writeln!(f, "mount {} {}", srv, dst)?,
                NsOp::Srv { path } => writeln!(f, "srv {}", path)?,
                NsOp::Unmount { dst } => writeln!(f, "unmount {}", dst)?,
            }
        }
        Ok(())
    }

    /// Persist namespace for an agent under `/srv/bootns/<id>`.
    pub fn persist(&self, agent_id: &str) -> io::Result<()> {
        let dir = srv_root().join("bootns");
        fs::create_dir_all(&dir)?;
        let path = dir.join(agent_id);
        match fs::write(&path, self.to_string()) {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("[namespace] persist failed for {}: {}", path.display(), e);
                Err(e)
            }
        }
    }


    /// Resolve a path through the namespace and return the first mount target.
    pub fn resolve(&self, path: &str) -> Option<String> {
        let mut node = &self.root;
        for part in path.trim_matches('/').split('/') {
            match node.children.get(part) {
                Some(n) => node = n,
                None => return None,
            }
        }
        node.mounts.first().cloned()
    }

    /// Dump this namespace to `/proc/nsmap/<role>` for traceability.
    pub fn dump_proc_nsmap(&self, role: &Role) -> io::Result<()> {
        let role_name = match role {
            Role::Other(s) => s.clone(),
            _ => format!("{:?}", role),
        };
        fs::create_dir_all("/proc/nsmap")?;
        let path = format!("/proc/nsmap/{}", role_name);
        fs::write(path, self.to_string())
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
