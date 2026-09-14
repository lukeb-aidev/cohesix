// Author: Lukas Bower
// Purpose: Verify delegated callers and retain finite, non-evictable live-ticket quotas beneath the gateway ceiling.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use cohesix_authority::policy::AuthorityPolicy;
use cohesix_ticket::{Role, TicketClaims, TicketKey, TicketToken, TicketVerb};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const TICKET_HEADER: &str = "x-cohesix-ticket";

#[derive(Debug, Clone)]
struct Usage {
    claims: TicketClaims,
    expires: u64,
    operations: u64,
    bytes: u64,
    rates: Vec<(u64, u32)>,
    unscoped_ceiling: bool,
}

impl Usage {
    fn new(claims: TicketClaims, now: u64, policy: AuthorityPolicy) -> Result<Self, &'static str> {
        let ttl = claims
            .budget
            .ttl_s()
            .ok_or("EPERM delegated-ticket-ttl-required")?;
        if ttl == 0 || ttl > policy.delegated_ticket_max_ttl_s as u64 || claims.issued_at_ms > now {
            return Err("EPERM delegated-ticket-time");
        }
        let expires = claims
            .issued_at_ms
            .checked_add(ttl.checked_mul(1000).ok_or("EPERM delegated-ticket-time")?)
            .ok_or("EPERM delegated-ticket-time")?;
        if now >= expires
            || claims
                .subject
                .as_deref()
                .is_none_or(|s| cohesix_authority::validate_id(s).is_err())
            || claims.scopes.is_empty()
            || (!claims.mounts.is_empty()
                && (cohesix_authority::validate_id(&claims.mounts.service).is_err()
                    || !canonical_path(&claims.mounts.at)))
            || claims.scopes.iter().any(|s| !canonical_path(&s.path))
        {
            return Err("EPERM delegated-ticket-claims");
        }
        Ok(Self {
            rates: vec![(0, 0); claims.scopes.len()],
            claims,
            expires,
            operations: 0,
            bytes: 0,
            unscoped_ceiling: false,
        })
    }

    fn ceiling(claims: TicketClaims, now: u64) -> Result<Self, &'static str> {
        if claims.issued_at_ms > now
            || (!claims.mounts.is_empty()
                && (cohesix_authority::validate_id(&claims.mounts.service).is_err()
                    || !canonical_path(&claims.mounts.at)))
            || claims
                .scopes
                .iter()
                .any(|scope| !canonical_path(&scope.path))
        {
            return Err("EPERM gateway-ceiling-claims");
        }
        let expires = match claims.budget.ttl_s() {
            Some(ttl) => claims
                .issued_at_ms
                .checked_add(ttl.checked_mul(1000).ok_or("EPERM gateway-ceiling-time")?)
                .ok_or("EPERM gateway-ceiling-time")?,
            None => u64::MAX,
        };
        if now >= expires {
            return Err("EPERM gateway-ceiling-expired");
        }
        Ok(Self {
            unscoped_ceiling: claims.scopes.is_empty(),
            rates: vec![(0, 0); claims.scopes.len()],
            claims,
            expires,
            operations: 0,
            bytes: 0,
        })
    }

    fn charge(
        &mut self,
        path: &str,
        bytes: usize,
        operations: usize,
        now: u64,
    ) -> Result<(), &'static str> {
        if now >= self.expires
            || !role_allows(&self.claims, path)
            || (!self.claims.mounts.is_empty() && !within(path, &self.claims.mounts.at))
        {
            return Err("EPERM delegated-ticket-scope");
        }
        let index = self
            .claims
            .scopes
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                matches!(s.verb, TicketVerb::Write | TicketVerb::ReadWrite) && within(path, &s.path)
            })
            .max_by_key(|(_, s)| s.path.len())
            .map(|(i, _)| i);
        if index.is_none() && !self.unscoped_ceiling {
            return Err("EPERM delegated-ticket-scope");
        }
        let total_ops = self
            .operations
            .checked_add(operations as u64)
            .ok_or("ELIMIT delegated-ticket-operations")?;
        let total_bytes = self
            .bytes
            .checked_add(bytes as u64)
            .ok_or("ELIMIT delegated-ticket-bytes")?;
        if self
            .claims
            .budget
            .ops()
            .is_some_and(|limit| total_ops > limit)
            || self
                .claims
                .budget
                .ticks()
                .is_some_and(|limit| total_ops > limit)
            || self
                .claims
                .quotas
                .bandwidth_bytes
                .is_some_and(|limit| total_bytes > limit)
        {
            return Err("ELIMIT delegated-ticket-quota");
        }
        if let Some(index) = index {
            let (window, used) = self.rates[index];
            let used = if window == now / 1000 { used } else { 0 };
            let count = used
                .checked_add(u32::try_from(operations).map_err(|_| "ELIMIT delegated-ticket-rate")?)
                .ok_or("ELIMIT delegated-ticket-rate")?;
            let limit = self.claims.scopes[index].rate_per_s;
            if limit != 0 && count > limit {
                return Err("ELIMIT delegated-ticket-rate");
            }
            self.rates[index] = (now / 1000, count);
        }
        self.operations = total_ops;
        self.bytes = total_bytes;
        Ok(())
    }
}

