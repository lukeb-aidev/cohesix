// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

use crate::cohesix_types::{Role, Syscall};
use crate::syscall::guard::check_permission;

/// Validate a syscall based on static role rules and guard defaults.
pub fn validate_syscall(role: Role, sc: &Syscall) -> bool {
    if matches!(
        (role.clone(), sc),
        (Role::QueenPrimary, Syscall::ApplyNamespace)
    ) {
        log::info!("explicit rule: QueenPrimary may ApplyNamespace");
        return true;
    }
    let allowed = check_permission(role.clone(), sc);
    if !allowed {
        log::warn!("syscall {:?} denied for {:?}", sc, role);
    }
    allowed
}
