// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce ticket scope, rate, and quota constraints for UI interactions.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

use cohesix_ticket::{TicketClaims, TicketQuotas, TicketVerb};

const RATE_WINDOW: Duration = Duration::from_secs(1);

/// Manifest-driven maxima for ticket scopes and quotas.
#[derive(Debug, Clone, Copy)]
pub struct TicketLimits {
    /// Maximum scope entries allowed in a ticket.
    pub max_scopes: u16,
    /// Maximum scope path length in bytes.
    pub max_scope_path_len: u16,
    /// Maximum allowed rate limit per second.
    pub max_scope_rate_per_s: u32,
    /// Default/max bandwidth quota in bytes (0 = unlimited).
    pub bandwidth_bytes: u64,
    /// Default/max cursor resume quota (0 = unlimited).
    pub cursor_resumes: u32,
    /// Default/max cursor advance quota (0 = unlimited).
    pub cursor_advances: u32,
}

impl Default for TicketLimits {
    fn default() -> Self {
        Self {
            max_scopes: 8,
            max_scope_path_len: 128,
            max_scope_rate_per_s: 64,
            bandwidth_bytes: 131_072,
            cursor_resumes: 16,
            cursor_advances: 256,
        }
    }
}

/// Validation error when a ticket exceeds manifest limits.
#[derive(Debug, Clone)]
pub enum TicketClaimError {
    /// Too many scopes were supplied.
    ScopeCount { count: usize, max: u16 },
    /// Scope path is invalid or too long.
    ScopePath { path: String, max_len: u16 },
    /// Scope rate exceeds the manifest maximum.
    ScopeRate { rate: u32, max: u32 },
    /// Bandwidth quota exceeds the manifest maximum.
    Bandwidth { value: u64, max: u64 },
    /// Cursor resume quota exceeds the manifest maximum.
    CursorResumes { value: u32, max: u32 },
    /// Cursor advance quota exceeds the manifest maximum.
    CursorAdvances { value: u32, max: u32 },
}

impl fmt::Display for TicketClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TicketClaimError::ScopeCount { count, max } => {
                write!(f, "scope count {count} exceeds {max}")
            }
            TicketClaimError::ScopePath { path, max_len } => {
                write!(f, "scope path '{path}' exceeds {max_len} bytes")
            }
            TicketClaimError::ScopeRate { rate, max } => {
                write!(f, "scope rate {rate} exceeds {max}")
            }
            TicketClaimError::Bandwidth { value, max } => {
                write!(f, "bandwidth quota {value} exceeds {max}")
            }
            TicketClaimError::CursorResumes { value, max } => {
                write!(f, "cursor resume quota {value} exceeds {max}")
            }
            TicketClaimError::CursorAdvances { value, max } => {
                write!(f, "cursor advance quota {value} exceeds {max}")
            }
        }
    }
}

/// Denial outcome for ticket enforcement.
#[derive(Debug, Clone, Copy)]
pub enum TicketDeny {
    /// Scope does not allow the requested operation.
    Scope,
    /// Rate limit exceeded.
    Rate { limit_per_s: u32 },
    /// Bandwidth quota exceeded.
    Bandwidth {
        limit_bytes: u64,
        remaining_bytes: u64,
        requested_bytes: u64,
    },
    /// Cursor resume quota exceeded.
    CursorResume { limit: u32 },
    /// Cursor advance quota exceeded.
    CursorAdvance { limit: u32 },
}

/// Cursor enforcement summary.
#[derive(Debug, Clone, Copy)]
pub struct CursorCheck {
    /// True when the read is a resume/rewind.
    pub is_resume: bool,
}

#[derive(Debug, Clone)]
struct TicketScopeState {
    path: Vec<String>,
    verb: TicketVerb,
    rate_limit: Option<u32>,
    window_start: Instant,
    window_count: u32,
}

impl TicketScopeState {
    fn allows_verb(&self, verb: TicketVerb) -> bool {
        match self.verb {
            TicketVerb::Read => matches!(verb, TicketVerb::Read),
            TicketVerb::Write => matches!(verb, TicketVerb::Write),
            TicketVerb::ReadWrite => true,
        }
    }

    fn matches_path(&self, path: &[String], allow_ancestor: bool) -> bool {
        if path.starts_with(self.path.as_slice()) {
            return true;
        }
        if allow_ancestor && self.path.starts_with(path) {
            return true;
        }
        false
    }