fn canonical_path(path: &str) -> bool {
    path == "/"
        || (path.starts_with('/')
            && path.len() <= 255
            && path.split('/').skip(1).all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && !part.bytes().any(|b| b.is_ascii_control())
            }))
}

fn within(path: &str, scope: &str) -> bool {
    scope == "/"
        || path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn role_allows(claims: &TicketClaims, path: &str) -> bool {
    if claims.role == Role::Queen {
        return true;
    }
    if within(path, "/host") || within(path, "/queen") || within(path, "/proc") {
        return false;
    }
    let Some(subject) = claims.subject.as_deref() else {
        return false;
    };
    let parts: Vec<_> = path.split('/').skip(1).collect();
    let worker_path = match parts.as_slice() {
        ["worker", worker, "telemetry"] => *worker == subject,
        ["shard", shard, "worker", worker, "telemetry"] => {
            *worker == subject
                && shard.len() == 2
                && shard
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }
        _ => false,
    };
    match claims.role {
        Role::WorkerHeartbeat | Role::WorkerLora => worker_path,
        Role::WorkerGpu => {
            worker_path
                || (within(path, "/gpu")
                    && !claims.mounts.at.is_empty()
                    && within(path, &claims.mounts.at))
        }
        Role::WorkerBus => {
            within(path, "/bus") && !claims.mounts.at.is_empty() && within(path, &claims.mounts.at)
        }
        Role::Queen => true,
    }
}

/// Live entries are never evicted to admit another caller: doing so would
/// reset quotas. Expired tickets cannot re-enter the table.
pub struct Delegation {
    key: Option<TicketKey>,
    policy: AuthorityPolicy,
    ceiling_role: Role,
    ceiling: Option<Usage>,
    entries: BTreeMap<String, Usage>,
    last_now: u64,
    pub hits: u64,
    pub misses: u64,
    pub refusals: u64,
    audit_records: u64,
    audit_emit_ns: u64,
}

impl Delegation {
    pub fn new(
        key: Option<TicketKey>,
        policy: AuthorityPolicy,
        ceiling_role: Role,
        ceiling: Option<TicketClaims>,
        now: u64,
    ) -> Result<Self, &'static str> {
        if ceiling.as_ref().is_some_and(|c| c.role != ceiling_role) {
            return Err("EPERM gateway-ceiling-role");
        }
        let ceiling = ceiling
            .map(|claims| Usage::ceiling(claims, now))
            .transpose()?;
        if ceiling_role != Role::Queen && ceiling.is_none() {
            return Err("EPERM gateway-ceiling-ticket-required");
        }
        Ok(Self {
            key,
            policy,
            ceiling_role,
            ceiling,
            entries: BTreeMap::new(),
            last_now: now,
            hits: 0,
            misses: 0,
            refusals: 0,
            audit_records: 0,
            audit_emit_ns: 0,
        })
    }

    pub fn snapshot(&self) -> cohesix_authority::policy::DelegationStatus {
        cohesix_authority::policy::DelegationStatus {
            identity_class: "gateway_enforced".into(),
            writer_epoch: self.policy.writer_epoch,
            cache_entries: self.entries.len(),
            cache_capacity: self.policy.delegated_ticket_entries as usize,
            cache_hits: self.hits,
            cache_misses: self.misses,
            refusals: self.refusals,
            audit_records: self.audit_records,
            audit_emit_ns: self.audit_emit_ns,
        }
    }

    pub fn record_audit(&mut self, elapsed_ns: u64) {
        self.audit_records = self.audit_records.saturating_add(1);
        self.audit_emit_ns = self.audit_emit_ns.saturating_add(elapsed_ns);
    }

    pub fn authorize(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        operations: usize,
        now: u64,
    ) -> Result<String, &'static str> {
        let result = self.admit(token, path, bytes, operations, now);
        if result.is_err() {
            self.refusals = self.refusals.saturating_add(1);
        }
        result
    }

    fn admit(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        operations: usize,
        now: u64,
    ) -> Result<String, &'static str> {
        if !canonical_path(path) || operations == 0 {
            return Err("EPERM delegated-request-shape");
        }
        if now < self.last_now {
            return Err("EPERM authority-clock-regressed");
        }
        self.last_now = now;
        let token = token.ok_or("EPERM delegated-ticket-required")?;
        if token.len() > cohsh_core::MAX_TICKET_LEN {
            return Err("ELIMIT delegated-ticket-length");
        }
        let key = self
            .key
            .as_ref()
            .ok_or("EPERM delegation-key-unavailable")?;
        let verified =
            TicketToken::decode(token, key).map_err(|_| "EPERM delegated-ticket-invalid")?;
        let canonical = verified
            .encode()
            .map_err(|_| "EPERM delegated-ticket-invalid")?;
        let identity = hex::encode(Sha256::digest(canonical.as_bytes()));
        self.entries.retain(|_, usage| usage.expires > now);
        if !self.entries.contains_key(&identity) {
            self.misses = self.misses.saturating_add(1);
            let usage = Usage::new(verified.claims().clone(), now, self.policy)?;
            if self.entries.len() >= self.policy.delegated_ticket_entries as usize {
                return Err("ELIMIT delegated-ticket-capacity");
            }
            self.entries.insert(identity.clone(), usage);
        } else {
            self.hits = self.hits.saturating_add(1);
        }
        // Clone the bounded usage records so a refused ceiling cannot partially
        // commit a caller charge. The lock covers the entire reservation.
        let mut caller = self
            .entries
            .get(&identity)
            .cloned()
            .ok_or("EPERM delegated-ticket-state")?;
        caller.charge(path, bytes, operations, now)?;
        if let Some(ceiling) = &mut self.ceiling {
            ceiling.charge(path, bytes, operations, now)?;
        } else if self.ceiling_role != Role::Queen {
            return Err("EPERM gateway-ceiling");
        }
        self.entries.insert(identity.clone(), caller);
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketIssuer, TicketScope};
    fn claims() -> TicketClaims {
        TicketClaims::new(
            Role::Queen,
            BudgetSpec::unbounded().with_ops(Some(2)).with_ttl(Some(10)),
            Some("operator".into()),
            MountSpec::empty(),
            1000,
        )
        .with_scopes(vec![TicketScope::new(
            "/queen/intents",
            TicketVerb::Write,
            2,
        )])
    }
    fn delegation() -> Delegation {
        Delegation::new(
            Some(TicketKey::from_secret("test-issuer-key")),
            AuthorityPolicy::default(),
            Role::Queen,
            None,
            1000,
        )
        .expect("fixture")
    }
    fn token(claims: TicketClaims) -> String {
        TicketIssuer::new("test-issuer-key")
            .issue(claims)
            .expect("issue")
            .encode()
            .expect("encode")
    }

    #[test]
    fn mount_ceiling_and_rotated_issuer_remain_independent_of_role() {
        let mut mounted = claims();
        mounted.mounts = MountSpec {
            service: "queen-control".into(),
            at: "/queen/intents/selected".into(),
        };
        let mut gateway = delegation();
        assert!(gateway
            .authorize(Some(&token(mounted)), "/queen/intents/ctl", 1, 1, 1000)
            .is_err());
        let mut rotated = Delegation::new(
            Some(TicketKey::from_secret("replacement-issuer-key")),
            AuthorityPolicy::default(),
            Role::Queen,
            None,
            1000,
        )
        .expect("rotated issuer");
        assert!(rotated
            .authorize(Some(&token(claims())), "/queen/intents/ctl", 1, 1, 1000)
            .is_err());
        let renewed = TicketIssuer::new("replacement-issuer-key")
            .issue(claims())
            .expect("issue")
            .encode()
            .expect("encode");
        assert!(rotated
            .authorize(Some(&renewed), "/queen/intents/ctl", 1, 1, 1000)
            .is_ok());
    }
    #[test]
    fn signatures_scopes_expiry_and_aggregate_quota_are_enforced() {
        let mut auth = delegation();
        let ticket = token(claims());
        assert_eq!(
            auth.authorize(None, "/queen/intents/ctl", 1, 1, 1000),
            Err("EPERM delegated-ticket-required")
        );
        let forged = TicketIssuer::new("wrong-key")
            .issue(claims())
            .expect("issue")
            .encode()
            .expect("encode");
        assert_eq!(
            auth.authorize(Some(&forged), "/queen/intents/ctl", 1, 1, 1000),
            Err("EPERM delegated-ticket-invalid")
        );
        assert_eq!(
            auth.authorize(Some(&ticket), "/queen/intents-other", 1, 1, 1000),
            Err("EPERM delegated-ticket-scope")
        );
        auth.authorize(Some(&ticket), "/queen/intents/ctl", 1, 2, 1000)
            .expect("two operations");
        assert_eq!(
            auth.authorize(Some(&ticket), "/queen/intents/ctl", 1, 1, 2000),
            Err("ELIMIT delegated-ticket-quota")
        );
        assert_eq!(
            auth.authorize(Some(&ticket), "/queen/intents/ctl", 1, 1, 11000),
            Err("EPERM delegated-ticket-claims")
        );
    }
    #[test]
    fn caller_cannot_exceed_ceiling_or_evict_live_quota_state() {
        let policy = AuthorityPolicy {
            delegated_ticket_entries: 1,
            ..AuthorityPolicy::default()
        };
        let mut auth = Delegation::new(
            Some(TicketKey::from_secret("test-issuer-key")),
            policy,
            Role::Queen,
            Some(claims()),
            1000,
        )
        .expect("ceiling");
        let mut broader = claims();
        broader.scopes[0].path = "/queen".into();
        let ticket = token(broader.clone());
        assert_eq!(
            auth.authorize(Some(&ticket), "/queen/ctl", 1, 1, 1000),
            Err("EPERM delegated-ticket-scope")
        );
        broader.subject = Some("another".into());
        assert_eq!(
            auth.authorize(Some(&token(broader)), "/queen/intents/ctl", 1, 1, 1000),
            Err("ELIMIT delegated-ticket-capacity")
        );
        auth.authorize(Some(&ticket), "/queen/intents/ctl", 1, 2, 1000)
            .expect("uncharged after refusal");
    }
    #[test]
    fn two_callers_share_the_upstream_ceiling_without_resetting_personal_quota() {
        let mut ceiling = claims();
        ceiling.budget = BudgetSpec::unbounded().with_ops(Some(3));
        ceiling.scopes[0].rate_per_s = 0;
        let mut auth = Delegation::new(
            Some(TicketKey::from_secret("test-issuer-key")),
            AuthorityPolicy::default(),
            Role::Queen,
            Some(ceiling),
            1000,
        )
        .expect("ceiling");
        let alice = token(claims());
        let mut bob_claims = claims();
        bob_claims.subject = Some("second".into());
        let bob = token(bob_claims);
        auth.authorize(Some(&alice), "/queen/intents/ctl", 1, 2, 1000)
            .expect("alice quota");
        auth.authorize(Some(&bob), "/queen/intents/ctl", 1, 1, 1000)
            .expect("bob quota");
        assert_eq!(
            auth.authorize(Some(&alice), "/queen/intents/ctl", 1, 1, 2000),
            Err("ELIMIT delegated-ticket-quota")
        );
        assert_eq!(
            auth.authorize(Some(&bob), "/queen/intents/ctl", 1, 1, 2000),
            Err("ELIMIT delegated-ticket-quota")
        );
        assert_eq!(auth.snapshot().cache_entries, 2);
    }

    #[test]
    fn worker_role_cannot_use_a_broad_scope_to_write_another_identity() {
        let mut worker = claims();
        worker.role = Role::WorkerHeartbeat;
        worker.subject = Some("worker-1".into());
        worker.scopes[0].path = "/".into();
        for path in [
            "/worker/worker-1/telemetry",
            "/shard/7f/worker/worker-1/telemetry",
        ] {
            assert!(role_allows(&worker, path));
        }
        for path in [
            "/worker/worker-2/telemetry",
            "/shard/7f/worker/worker-2/telemetry",
            "/worker/x/worker-1/telemetry",
            "/worker/worker-1/job",
            "/queen/ctl",
            "/host/tickets/spec",
        ] {
            assert!(!role_allows(&worker, path));
        }
        worker.role = Role::WorkerBus;
        worker.mounts.at = "/".into();
        assert!(!role_allows(&worker, "/log/queen.log"));
    }
}
