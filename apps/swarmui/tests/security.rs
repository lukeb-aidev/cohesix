// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI unauthorized ticket handling and audit logs.
// Author: Lukas Bower

use anyhow::Result;
use cohsh::client::InProcessTransport;
use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketQuotas, TicketScope,
    TicketVerb,
};
use nine_door::NineDoor;
use swarmui::{SwarmUiBackend, SwarmUiConfig, SwarmUiTransportFactory};
use std::time::{SystemTime, UNIX_EPOCH};

struct InProcessFactory {
    server: NineDoor,
}

impl SwarmUiTransportFactory for InProcessFactory {
    type Transport = InProcessTransport;

    fn connect(&self) -> Result<Self::Transport, swarmui::SwarmUiError> {
        let connection = self
            .server
            .connect()
            .map_err(|err| swarmui::SwarmUiError::Transport(err.to_string()))?;
        Ok(InProcessTransport::new(connection))
    }
}

#[test]
fn unauthorized_ticket_returns_err_and_logs_audit() -> Result<()> {
    let server = NineDoor::new();
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let transcript = backend.attach(Role::WorkerHeartbeat, None);
    assert!(!transcript.ok);
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.starts_with("ERR ATTACH")));
    assert!(backend
        .audit_log()
        .iter()
        .any(|line| line.contains("audit swarmui.attach outcome=err")));
    Ok(())
}

#[test]
fn scope_violation_returns_eperm() -> Result<()> {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, "queen-secret");
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_scopes(vec![TicketScope::new("/proc/boot", TicketVerb::Read, 0)]);
    let token = TicketIssuer::new("queen-secret")
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();
    let transcript = backend.fleet_snapshot(Role::Queen, Some(token.as_str()));
    assert!(!transcript.ok);
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.starts_with("ERR CAT")));
    assert!(transcript.lines.iter().any(|line| line.contains("EPERM")));
    Ok(())
}

#[test]
fn bandwidth_quota_returns_elimit() -> Result<()> {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::Queen, "queen-secret");
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_scopes(vec![TicketScope::new("/", TicketVerb::Read, 0)])
    .with_quotas(TicketQuotas {
        bandwidth_bytes: Some(64),
        ..TicketQuotas::default()
    });
    let token = TicketIssuer::new("queen-secret")
        .issue(claims)
        .unwrap()
        .encode()
        .unwrap();
    let transcript = backend.fleet_snapshot(Role::Queen, Some(token.as_str()));
    assert!(!transcript.ok);
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.starts_with("ERR CAT")));
    assert!(transcript.lines.iter().any(|line| line.contains("ELIMIT")));
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
