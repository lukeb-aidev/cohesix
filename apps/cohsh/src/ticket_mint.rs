// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-side ticket minting helper for cohsh and SwarmUI.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketScope, TicketVerb,
};
use cohsh_core::{parse_role, RoleParseMode};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TicketConfig {
    #[serde(default)]
    tickets: Vec<TicketEntry>,
}

#[derive(Debug, Deserialize)]
struct TicketEntry {
    role: String,
    #[serde(default)]
    secret: String,
    secret_ref: Option<String>,
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
    /// Explicit delegated mutation scopes; empty retains legacy attach semantics.
    pub scopes: Vec<TicketScope>,
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
            scopes: Vec::new(),
        })
    }
    /// Require a finite scoped caller ticket suitable for REST delegation.
    pub fn with_delegated_write_scope(
        self,
        path: &str,
        ttl_s: u64,
        operations: u64,
    ) -> Result<Self> {
        self.with_delegated_scope(path, TicketVerb::Write, ttl_s, operations)
    }

    /// Add a bounded read scope; the same path with write authority becomes read/write.
    pub fn with_delegated_read_scope(
        self,
        path: &str,
        ttl_s: u64,
        operations: u64,
    ) -> Result<Self> {
        self.with_delegated_scope(path, TicketVerb::Read, ttl_s, operations)
    }

    fn with_delegated_scope(
        mut self,
        path: &str,
        verb: TicketVerb,
        ttl_s: u64,
        operations: u64,
    ) -> Result<Self> {
        let subject = self
            .subject
            .as_deref()
            .ok_or_else(|| anyhow!("delegated ticket requires subject"))?;
        cohesix_authority::validate_id(subject).map_err(|error| anyhow!("{error}"))?;
        if !path.starts_with('/')
            || path.len() > cohsh_core::MAX_PATH_LEN
            || (path != "/"
                && path.split('/').skip(1).any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || part.bytes().any(|byte| byte.is_ascii_control())
                }))
            || ttl_s == 0
            || ttl_s > 86400
            || operations == 0
        {
            return Err(anyhow!("invalid delegated scope, TTL or operations"));
        }
        if let Some(scope) = self.scopes.iter_mut().find(|scope| scope.path == path) {
            if scope.verb != verb {
                scope.verb = TicketVerb::ReadWrite;
            }
        } else {
            if self.scopes.len() >= 16 {
                return Err(anyhow!("delegated scope limit"));
            }
            self.scopes.push(TicketScope::new(path, verb, 0));
        }
        self.budget = self.budget.with_ttl(Some(ttl_s)).with_ops(Some(operations));
        Ok(self)
    }
}

/// Return the default budget used when minting tickets for the role.
#[must_use]
pub fn default_budget_for_role(role: Role) -> BudgetSpec {
    match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerGpu => BudgetSpec::default_gpu(),
        Role::WorkerHeartbeat | Role::WorkerBus | Role::WorkerLora => {
            BudgetSpec::default_heartbeat()
        }
    }
}

/// Mint a ticket using the supplied shared secret.
pub fn mint_ticket_from_secret(request: &TicketMintRequest, secret: &str) -> Result<String> {
    let claims = TicketClaims::new(
        request.role,
        request.budget,
        request.subject.clone(),
        MountSpec::empty(),
        unix_time_ms()?,
    )
    .with_scopes(request.scopes.clone());
    let resolved;
    let secret = if secret.starts_with("env:") || secret.starts_with("file:") {
        resolved = cohesix_authority::secret::resolve_reference(secret)?;
        resolved.as_str()
    } else {
        secret
    };
    let token = TicketIssuer::new(secret)
        .issue(claims)
        .map_err(|err| anyhow!("failed to issue ticket: {err:?}"))?;
    let encoded = token
        .encode()
        .map_err(|err| anyhow!("failed to encode ticket: {err:?}"))?;
    if encoded.len() > cohsh_core::MAX_TICKET_LEN {
        return Err(anyhow!(
            "ticket exceeds transport ticket byte bound; shorten subject/scope"
        ));
    }
    Ok(encoded)
}

/// Mint a ticket using the role secret from the provided root_task.toml.
pub fn mint_ticket_from_config(request: &TicketMintRequest, config_path: &Path) -> Result<String> {
    let secret = load_ticket_secret(config_path, request.role)?;
    mint_ticket_from_secret(request, secret.as_str())
}

fn load_ticket_secret(config_path: &Path, role: Role) -> Result<String> {
    let payload = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read ticket config {}", config_path.display()))?;
    let config: TicketConfig = toml::from_str(&payload)
        .with_context(|| format!("failed to parse ticket config {}", config_path.display()))?;
    for entry in config.tickets {
        let parsed = parse_role(entry.role.as_str(), RoleParseMode::Strict);
        if parsed == Some(role) {
            return match entry.secret_ref {
                Some(reference) if entry.secret.is_empty() => {
                    cohesix_authority::secret::resolve_reference(&reference).map_err(Into::into)
                }
                Some(_) => Err(anyhow!("ticket secret sources are ambiguous")),
                None => Ok(entry.secret),
            };
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

fn unix_time_ms() -> Result<u64> {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()).map_err(Into::into)
}
