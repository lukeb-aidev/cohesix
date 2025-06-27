// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-09-30

use crate::cohesix_types::{Role, Syscall};
use crate::syscall::guard::check_permission;
use crate::validator::record_syscall;

/// Validate a syscall based on static role rules and guard defaults.
pub fn validate_syscall(role: Role, sc: &Syscall) -> bool {
    println!("Validator received syscall: {:?}", sc);
    match (role.clone(), sc) {
        (Role::QueenPrimary, Syscall::ApplyNamespace) => {
            log::info!("explicit rule: QueenPrimary may ApplyNamespace");
            return true;
        }
        _ => {}
    }
    let allowed = check_permission(role.clone(), sc);
    if !allowed {
        println!(
            "Validator fallback deny: syscall {:?} not recognized for role {:?}",
            sc, role
        );
        log::warn!("syscall {:?} denied for {:?}", sc, role);
    }
    println!(
        "Validator: role={:?}, syscall={:?} -> {}",
        role, sc, allowed
    );
    record_syscall(sc);
    allowed
}
