// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Maintain PolicyFS state, approvals, and gate decisions for NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use secure9p_codec::ErrorCode;
use secure9p_core::append_only_write_bounds;

use super::cbor::{CborError, CborWriter};
use super::ui::UI_MAX_STREAM_BYTES;
use crate::NineDoorError;

const MAX_POLICY_PATH_COMPONENTS: usize = 8;
const MAX_ACTION_ID_LEN: usize = 64;

/// Manifest-derived policy configuration.
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    enabled: bool,
    rules: Vec<PolicyRuleSpec>,
    limits: PolicyLimits,
}

impl PolicyConfig {
    /// Construct a disabled policy configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            rules: Vec::new(),
            limits: PolicyLimits::default(),
        }
    }

    /// Construct an enabled policy configuration with explicit rules and limits.
    pub fn enabled(rules: Vec<PolicyRuleSpec>, limits: PolicyLimits) -> Self {
        Self {
            enabled: true,
            rules,
            limits,
        }
    }

    pub(crate) fn enabled_flag(&self) -> bool {
        self.enabled
    }

    pub(crate) fn rules(&self) -> &[PolicyRuleSpec] {
        &self.rules
    }

    pub(crate) fn limits(&self) -> PolicyLimits {
        self.limits
    }
}

/// Policy queue sizing limits.
#[derive(Debug, Clone, Copy)]
pub struct PolicyLimits {
    /// Maximum number of action entries accepted in the queue.
    pub queue_max_entries: usize,
    /// Maximum byte size for the action queue file.
    pub queue_max_bytes: usize,
    /// Maximum byte size for the policy control log.
    pub ctl_max_bytes: usize,
    /// Maximum byte size for a single action status payload.
    pub status_max_bytes: usize,
}

impl Default for PolicyLimits {
    fn default() -> Self {
        Self {
            queue_max_entries: 32,
            queue_max_bytes: 4096,
            ctl_max_bytes: 2048,
            status_max_bytes: 512,
        }
    }
}

/// Manifest rule describing a gated path.
#[derive(Debug, Clone)]
pub struct PolicyRuleSpec {
    /// Stable rule identifier.
    pub id: String,
    /// Target path pattern (absolute).
    pub target: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PolicyStore {
    config: PolicyConfig,
    rules: Vec<PolicyRule>,
    rules_snapshot: Vec<u8>,
    ctl_log: Vec<u8>,
    queue_log: Vec<u8>,
    actions: Vec<PolicyAction>,
}

/// Preflight payloads for policy UI providers.
#[derive(Debug, Clone)]
pub struct PolicyPreflightPayloads {
    /// Text payload.
    pub text: Vec<u8>,
    /// CBOR payload.
    pub cbor: Vec<u8>,
}

impl PolicyStore {
    pub fn new(config: PolicyConfig) -> Result<Self, NineDoorError> {
        let rules = config
            .rules()
            .iter()
            .map(|rule| PolicyRule::parse(rule))
            .collect::<Result<Vec<_>, _>>()?;
        let rules_snapshot = render_rules_snapshot(config.enabled_flag(), &rules, config.limits())?;
        Ok(Self {
            config,
            rules,
            rules_snapshot,
            ctl_log: Vec::new(),
            queue_log: Vec::new(),
            actions: Vec::new(),
        })
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled_flag()
    }

    pub fn limits(&self) -> PolicyLimits {
        self.config.limits()
    }

    pub fn rules_snapshot(&self) -> &[u8] {
        &self.rules_snapshot
    }

    pub fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    pub fn queue_log(&self) -> &[u8] {
        &self.queue_log
    }

    pub fn action_status_payload(&self, id: &str) -> Option<Vec<u8>> {
        let action = self.actions.iter().find(|action| action.id == id)?;
        Some(action.status_payload(self.limits()))
    }

