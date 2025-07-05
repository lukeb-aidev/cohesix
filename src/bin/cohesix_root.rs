// CLASSIFICATION: COMMUNITY
// Filename: cohesix_root.rs v0.2
// Author: Lukas Bower
// Date Modified: 2027-07-05
use cohesix::runtime::env::init::initialize_runtime_env;
use cohesix::plan9::namespace::{Namespace, NamespaceLoader, NsOp};
use std::fs;
use std::process::Command;

fn load_namespace() -> Namespace {
    let path = "/etc/plan9.ns";
    match fs::read_to_string(path) {
        Ok(data) => {
            println!("[ns] loading {path}");
            NamespaceLoader::parse(&data)
        }
        Err(e) => {
            println!("[ns] {path} missing ({e}); using fallback");
            let default = concat!(
                "bind /usr/coh/bin /bin\n",
                "bind /usr/plan9/bin /usr/plan9/bin\n",
                "srv /srv\n",
            );
            NamespaceLoader::parse(default)
        }
    }
}

fn apply_namespace(ns: &mut Namespace) {
    for op in &ns.ops {
        match op {
            NsOp::Bind { src, dst, .. } => println!("[ns] Binding {src} -> {dst}"),
            NsOp::Mount { srv, dst } => println!("[ns] Mounting {srv} -> {dst}"),
            NsOp::Srv { path } => println!("[ns] Srv {path}"),
            NsOp::Unmount { dst } => println!("[ns] Unmount {dst}"),
        }
    }
    if let Err(e) = NamespaceLoader::apply(ns) {
        eprintln!("[ns] apply failed: {e}");
    }
}

fn main() {
    initialize_runtime_env();
    let mut ns = load_namespace();
    apply_namespace(&mut ns);
    println!("[ns] Starting /bin/init");
    match Command::new("/bin/init").spawn() {
        Ok(mut child) => match child.wait() {
            Ok(status) => println!("[root] /bin/init exited with {:?}", status.code()),
            Err(e) => eprintln!("[root] wait failed: {e}"),
        },
        Err(e) => eprintln!("[root] failed to start /bin/init: {e}"),
    }
}
