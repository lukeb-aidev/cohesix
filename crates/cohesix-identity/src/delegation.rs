// Author: Lukas Bower
// Purpose: Bind mapped delegation to generated policy and preserve issuer time across repeated exchanges.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{IdentityError, MappedRequest, Policy, Result, Rule, Scope};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketScope, TicketVerb,
};
use sha2::{Digest, Sha256};

/// Reserved namespace; unknown or changed mapped policies must fail closed.
pub const SUBJECT_PREFIX: &str = "mi-";

/// Derive a gateway-only issuer domain from the enrolled delegation secret.
/// A mapped ticket must not authenticate directly to a VM or an ordinary
/// delegation verifier, even if an operator reused the underlying secret.
pub fn gateway_issuer(secret: &str) -> TicketIssuer {
    let mut digest = Sha256::new();
    digest.update(b"cohesix-gateway-mapped-issuer/v1\0");
    digest.update(secret.as_bytes());
    TicketIssuer::new(&hex::encode(digest.finalize()))
}

fn subject(policy_sha256: &str, subject: &str, graph: &str) -> Result<String> {
    let bytes = serde_json::to_vec(&(
        "cohesix-mapped-delegation/v1",
        policy_sha256,
        subject,
        graph,
    ))
    .map_err(|_| IdentityError::Policy)?;
    // A 128-bit policy/subject binding fits the existing bounded ticket wire
    // contract. Policy validation also rejects duplicate normalized subjects.
    let digest = Sha256::digest(bytes);
    Ok(format!(
        "{SUBJECT_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(&digest[..16])
    ))
}

fn role(value: &str) -> Result<Role> {
    match value {
        "queen" => Ok(Role::Queen),
        "worker-heartbeat" => Ok(Role::WorkerHeartbeat),
        "worker-gpu" => Ok(Role::WorkerGpu),
        "worker-lora" => Ok(Role::WorkerLora),
        "worker-bus" => Ok(Role::WorkerBus),
        _ => Err(IdentityError::Policy),
    }
}

fn scopes(values: &[Scope]) -> Result<Vec<TicketScope>> {
    values
        .iter()
        .map(|scope| {
            let verb = match scope.verb.as_str() {
                "read" => TicketVerb::Read,
                "write" => TicketVerb::Write,
                "read_write" => TicketVerb::ReadWrite,
                _ => return Err(IdentityError::Policy),
            };
            Ok(TicketScope::new(&scope.path, verb, 0))
        })
        .collect()
}

impl MappedRequest {
    /// Exact mapped claims. JWT issuer time and expiry are retained, so a
    /// repeated exchange cannot manufacture a fresh quota identity or lifetime.
    /// These claims become authority only through the enrolled issuer and the
    /// gateway's generated action checks; serialization is still a proposal.
    pub fn ticket_claims(&self) -> Result<TicketClaims> {
        let ttl = self
            .expires_unix_s
            .checked_sub(self.issued_unix_s)
            .filter(|ttl| (1..=3600).contains(ttl))
            .ok_or(IdentityError::Stale)?;
        Ok(TicketClaims::new(
            role(&self.role)?,
            BudgetSpec::unbounded()
                .with_ops(Some(self.maximum_operations))
                .with_ttl(Some(ttl)),
            Some(subject(
                &self.mapping_sha256,
                &self.subject,
                &self.provider_graph_sha256,
            )?),
            MountSpec::empty(),
            self.issued_unix_s
                .checked_mul(1000)
                .ok_or(IdentityError::Stale)?,
        )
        .with_scopes(scopes(&self.scopes)?))
    }

    /// Issue only verified mapped claims using an independently enrolled host
    /// issuer. The receiver must enforce `Policy::mapped_rule` before reads or
    /// provider writes; the ticket does not carry an unchecked action allowlist.
    pub fn issue(&self, issuer: &TicketIssuer) -> Result<String> {
        let token = issuer
            .issue_compact(self.ticket_claims()?)
            .map_err(|_| IdentityError::Policy)?
            .encode()
            .map_err(|_| IdentityError::Policy)?;
        if token.len() > cohsh_core::MAX_TICKET_LEN {
            return Err(IdentityError::Limit);
        }
        Ok(token)
    }

    /// Hash used for audit correlation; never log the credential itself.
    pub fn credential_sha256(&self) -> &str {
        &self.credential_sha256
    }

    /// Original verified credential expiry; no gateway clock extension is permitted.
    pub fn expires_unix_s(&self) -> u64 {
        self.expires_unix_s
    }
}

impl Policy {
    /// Resolve a signed mapped ticket to the exact current policy rule and
    /// refuse an expanded role, path, operation budget or lifetime. The caller
    /// verifies the ticket MAC and freshness before invoking this method.
    pub fn mapped_rule<'a>(
        &'a self,
        claims: &TicketClaims,
        graph: &str,
    ) -> Result<Option<&'a Rule>> {
        let Some(claimed_subject) = claims.subject.as_deref() else {
            return Ok(None);
        };
        if !self.enabled || !claimed_subject.starts_with(SUBJECT_PREFIX) {
            return Ok(None);
        }
        let policy_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(self).map_err(|_| IdentityError::Policy)?,
        ));
        for rule in &self.rules {
            if claimed_subject != subject(&policy_hash, &rule.normalized_subject, graph)? {
                continue;
            }
            if claims.role != role(&rule.role)?
                || claims.scopes != scopes(&rule.scopes)?
                || !claims.mounts.is_empty()
                || claims
                    .budget
                    .ops()
                    .is_none_or(|ops| ops == 0 || ops > rule.maximum_operations)
                || claims
                    .budget
                    .ttl_s()
                    .is_none_or(|ttl| ttl == 0 || ttl > self.maximum_ttl_s)
            {
                return Err(IdentityError::Policy);
            }
            return Ok(Some(rule));
        }
        Ok(None)
    }
}