    pub fn preflight_req_payloads(&self) -> Result<PolicyPreflightPayloads, NineDoorError> {
        let mut total = 0usize;
        let mut queued = 0usize;
        let mut consumed = 0usize;
        for action in &self.actions {
            total = total.saturating_add(1);
            if action.consumed {
                consumed = consumed.saturating_add(1);
            } else {
                queued = queued.saturating_add(1);
            }
        }
        let mut text = String::new();
        let _ = writeln!(text, "req total={} queued={} consumed={}", total, queued, consumed);
        for action in &self.actions {
            let state = if action.consumed { "consumed" } else { "queued" };
            let decision = policy_decision_label(action.decision);
            let _ = writeln!(
                text,
                "req id={} target={} decision={} state={}",
                action.id, action.target, decision, state
            );
        }
        ensure_stream_len("policy/preflight/req", text.len())?;
        let cbor = build_preflight_req_cbor(total, queued, consumed, &self.actions)?;
        Ok(PolicyPreflightPayloads {
            text: text.into_bytes(),
            cbor,
        })
    }

    pub fn preflight_diff_payloads(&self) -> Result<PolicyPreflightPayloads, NineDoorError> {
        let mut unmatched = 0usize;
        for action in &self.actions {
            if !self.rules.iter().any(|rule| rule.matches_path(&action.path)) {
                unmatched = unmatched.saturating_add(1);
            }
        }
        let mut text = String::new();
        let _ = writeln!(
            text,
            "diff rules={} actions={} unmatched={}",
            self.rules.len(),
            self.actions.len(),
            unmatched
        );
        let mut rule_counts = Vec::with_capacity(self.rules.len());
        for rule in &self.rules {
            let mut queued = 0usize;
            let mut consumed = 0usize;
            for action in &self.actions {
                if rule.matches_path(&action.path) {
                    if action.consumed {
                        consumed = consumed.saturating_add(1);
                    } else {
                        queued = queued.saturating_add(1);
                    }
                }
            }
            rule_counts.push((queued, consumed));
            let _ = writeln!(
                text,
                "rule id={} target={} queued={} consumed={}",
                rule.id, rule.target, queued, consumed
            );
        }
        ensure_stream_len("policy/preflight/diff", text.len())?;
        let cbor = build_preflight_diff_cbor(self.rules.len(), self.actions.len(), unmatched, &self.rules, &rule_counts)?;
        Ok(PolicyPreflightPayloads {
            text: text.into_bytes(),
            cbor,
        })
    }

    pub fn append_policy_ctl(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<PolicyAppendOutcome, NineDoorError> {
        ensure_utf8(data, "policy control payload")?;
        validate_json_lines(data, "policy control")?;
        let max_len = self.config.limits().ctl_max_bytes;
        let current_len = self.ctl_log.len();
        let appended = apply_append(
            current_len,
            offset,
            max_len,
            data.len(),
            "policy control",
        )?;
        if appended.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!("policy control exceeds max bytes {}", max_len),
            ));
        }
        self.ctl_log.extend_from_slice(&data[..appended.len]);
        Ok(PolicyAppendOutcome {
            count: appended.len as u32,
        })
    }

    pub fn append_action_queue(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<PolicyQueueOutcome, NineDoorError> {
        ensure_utf8(data, "action queue payload")?;
        let parsed = parse_action_lines(data)?;
        let limits = self.config.limits();
        if self.actions.len() + parsed.len() > limits.queue_max_entries {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "action queue exceeds max entries {}",
                    limits.queue_max_entries
                ),
            ));
        }
        let bounds = apply_append(
            self.queue_log.len(),
            offset,
            limits.queue_max_bytes,
            data.len(),
            "action queue",
        )?;
        if bounds.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!("action queue exceeds max bytes {}", limits.queue_max_bytes),
            ));
        }
        let mut appended = Vec::new();
        for entry in parsed {
            if self.actions.iter().any(|action| action.id == entry.id) {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("action id '{}' already exists", entry.id),
                ));
            }
            appended.push(PolicyActionAudit {
                id: entry.id.clone(),
                target: entry.target.clone(),
                decision: entry.decision,
            });
            self.actions.push(PolicyAction::from_request(entry));
        }
        self.queue_log.extend_from_slice(&data[..bounds.len]);
        Ok(PolicyQueueOutcome {
            count: bounds.len as u32,
            appended,
        })
    }

    pub fn consume_gate(&mut self, path: &[String]) -> PolicyGateDecision {
        if !self.config.enabled_flag() {
            return PolicyGateDecision::Allowed(PolicyGateAllowance::Ungated);
        }
        if !self
            .rules
            .iter()
            .any(|rule| rule.matches_path(path))
        {
            return PolicyGateDecision::Allowed(PolicyGateAllowance::NotRequired);
        }
        if let Some(action) = self
            .actions
            .iter_mut()
            .find(|action| action.matches_path(path) && !action.consumed)
        {
            action.consumed = true;
            let decision = match action.decision {
                PolicyDecision::Approve => PolicyGateDecision::Allowed(PolicyGateAllowance::Action {
                    id: action.id.clone(),
                    target: action.target.clone(),
                }),
                PolicyDecision::Deny => PolicyGateDecision::Denied(PolicyGateDenial::Action {
                    id: action.id.clone(),
                    target: action.target.clone(),
                }),
            };
            return decision;
        }
        PolicyGateDecision::Denied(PolicyGateDenial::Missing)
    }
}

