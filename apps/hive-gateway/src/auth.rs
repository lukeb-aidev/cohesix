// Author: Lukas Bower
// Purpose: Verify delegated callers and retain finite, non-evictable live-ticket quotas beneath the gateway ceiling.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use cohesix_authority::policy::AuthorityPolicy;
use cohesix_ticket::{Role, TicketClaims, TicketKey, TicketToken, TicketVerb};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const TICKET_HEADER: &str = "x-cohesix-ticket";

/// Verified caller identity and the separate ticket fingerprint used by
/// evidence custody and quota accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatedPrincipal {
    pub ticket_hash: String,
    pub subject: String,
}

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
        read: bool,
    ) -> Result<(), &'static str> {
        if now >= self.expires
            || !(if read {
                read_role_allows(&self.claims, path)
            } else {
                role_allows(&self.claims, path)
            })
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
                (if read {
                    matches!(s.verb, TicketVerb::Read | TicketVerb::ReadWrite)
                } else {
                    matches!(s.verb, TicketVerb::Write | TicketVerb::ReadWrite)
                }) && within(path, &s.path)
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

fn read_role_allows(claims: &TicketClaims, path: &str) -> bool {
    // An explicitly issued root read scope is administrative authority. Empty
    // scopes occur only on the configured gateway ceiling, never a caller.
    if claims.role == Role::Queen
        && (claims.scopes.is_empty()
            || claims.scopes.iter().any(|scope| {
                scope.path == "/" && matches!(scope.verb, TicketVerb::Read | TicketVerb::ReadWrite)
            }))
    {
        return true;
    }
    let Ok((class, owner)) = cohesix_authority::provider::read_visibility(path) else {
        return false;
    };
    if class == "admin_only" {
        return claims.role == Role::Queen;
    }
    if class == "public" {
        return true;
    }
    if owner == "worker_subject" {
        let parts: Vec<_> = path.split('/').skip(1).collect();
        let worker = match parts.as_slice() {
            ["worker", worker, ..] => Some(*worker),
            ["shard", shard, "worker", worker, ..]
                if shard.len() == 2
                    && shard
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
            {
                Some(*worker)
            }
            _ => None,
        };
        return worker.is_some() && worker == claims.subject.as_deref();
    }
    claims.role == Role::Queen || (!claims.mounts.is_empty() && within(path, &claims.mounts.at))
}

/// Live entries are never evicted to admit another caller: doing so would
/// reset quotas. Expired tickets cannot re-enter the table.
pub struct Delegation {
    key: Option<TicketKey>,
    identity: Option<std::sync::Arc<crate::identity::Identity>>,
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
            identity: None,
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

    pub fn with_identity(mut self, identity: std::sync::Arc<crate::identity::Identity>) -> Self {
        self.identity = Some(identity);
        self
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

    #[cfg(test)]
    pub fn authorize(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        operations: usize,
        now: u64,
    ) -> Result<String, &'static str> {
        let result = self
            .admit(token, path, bytes, operations, now, Some(&[]))
            .map(|principal| principal.ticket_hash);
        if result.is_err() {
            self.refusals = self.refusals.saturating_add(1);
        }
        result
    }

