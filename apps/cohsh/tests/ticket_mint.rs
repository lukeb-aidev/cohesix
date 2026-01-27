// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate host-side ticket minting helpers.
// Author: Lukas Bower

use anyhow::Result;
use cohesix_ticket::{Role, TicketKey, TicketToken};
use cohsh::ticket_mint::{
    default_budget_for_role, mint_ticket_from_config, mint_ticket_from_secret, TicketMintRequest,
};
use std::io::Write;

#[test]
fn mint_ticket_from_secret_round_trips() -> Result<()> {
    let request = TicketMintRequest::new(Role::WorkerHeartbeat, Some("worker-1"), None)?;
    let token = mint_ticket_from_secret(&request, "worker-secret")?;
    let decoded = TicketToken::decode(&token, &TicketKey::from_secret("worker-secret"))?;
    let claims = decoded.claims();
    assert_eq!(claims.role, Role::WorkerHeartbeat);
    assert_eq!(claims.subject.as_deref(), Some("worker-1"));
    assert_eq!(
        claims.budget,
        default_budget_for_role(Role::WorkerHeartbeat)
    );
    Ok(())
}

#[test]
fn mint_ticket_from_config_uses_role_secret() -> Result<()> {
    let mut config = tempfile::NamedTempFile::new()?;
    writeln!(
        config,
        r#"[[tickets]]
role = "queen"
secret = "queen-secret"

[[tickets]]
role = "worker-heartbeat"
secret = "worker-secret"
"#
    )?;
    let request = TicketMintRequest::new(Role::WorkerHeartbeat, Some("worker-9"), None)?;
    let token = mint_ticket_from_config(&request, config.path())?;
    let decoded = TicketToken::decode(&token, &TicketKey::from_secret("worker-secret"))?;
    assert_eq!(decoded.claims().role, Role::WorkerHeartbeat);
    assert_eq!(decoded.claims().subject.as_deref(), Some("worker-9"));
    Ok(())
}

#[test]
fn worker_roles_require_subject() {
    let err = TicketMintRequest::new(Role::WorkerGpu, None, None).unwrap_err();
    assert!(err.to_string().contains("subject"));
}

#[test]
fn queen_subject_optional() -> Result<()> {
    let request = TicketMintRequest::new(Role::Queen, None, None)?;
    let token = mint_ticket_from_secret(&request, "queen-secret")?;
    let decoded = TicketToken::decode(&token, &TicketKey::from_secret("queen-secret"))?;
    let claims = decoded.claims();
    assert_eq!(claims.role, Role::Queen);
    assert!(claims.subject.is_none());
    assert_eq!(claims.budget, default_budget_for_role(Role::Queen));
    Ok(())
}