/// Outcome of appending to a policy control file.
#[derive(Debug, Clone)]
pub struct PolicyAppendOutcome {
    /// Count of bytes accepted.
    pub count: u32,
}

/// Outcome of appending to the action queue.
#[derive(Debug, Clone)]
pub struct PolicyQueueOutcome {
    /// Count of bytes accepted.
    pub count: u32,
    /// Actions appended in this write.
    pub appended: Vec<PolicyActionAudit>,
}

/// Audit snapshot for appended actions.
#[derive(Debug, Clone)]
pub struct PolicyActionAudit {
    /// Action identifier.
    pub id: String,
    /// Target path.
    pub target: String,
    /// Decision recorded.
    pub decision: PolicyDecision,
}

/// Decision result when evaluating a policy gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyGateDecision {
    /// Gate allowed the action.
    Allowed(PolicyGateAllowance),
    /// Gate denied the action.
    Denied(PolicyGateDenial),
}

/// Allowance detail for a gate decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyGateAllowance {
    /// Policy gating disabled.
    Ungated,
    /// Policy gating not required for this path.
    NotRequired,
    /// Approved via an action entry.
    Action { id: String, target: String },
}

/// Denial detail for a gate decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyGateDenial {
    /// Missing approval entry.
    Missing,
    /// Explicit denial entry consumed.
    Action { id: String, target: String },
}

#[derive(Debug, Clone)]
struct PolicyRule {
    id: String,
    target: String,
    pattern: Vec<PolicyPathSegment>,
}

impl PolicyRule {
    fn parse(spec: &PolicyRuleSpec) -> Result<Self, NineDoorError> {
        let target = spec.target.trim();
        let pattern = parse_path_pattern(target)?;
        Ok(Self {
            id: spec.id.clone(),
            target: target.to_owned(),
            pattern,
        })
    }