    pub fn authorize_write(
        &mut self,
        token: Option<&str>,
        path: &str,
        lines: &[&str],
        now: u64,
    ) -> Result<String, &'static str> {
        self.authorize_write_principal(token, path, lines, now)
            .map(|principal| principal.ticket_hash)
    }

    /// Return the verified subject as well as the ticket hash for standing
    /// scope selection. Both values come from the same charged authorization.
    pub fn authorize_write_principal(
        &mut self,
        token: Option<&str>,
        path: &str,
        lines: &[&str],
        now: u64,
    ) -> Result<DelegatedPrincipal, &'static str> {
        let result = lines
            .iter()
            .try_fold(0usize, |total, line| total.checked_add(line.len()))
            .ok_or("ELIMIT delegated-request-bytes")
            .and_then(|bytes| self.admit(token, path, bytes, lines.len(), now, Some(lines)));
        if result.is_err() {
            self.refusals = self.refusals.saturating_add(1);
        }
        result
    }

    /// Read scopes share signature, expiry, quotas and ceiling state with writes.
    pub fn authorize_read(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        now: u64,
    ) -> Result<String, &'static str> {
        self.authorize_read_principal(token, path, bytes, now)
            .map(|principal| principal.ticket_hash)
    }

    /// Preserve the authenticated subject across status, cancel and recovery.
    pub fn authorize_read_principal(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        now: u64,
    ) -> Result<DelegatedPrincipal, &'static str> {
        let result = self.admit(token, path, bytes, 1, now, None);
        if result.is_err() {
            self.refusals = self.refusals.saturating_add(1);
        }
        result
    }

    /// Inspect current scope visibility after a charged authentication read.
    /// This never reserves budget; the later write validates the exact line.
    pub fn permits_path(&self, ticket_hash: &str, path: &str, write: bool, now: u64) -> bool {
        let Some(caller) = self.entries.get(ticket_hash) else {
            return false;
        };
        let permitted = |usage: &Usage| {
            now < usage.expires
                && (if write {
                    role_allows(&usage.claims, path)
                } else {
                    read_role_allows(&usage.claims, path)
                })
                && (usage.claims.mounts.is_empty() || within(path, &usage.claims.mounts.at))
                && (usage.unscoped_ceiling
                    || usage.claims.scopes.iter().any(|scope| {
                        (if write {
                            matches!(scope.verb, TicketVerb::Write | TicketVerb::ReadWrite)
                        } else {
                            matches!(scope.verb, TicketVerb::Read | TicketVerb::ReadWrite)
                        }) && within(path, &scope.path)
                    }))
        };
        permitted(caller)
            && self
                .ceiling
                .as_ref()
                .map_or(self.ceiling_role == Role::Queen, permitted)
    }

    fn admit(
        &mut self,
        token: Option<&str>,
        path: &str,
        bytes: usize,
        operations: usize,
        now: u64,
        write_lines: Option<&[&str]>,
    ) -> Result<DelegatedPrincipal, &'static str> {
        let read = write_lines.is_none();
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
        // Unverified claims select a verifier only. Neither signature domain
        // falls back to the other after a refusal.
        let claims =
            TicketToken::decode_unverified(token).map_err(|_| "EPERM delegated-ticket-invalid")?;
        let (verified, claims) = if claims.subject.as_deref().is_some_and(|subject| {
            subject.starts_with(cohesix_identity::delegation::SUBJECT_PREFIX)
        }) {
            self.identity
                .as_ref()
                .ok_or("EPERM identity mapping unavailable")?
                .verify(token, path, write_lines, read)?
        } else {
            let key = self
                .key
                .as_ref()
                .ok_or("EPERM delegation-key-unavailable")?;
            let verified =
                TicketToken::decode(token, key).map_err(|_| "EPERM delegated-ticket-invalid")?;
            let claims = verified.claims().clone();
            (verified, claims)
        };
        let canonical = verified
            .encode()
            .map_err(|_| "EPERM delegated-ticket-invalid")?;
        let identity = hex::encode(Sha256::digest(canonical.as_bytes()));
        self.entries.retain(|_, usage| usage.expires > now);
        if !self.entries.contains_key(&identity) {
            self.misses = self.misses.saturating_add(1);
            let usage = Usage::new(claims, now, self.policy)?;
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
        caller.charge(path, bytes, operations, now, read)?;
        if let Some(ceiling) = &mut self.ceiling {
            ceiling.charge(path, bytes, operations, now, read)?;
        } else if self.ceiling_role != Role::Queen {
            return Err("EPERM gateway-ceiling");
        }
        let subject = caller
            .claims
            .subject
            .as_ref()
            .ok_or("EPERM delegated-ticket-subject")?
            .clone();
        self.entries.insert(identity.clone(), caller);
        Ok(DelegatedPrincipal {
            ticket_hash: identity,
            subject,
        })
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
    fn standing_principal_keeps_subject_distinct_from_ticket_fingerprint() {
        let mut writer = delegation();
        let ticket = token(claims());
        let first = writer
            .authorize_write_principal(Some(&ticket), "/queen/intents/ctl", &["{}"], 1000)
            .expect("verified write");
        assert_eq!(first.subject, "operator");
        assert_eq!(first.ticket_hash.len(), 64);
        assert_ne!(first.subject, first.ticket_hash);
        let again = writer
            .authorize_write_principal(Some(&ticket), "/queen/intents/ctl", &["{}"], 1000)
            .expect("same ticket and subject");
        assert_eq!(again, first);

        let mut reads = delegation();
        let read_ticket =
            token(claims().with_scopes(vec![TicketScope::new("/", TicketVerb::Read, 2)]));
        let status = reads
            .authorize_read_principal(Some(&read_ticket), "/host/tickets/status", 20, 1000)
            .expect("verified status read");
        assert_eq!(status.subject, "operator");
        assert_ne!(status.ticket_hash, first.ticket_hash);
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
    #[test]
    fn delegated_reads_reject_other_subjects_global_state_write_only_and_expiry() {
        let mut caller = claims();
        caller.subject = Some("alice".into());
        caller.scopes = vec![TicketScope::new("/worker", TicketVerb::Read, 0)];
        let ticket = token(caller.clone());
        let mut gateway = delegation();
        assert!(gateway
            .authorize_read(Some(&ticket), "/worker/alice/telemetry", 20, 1000)
            .is_ok());
        for path in [
            "/worker/bob/telemetry",
            "/host/tickets/status",
            "/audit",
            "/replay/status",
            "/proc/identity",
        ] {
            assert!(
                gateway
                    .authorize_read(Some(&ticket), path, 20, 1000)
                    .is_err(),
                "{path}"
            );
        }
        caller.scopes[0].verb = TicketVerb::Write;
        assert!(delegation()
            .authorize_read(Some(&token(caller)), "/worker/alice/telemetry", 20, 1000)
            .is_err());
        assert!(gateway
            .authorize_read(Some(&ticket), "/worker/alice/telemetry", 20, 11_000)
            .is_err());
        assert!(gateway
            .authorize_read(None, "/worker/alice/telemetry", 20, 11_000)
            .is_err());
    }
}
