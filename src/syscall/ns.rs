// CLASSIFICATION: COMMUNITY
// Filename: ns.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-09-30

use std::io::{self, ErrorKind};

use crate::cohesix_types::{RoleManifest, Syscall};
use crate::plan9::namespace::{Namespace, NamespaceLoader};
use crate::validator::syscall::validate_syscall;

/// Apply a namespace after validating permissions.
pub fn apply_ns(ns: &mut Namespace) -> io::Result<()> {
    let role = RoleManifest::current_role();
    println!("[syscall] apply_ns role: {:?}", role);
    if !validate_syscall(role.clone(), &Syscall::ApplyNamespace) {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            "apply_ns denied",
        ));
    }
    NamespaceLoader::apply(ns)
}
