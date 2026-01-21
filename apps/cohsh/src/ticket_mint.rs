// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-side ticket minting helper for cohsh and SwarmUI.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use cohsh_core::{parse_role, RoleParseMode};
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TicketConfig {
    #[serde(default)]
    tickets: Vec<TicketEntry>,
}

#[derive(Debug, Deserialize)]
struct TicketEntry {
    role: String,
    secret: String,
}

/// Parameters required to mint a capability ticket.
#[derive(Debug, Clone)]
pub struct TicketMintRequest {
    /// Ticket role to issue.
    pub role: Role,
    /// Optional subject identifier.
    pub subject: Option<String>,
    /// Budget limits to embed in the ticket.
    pub budget: BudgetSpec,
}

impl TicketMintRequest {
    /// Build a mint request with default budgets when none is supplied.
    pub fn new(role: Role, subject: Option<&str>, budget: Option<BudgetSpec>) -> Result<Self> {
        let subject = normalize_subject(role, subject)?;
        let budget = budget.unwrap_or_else(|| default_budget_for_role(role));
        Ok(Self {
            role,
            subject,
            budget,
        })
    }
}

/// Return the default budget used when minting tickets for the role.
#[must_use]
pub fn default_budget_for_role(role: Role) -> BudgetSpec {
    match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerGpu => BudgetSpec::default_gpu(),
        Role::WorkerHeartbeat | Role::WorkerBus | Role::WorkerLora => BudgetSpec::default_heartbeat(),
    }
}

/// Mint a ticket using the supplied shared secret.
pub fn mint_ticket_from_secret(request: &TicketMintRequest, secret: &str) -> Result<String> {
    let claims = TicketClaims::new(
        request.role,
        request.budget,
        request.subject.clone(),
        MountSpec::empty(),
        unix_time_ms(),
    );
    let token = TicketIssuer::new(secret)
        .issue(claims)
        .map_err(|err| anyhow!("failed to issue ticket: {err:?}"))?;
    token
        .encode()
        .map_err(|err| anyhow!("failed to encode ticket: {err:?}"))
}

/// Mint a ticket using the role secret from the provided root_task.toml.
pub fn mint_ticket_from_config(
    request: &TicketMintRequest,
    config_path: &Path,
) -> Result<String> {
    let secret = load_ticket_secret(config_path, request.role)?;
    mint_ticket_from_secret(request, secret.as_str())
}

fn load_ticket_secret(config_path: &Path, role: Role) -> Result<String> {
    let payload = fs::read_to_string(config_path).with_context(|| {
        format!(
            "failed to read ticket config {}",
            config_path.display()
        )
    })?;
    let config: TicketConfig = toml::from_str(&payload).with_context(|| {
        format!(
            "failed to parse ticket config {}",
            config_path.display()
        )
    })?;
    for entry in config.tickets {
        let parsed = parse_role(entry.role.as_str(), RoleParseMode::Strict);
        if parsed == Some(role) {
            return Ok(entry.secret);
        }
    }
    Err(anyhow!(
        "ticket secret for role {:?} not found in {}",
        role,
        config_path.display()
    ))
}

fn normalize_subject(role: Role, subject: Option<&str>) -> Result<Option<String>> {
    let trimmed = subject
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if trimmed.is_none() && role_requires_subject(role) {
        return Err(anyhow!("worker roles require a subject identity"));
    }
    Ok(trimmed)
}

fn role_requires_subject(role: Role) -> bool {
    matches!(
        role,
        Role::WorkerHeartbeat | Role::WorkerGpu | Role::WorkerBus | Role::WorkerLora
    )
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