    fn matches_path(&self, path: &[String]) -> bool {
        if path.len() != self.pattern.len() {
            return false;
        }
        for (segment, pattern) in path.iter().zip(self.pattern.iter()) {
            match pattern {
                PolicyPathSegment::Wildcard => continue,
                PolicyPathSegment::Literal(value) if value == segment => continue,
                _ => return false,
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
enum PolicyPathSegment {
    Literal(String),
    Wildcard,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRequest {
    id: String,
    target: String,
    decision: PolicyDecision,
}

#[derive(Debug, Clone)]
struct PolicyAction {
    id: String,
    target: String,
    decision: PolicyDecision,
    path: Vec<String>,
    consumed: bool,
}

impl PolicyAction {
    fn from_request(request: ActionRequest) -> Self {
        let path = request
            .target
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Self {
            id: request.id,
            target: request.target,
            decision: request.decision,
            path,
            consumed: false,
        }
    }

    fn matches_path(&self, path: &[String]) -> bool {
        self.path == path
    }

    fn status_payload(&self, limits: PolicyLimits) -> Vec<u8> {
        let status = PolicyStatusSnapshot {
            id: &self.id,
            target: &self.target,
            decision: self.decision,
            state: if self.consumed {
                "consumed"
            } else {
                "queued"
            },
        };
        let json = serde_json::to_vec(&status).unwrap_or_default();
        if json.len() > limits.status_max_bytes {
            return format!(
                "{{\"id\":\"{}\",\"state\":\"oversize\"}}",
                self.id
            )
            .into_bytes();
        }
        json
    }
}

/// Policy decision captured in an action entry.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDecision {
    /// Approve a gated action.
    Approve,
    /// Deny a gated action.
    Deny,
}

#[derive(Debug, Serialize)]
struct PolicyRuleSnapshot<'a> {
    id: &'a str,
    target: &'a str,
}

#[derive(Debug, Serialize)]
struct PolicyLimitsSnapshot {
    queue_max_entries: usize,
    queue_max_bytes: usize,
    ctl_max_bytes: usize,
    status_max_bytes: usize,
}

#[derive(Debug, Serialize)]
struct PolicyRulesSnapshot<'a> {
    enabled: bool,
    limits: PolicyLimitsSnapshot,
    rules: Vec<PolicyRuleSnapshot<'a>>,
}

#[derive(Debug, Serialize)]
struct PolicyStatusSnapshot<'a> {
    id: &'a str,
    target: &'a str,
    decision: PolicyDecision,
    state: &'a str,
}

fn build_preflight_req_cbor(
    total: usize,
    queued: usize,
    consumed: usize,
    actions: &[PolicyAction],
) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(4)
        .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    writer
        .text("total")
        .and_then(|_| writer.unsigned(total as u64))
        .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    writer
        .text("queued")
        .and_then(|_| writer.unsigned(queued as u64))
        .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    writer
        .text("consumed")
        .and_then(|_| writer.unsigned(consumed as u64))
        .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    writer
        .text("actions")
        .and_then(|_| writer.array(actions.len()))
        .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    for action in actions {
        let state = if action.consumed { "consumed" } else { "queued" };
        let decision = policy_decision_label(action.decision);
        writer
            .map(4)
            .and_then(|_| writer.text("id"))
            .and_then(|_| writer.text(&action.id))
            .and_then(|_| writer.text("target"))
            .and_then(|_| writer.text(&action.target))
            .and_then(|_| writer.text("decision"))
            .and_then(|_| writer.text(decision))
            .and_then(|_| writer.text("state"))
            .and_then(|_| writer.text(state))
            .map_err(|err| cbor_error("policy/preflight/req.cbor", err))?;
    }
    Ok(writer.into_bytes())
}

fn build_preflight_diff_cbor(
    rules_total: usize,
    actions_total: usize,
    unmatched: usize,
    rules: &[PolicyRule],
    rule_counts: &[(usize, usize)],
) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(4)
        .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    writer
        .text("rules")
        .and_then(|_| writer.unsigned(rules_total as u64))
        .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    writer
        .text("actions")
        .and_then(|_| writer.unsigned(actions_total as u64))
        .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    writer
        .text("unmatched")
        .and_then(|_| writer.unsigned(unmatched as u64))
        .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    writer
        .text("entries")
        .and_then(|_| writer.array(rules.len()))
        .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    for (rule, (queued, consumed)) in rules.iter().zip(rule_counts.iter()) {
        writer
            .map(4)
            .and_then(|_| writer.text("id"))
            .and_then(|_| writer.text(&rule.id))
            .and_then(|_| writer.text("target"))
            .and_then(|_| writer.text(&rule.target))
            .and_then(|_| writer.text("queued"))
            .and_then(|_| writer.unsigned(*queued as u64))
            .and_then(|_| writer.text("consumed"))
            .and_then(|_| writer.unsigned(*consumed as u64))
            .map_err(|err| cbor_error("policy/preflight/diff.cbor", err))?;
    }
    Ok(writer.into_bytes())
}

fn render_rules_snapshot(
    enabled: bool,
    rules: &[PolicyRule],
    limits: PolicyLimits,
) -> Result<Vec<u8>, NineDoorError> {
    let snapshot = PolicyRulesSnapshot {
        enabled,
        limits: PolicyLimitsSnapshot {
            queue_max_entries: limits.queue_max_entries,
            queue_max_bytes: limits.queue_max_bytes,
            ctl_max_bytes: limits.ctl_max_bytes,
            status_max_bytes: limits.status_max_bytes,
        },
        rules: rules
            .iter()
            .map(|rule| PolicyRuleSnapshot {
                id: rule.id.as_str(),
                target: rule.target.as_str(),
            })
            .collect(),
    };
    serde_json::to_vec_pretty(&snapshot).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("policy rule snapshot failed: {err}"),
        )
    })
}