    fn check_rate(&mut self, now: Instant) -> Result<(), TicketDeny> {
        let Some(limit) = self.rate_limit else {
            return Ok(());
        };
        if now.duration_since(self.window_start) >= RATE_WINDOW {
            self.window_start = now;
            self.window_count = 0;
        }
        if self.window_count >= limit {
            return Err(TicketDeny::Rate { limit_per_s: limit });
        }
        self.window_count = self.window_count.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TicketQuotaState {
    bandwidth_limit: Option<u64>,
    bandwidth_remaining: Option<u64>,
    cursor_resume_limit: Option<u32>,
    cursor_resume_remaining: Option<u32>,
    cursor_advance_limit: Option<u32>,
    cursor_advance_remaining: Option<u32>,
}

impl TicketQuotaState {
    fn check_bandwidth(&self, requested: u64) -> Result<(), TicketDeny> {
        let Some(remaining) = self.bandwidth_remaining else {
            return Ok(());
        };
        if requested > remaining {
            let limit = self.bandwidth_limit.unwrap_or(remaining);
            return Err(TicketDeny::Bandwidth {
                limit_bytes: limit,
                remaining_bytes: remaining,
                requested_bytes: requested,
            });
        }
        Ok(())
    }

    fn consume_bandwidth(&mut self, consumed: u64) {
        if let Some(remaining) = &mut self.bandwidth_remaining {
            *remaining = remaining.saturating_sub(consumed);
        }
    }

    fn check_cursor(&self, is_resume: bool) -> Result<(), TicketDeny> {
        if let Some(remaining) = self.cursor_advance_remaining {
            if remaining == 0 {
                let limit = self.cursor_advance_limit.unwrap_or(0);
                return Err(TicketDeny::CursorAdvance { limit });
            }
        }
        if is_resume {
            if let Some(remaining) = self.cursor_resume_remaining {
                if remaining == 0 {
                    let limit = self.cursor_resume_limit.unwrap_or(0);
                    return Err(TicketDeny::CursorResume { limit });
                }
            }
        }
        Ok(())
    }

    fn consume_cursor(&mut self, is_resume: bool) {
        if let Some(remaining) = &mut self.cursor_advance_remaining {
            *remaining = remaining.saturating_sub(1);
        }
        if is_resume {
            if let Some(remaining) = &mut self.cursor_resume_remaining {
                *remaining = remaining.saturating_sub(1);
            }
        }
    }

    fn has_limits(&self) -> bool {
        self.bandwidth_limit.is_some()
            || self.cursor_resume_limit.is_some()
            || self.cursor_advance_limit.is_some()
    }
}

/// Mutable enforcement state for a ticket.
#[derive(Debug, Clone)]
pub struct TicketUsage {
    scopes: Vec<TicketScopeState>,
    quotas: TicketQuotaState,
    cursor_offsets: HashMap<String, u64>,
}

impl TicketUsage {
    /// Build ticket usage state from validated claims and manifest limits.
    pub fn from_claims(
        claims: &TicketClaims,
        limits: TicketLimits,
        now: Instant,
    ) -> Result<Self, TicketClaimError> {
        if claims.scopes.len() > limits.max_scopes as usize {
            return Err(TicketClaimError::ScopeCount {
                count: claims.scopes.len(),
                max: limits.max_scopes,
            });
        }
        let mut scopes = Vec::with_capacity(claims.scopes.len());
        for scope in &claims.scopes {
            let path = scope.path.trim().to_owned();
            if path.len() > limits.max_scope_path_len as usize
                || (!path.is_empty() && !path.starts_with('/'))
            {
                return Err(TicketClaimError::ScopePath {
                    path,
                    max_len: limits.max_scope_path_len,
                });
            }
            if limits.max_scope_rate_per_s > 0 && scope.rate_per_s > limits.max_scope_rate_per_s {
                return Err(TicketClaimError::ScopeRate {
                    rate: scope.rate_per_s,
                    max: limits.max_scope_rate_per_s,
                });
            }
            let components = split_scope_path(&path, limits.max_scope_path_len)?;
            let rate_limit = (scope.rate_per_s > 0).then_some(scope.rate_per_s);
            scopes.push(TicketScopeState {
                path: components,
                verb: scope.verb,
                rate_limit,
                window_start: now,
                window_count: 0,
            });
        }
        let quotas = resolve_quotas(claims.quotas, limits)?;
        Ok(Self {
            scopes,
            quotas,
            cursor_offsets: HashMap::new(),
        })
    }

    /// Return true when any enforcement data is present.
    pub fn has_enforcement(&self) -> bool {
        !self.scopes.is_empty() || self.quotas.has_limits()
    }

    /// Verify scope and rate limits for a path/verb pair.
    pub fn check_scope(
        &mut self,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
        now: Instant,
    ) -> Result<(), TicketDeny> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        let Some(idx) = self.best_scope_index(path, verb, allow_ancestor) else {
            return Err(TicketDeny::Scope);
        };
        self.scopes[idx].check_rate(now)
    }

    /// Verify scope membership without consuming rate quotas.
    pub fn check_scope_no_rate(
        &self,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
    ) -> Result<(), TicketDeny> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        if self.best_scope_index(path, verb, allow_ancestor).is_some() {
            Ok(())
        } else {
            Err(TicketDeny::Scope)
        }
    }

