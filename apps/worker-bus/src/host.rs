// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side field bus worker descriptors and mount helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Field bus worker scaffolding for host-mode tests and tooling.

use anyhow::{anyhow, Result};
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims};
use secure9p_codec::SessionId;

/// Paths exposed for a bus mount under `/bus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusPaths {
    /// Root mount path (`/bus/<mount>`).
    pub root: String,
    /// Control file path (`/bus/<mount>/ctl`).
    pub ctl: String,
    /// Telemetry file path (`/bus/<mount>/telemetry`).
    pub telemetry: String,
    /// Link state control path (`/bus/<mount>/link`).
    pub link: String,
    /// Replay trigger path (`/bus/<mount>/replay`).
    pub replay: String,
    /// Spool status path (`/bus/<mount>/spool`).
    pub spool: String,
}

impl BusPaths {
    /// Build bus paths for the given mount root and label.
    pub fn new(mount_at: &str, mount: &str) -> Result<Self> {
        let mount_at = mount_at.trim_end_matches('/');
        if mount_at.is_empty() || !mount_at.starts_with('/') {
            return Err(anyhow!("mount_at must be an absolute path"));
        }
        let root = format!("{}/{}", mount_at, mount);
        Ok(Self {
            root: root.clone(),
            ctl: format!("{root}/ctl"),
            telemetry: format!("{root}/telemetry"),
            link: format!("{root}/link"),
            replay: format!("{root}/replay"),
            spool: format!("{root}/spool"),
        })
    }
}

/// Worker descriptor for field bus adapters.
#[derive(Debug, Clone)]
pub struct BusWorker {
    ticket: TicketClaims,
    session: SessionId,
    scope: String,
    paths: BusPaths,
}

impl BusWorker {
    /// Create a bus worker bound to a scope and mount label.
    pub fn new(
        session: SessionId,
        scope: impl Into<String>,
        mount_at: impl Into<String>,
        mount: impl Into<String>,
    ) -> Result<Self> {
        let scope = scope.into();
        let mount_at = mount_at.into();
        let mount = mount.into();
        let paths = BusPaths::new(mount_at.as_str(), mount.as_str())?;
        let ticket = TicketClaims::new(
            Role::WorkerBus,
            BudgetSpec::default_heartbeat(),
            Some(scope.clone()),
            MountSpec::empty(),
            0,
        );
        Ok(Self {
            ticket,
            session,
            scope,
            paths,
        })
    }

    /// Return the capability ticket template.
    #[must_use]
    pub fn ticket(&self) -> &TicketClaims {
        &self.ticket
    }

    /// Return the bus scope identifier.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Return the session identifier bound to the worker.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Return the generated bus paths for this worker.
    #[must_use]
    pub fn paths(&self) -> &BusPaths {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_paths_are_rooted() {
        let paths = BusPaths::new("/bus", "bus-1").expect("paths");
        assert_eq!(paths.root, "/bus/bus-1");
        assert_eq!(paths.ctl, "/bus/bus-1/ctl");
        assert_eq!(paths.telemetry, "/bus/bus-1/telemetry");
        assert_eq!(paths.replay, "/bus/bus-1/replay");
    }

    #[test]
    fn bus_worker_uses_scope_ticket() {
        let worker = BusWorker::new(SessionId::from_raw(4), "scope-1", "/bus", "bus-1")
            .expect("worker");
        assert_eq!(worker.scope(), "scope-1");
        assert_eq!(worker.ticket().role, Role::WorkerBus);
        assert_eq!(worker.session(), SessionId::from_raw(4));
    }
}