fn parse_path_pattern(target: &str) -> Result<Vec<PolicyPathSegment>, NineDoorError> {
    if !target.starts_with('/') {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "policy rule target must be absolute",
        ));
    }
    let mut segments = Vec::new();
    for component in target.split('/').filter(|segment| !segment.is_empty()) {
        if component == ".." {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "policy rule target contains disallowed '..'",
            ));
        }
        if component == "*" {
            segments.push(PolicyPathSegment::Wildcard);
        } else {
            segments.push(PolicyPathSegment::Literal(component.to_owned()));
        }
    }
    if segments.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "policy rule target must not be root",
        ));
    }
    if segments.len() > MAX_POLICY_PATH_COMPONENTS {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!(
                "policy rule target exceeds max depth {}",
                MAX_POLICY_PATH_COMPONENTS
            ),
        ));
    }
    Ok(segments)
}

fn parse_action_lines(data: &[u8]) -> Result<Vec<ActionRequest>, NineDoorError> {
    let text = std::str::from_utf8(data).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("action queue must be utf-8: {err}"),
        )
    })?;
    let mut actions = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let action: ActionRequest = serde_json::from_str(trimmed).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid action entry: {err}"),
            )
        })?;
        validate_action_id(&action.id)?;
        validate_action_target(&action.target)?;
        actions.push(action);
    }
    Ok(actions)
}

fn ensure_stream_len(label: &str, len: usize) -> Result<(), NineDoorError> {
    if len > UI_MAX_STREAM_BYTES {
        return Err(NineDoorError::protocol(
            ErrorCode::TooBig,
            format!(
                "{label} output exceeds {} bytes",
                UI_MAX_STREAM_BYTES
            ),
        ));
    }
    Ok(())
}

fn cbor_error(label: &str, err: CborError) -> NineDoorError {
    match err {
        CborError::TooLarge => NineDoorError::protocol(
            ErrorCode::TooBig,
            format!("{label} output exceeds {} bytes", UI_MAX_STREAM_BYTES),
        ),
    }
}

fn policy_decision_label(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Approve => "approve",
        PolicyDecision::Deny => "deny",
    }
}

fn validate_action_id(id: &str) -> Result<(), NineDoorError> {
    if id.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "action id must not be empty",
        ));
    }
    if id.len() > MAX_ACTION_ID_LEN {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("action id exceeds max length {}", MAX_ACTION_ID_LEN),
        ));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "action id must be alphanumeric, '-' or '_'",
        ));
    }
    Ok(())
}

fn validate_action_target(target: &str) -> Result<(), NineDoorError> {
    for component in target.split('/').filter(|segment| !segment.is_empty()) {
        if component == "*" {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "action target must not include wildcards",
            ));
        }
    }
    parse_path_pattern(target).map(|_| ())
}

fn ensure_utf8(data: &[u8], label: &str) -> Result<(), NineDoorError> {
    std::str::from_utf8(data).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must be utf-8: {err}"),
        )
    })?;
    Ok(())
}

fn validate_json_lines(data: &[u8], label: &str) -> Result<(), NineDoorError> {
    let text = std::str::from_utf8(data).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must be utf-8: {err}"),
        )
    })?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(trimmed).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid {label} entry: {err}"),
            )
        })?;
    }
    Ok(())
}

struct AppendBounds {
    len: usize,
    short: bool,
}

fn apply_append(
    current_len: usize,
    offset: u64,
    max_len: usize,
    requested: usize,
    label: &str,
) -> Result<AppendBounds, NineDoorError> {
    let expected_offset = current_len as u64;
    let provided_offset = if offset == u64::MAX {
        expected_offset
    } else {
        offset
    };
    let bounds = append_only_write_bounds(expected_offset, provided_offset, max_len, requested)
        .map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("{label} append offset rejected: {err}"),
            )
        })?;
    Ok(AppendBounds {
        len: bounds.len,
        short: bounds.short,
    })
}