    /// Check bandwidth quotas for the requested byte count.
    pub fn check_bandwidth(&self, requested: u64) -> Result<(), TicketDeny> {
        self.quotas.check_bandwidth(requested)
    }

    /// Consume bandwidth quota after a successful operation.
    pub fn consume_bandwidth(&mut self, consumed: u64) {
        self.quotas.consume_bandwidth(consumed);
    }

    /// Check cursor quotas for a telemetry read.
    pub fn check_cursor(&self, path_key: &str, offset: u64) -> Result<CursorCheck, TicketDeny> {
        let last = self.cursor_offsets.get(path_key).copied();
        let is_resume = last.map_or(false, |last| offset < last);
        self.quotas.check_cursor(is_resume)?;
        Ok(CursorCheck { is_resume })
    }

    /// Record cursor progress after a successful telemetry read.
    pub fn record_cursor(&mut self, path_key: String, offset: u64, len: usize, check: CursorCheck) {
        let next = offset.saturating_add(len as u64);
        self.cursor_offsets.insert(path_key, next);
        self.quotas.consume_cursor(check.is_resume);
    }

    fn best_scope_index(
        &self,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
    ) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (idx, scope) in self.scopes.iter().enumerate() {
            if !scope.allows_verb(verb) {
                continue;
            }
            if !scope.matches_path(path, allow_ancestor) {
                continue;
            }
            let match_len = scope.path.len();
            if best.map_or(true, |(_, best_len)| match_len > best_len) {
                best = Some((idx, match_len));
            }
        }
        best.map(|(idx, _)| idx)
    }
}

fn split_scope_path(path: &str, max_len: u16) -> Result<Vec<String>, TicketClaimError> {
    if path.is_empty() || path == "/" {
        return Ok(Vec::new());
    }
    let components: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    if components.iter().any(|segment| segment == "..") {
        return Err(TicketClaimError::ScopePath {
            path: path.to_owned(),
            max_len,
        });
    }
    Ok(components)
}

fn resolve_quotas(quotas: TicketQuotas, limits: TicketLimits) -> Result<TicketQuotaState, TicketClaimError> {
    let bandwidth_limit = resolve_quota_u64(
        quotas.bandwidth_bytes,
        limits.bandwidth_bytes,
        |value, max| TicketClaimError::Bandwidth { value, max },
    )?;
    let cursor_resume_limit = resolve_quota_u32(
        quotas.cursor_resumes,
        limits.cursor_resumes,
        |value, max| TicketClaimError::CursorResumes { value, max },
    )?;
    let cursor_advance_limit = resolve_quota_u32(
        quotas.cursor_advances,
        limits.cursor_advances,
        |value, max| TicketClaimError::CursorAdvances { value, max },
    )?;
    Ok(TicketQuotaState {
        bandwidth_limit,
        bandwidth_remaining: bandwidth_limit,
        cursor_resume_limit,
        cursor_resume_remaining: cursor_resume_limit,
        cursor_advance_limit,
        cursor_advance_remaining: cursor_advance_limit,
    })
}

fn resolve_quota_u64<F>(
    value: Option<u64>,
    max: u64,
    err: F,
) -> Result<Option<u64>, TicketClaimError>
where
    F: FnOnce(u64, u64) -> TicketClaimError,
{
    match value {
        Some(value) => {
            if max > 0 && value > max {
                return Err(err(value, max));
            }
            Ok(Some(value))
        }
        None => Ok((max > 0).then_some(max)),
    }
}

fn resolve_quota_u32<F>(
    value: Option<u32>,
    max: u32,
    err: F,
) -> Result<Option<u32>, TicketClaimError>
where
    F: FnOnce(u32, u32) -> TicketClaimError,
{
    match value {
        Some(value) => {
            if max > 0 && value > max {
                return Err(err(value, max));
            }
            Ok(Some(value))
        }
        None => Ok((max > 0).then_some(max)),
    }
}
