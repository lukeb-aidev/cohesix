// CLASSIFICATION: COMMUNITY
// Filename: plan9_ns.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! Plan 9 style namespace builder for early boot.
//! Parses boot arguments and produces a textual namespace description
//! compatible with Plan 9 bind and mount rules.

use std::fs;
use std::io;

/// Single namespace action.
#[derive(Debug, Clone, PartialEq)]
pub enum NsAction {
    /// `bind [ -a ] src dst`
    Bind {
        /// Source path
        src: String,
        /// Destination path
        dst: String,
        /// Bind after existing mounts
        after: bool,
    },
    /// `mount srv dst`
    Mount {
        /// Service name
        srv: String,
        /// Destination path
        dst: String,
    },
    /// `srv path`
    Srv {
        /// Service path
        path: String,
    },
}

/// Collection of namespace actions.
#[derive(Debug, Default, Clone)]
pub struct Namespace {
    actions: Vec<NsAction>,
}

impl Namespace {
    /// Create an empty namespace.
    pub fn new() -> Self {
        Self { actions: Vec::new() }
    }

    /// Add a bind command.
    pub fn bind(mut self, src: &str, dst: &str, after: bool) -> Self {
        self.actions.push(NsAction::Bind {
            src: src.to_string(),
            dst: dst.to_string(),
            after,
        });
        self
    }

    /// Add a mount command.
    pub fn mount(mut self, srv: &str, dst: &str) -> Self {
        self.actions.push(NsAction::Mount {
            srv: srv.to_string(),
            dst: dst.to_string(),
        });
        self
    }

    /// Add a srv entry.
    pub fn srv(mut self, path: &str) -> Self {
        self.actions.push(NsAction::Srv {
            path: path.to_string(),
        });
        self
    }

    /// Return reference to actions.
    pub fn actions(&self) -> &[NsAction] {
        &self.actions
    }

    /// Serialize namespace to a Plan 9 compatible text format.
    pub fn to_string(&self) -> String {
        let mut lines = Vec::new();
        for a in &self.actions {
            match a {
                NsAction::Bind { src, dst, after } => {
                    if *after {
                        lines.push(format!("bind -a {} {}", src, dst));
                    } else {
                        lines.push(format!("bind {} {}", src, dst));
                    }
                }
                NsAction::Mount { srv, dst } => {
                    lines.push(format!("mount {} {}", srv, dst));
                }
                NsAction::Srv { path } => {
                    lines.push(format!("srv {}", path));
                }
            }
        }
        lines.join("\n")
    }
}

/// Boot parameters extracted from kernel command line.
#[derive(Debug, Default)]
pub struct BootArgs {
    /// Root filesystem to mount as `/`.
    pub rootfs: String,
    /// Runtime role name.
    pub role: String,
    /// List of services to mount under `/srv`.
    pub srv: Vec<String>,
}

/// Parse boot arguments for `rootfs=`, `role=`, and `srv=` entries.
pub fn parse_boot_args(args: &[String]) -> BootArgs {
    let mut b = BootArgs::default();
    for a in args {
        if let Some(v) = a.strip_prefix("rootfs=") {
            b.rootfs = v.to_string();
        } else if let Some(v) = a.strip_prefix("role=") {
            b.role = v.to_string();
        } else if let Some(v) = a.strip_prefix("srv=") {
            b.srv.push(v.to_string());
        }
    }
    b
}

/// Build a default namespace from boot arguments.
pub fn build_namespace(args: &BootArgs) -> Namespace {
    let mut ns = Namespace::new();

    let root = if args.rootfs.is_empty() { "/" } else { &args.rootfs };
    if fs::metadata(root).is_err() {
        println!("[ns] warning: rootfs '{}' not found", root);
    }

    ns = ns.bind(root, "/", false)
        .bind("/bin", "/bin", true)
        .srv("/srv");

    for srv in &args.srv {
        ns = ns.mount(srv, "/srv");
    }

    ns
}

/// Write namespace description to `/srv/bootns`.
pub fn expose_namespace(ns: &Namespace) -> io::Result<()> {
    fs::create_dir_all("/srv")?;
    fs::write("/srv/bootns", ns.to_string())
}

/// Load a namespace from the `/srv/bootns` file.
pub fn load_namespace(path: &str) -> io::Result<Namespace> {
    let data = fs::read_to_string(path)?;
    Ok(parse_namespace(&data))
}

/// Parse namespace text format into a [`Namespace`].
pub fn parse_namespace(text: &str) -> Namespace {
    let mut ns = Namespace::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match tokens.as_slice() {
            ["bind", "-a", src, dst] => ns = ns.bind(src, dst, true),
            ["bind", src, dst] => ns = ns.bind(src, dst, false),
            ["mount", srv, dst] => ns = ns.mount(srv, dst),
            ["srv", path] => ns = ns.srv(path),
            _ => println!("[ns] ignoring malformed line: {}", line),
        }
    }
    ns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let args = BootArgs {
            rootfs: "/".into(),
            role: "QueenPrimary".into(),
            srv: vec!["tcp!1.2.3.4".into()],
        };
        let ns = build_namespace(&args);
        let text = ns.to_string();
        let parsed = parse_namespace(&text);
        assert_eq!(ns.actions(), parsed.actions());
    }
}

