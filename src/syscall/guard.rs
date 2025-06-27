// CLASSIFICATION: COMMUNITY
// Filename: guard.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-09-30

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};

use crate::cohesix_types::{Role, Syscall};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SyscallOp {
    Spawn,
    CapGrant,
    Mount,
    Exec,
    ApplyNs,
}

impl From<&Syscall> for SyscallOp {
    fn from(sc: &Syscall) -> Self {
        match sc {
            Syscall::Spawn { .. } => SyscallOp::Spawn,
            Syscall::CapGrant { .. } => SyscallOp::CapGrant,
            Syscall::Mount { .. } => SyscallOp::Mount,
            Syscall::Exec { .. } => SyscallOp::Exec,
            Syscall::ApplyNamespace => SyscallOp::ApplyNs,
            Syscall::Unknown => SyscallOp::Exec,
        }
    }
}

pub static PERMISSIONS: Lazy<HashMap<Role, HashSet<SyscallOp>>> = Lazy::new(|| {
    use Role::*;
    use SyscallOp::*;
    let mut m: HashMap<Role, HashSet<SyscallOp>> = HashMap::new();
    m.insert(
        QueenPrimary,
        [Spawn, CapGrant, Mount, Exec, ApplyNs]
            .into_iter()
            .collect(),
    );
    m.insert(DroneWorker, [Spawn, CapGrant, Mount].into_iter().collect());
    m.insert(
        InteractiveAIBooth,
        [Spawn, CapGrant, Mount].into_iter().collect(),
    );
    m.insert(
        SimulatorTest,
        [Spawn, CapGrant, Mount, Exec].into_iter().collect(),
    );
    m
});

pub fn check_permission(role: Role, sc: &Syscall) -> bool {
    let op = SyscallOp::from(sc);
    let allowed = PERMISSIONS
        .get(&role)
        .map_or(false, |set| set.contains(&op));
    if !allowed {
        log::warn!("permission denied: {:?} for {:?}", op, role);
    } else {
        log::info!("permission allowed: {:?} for {:?}", op, role);
    }
    allowed
}
