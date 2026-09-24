// Author: Lukas Bower
// Purpose: Refuse adapter promotion unless native evaluations bind comparable artifacts, current deployment state and predeclared bounds.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{ensure, Result};
use cohesix_evidence::digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// All fields are part of comparison identity, including the exact native evaluator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationContext {
    /// Exact immutable evaluation dataset snapshot.
    pub dataset_sha256: String,
    /// Declared held-out split; training and evaluation rows remain distinct.
    pub split: String,
    /// Digest of tokenization and preprocessing settings.
    pub preprocessing_sha256: String,
    /// Qualified base artifact identity, including its revision.
    pub base_sha256: String,
    /// Exact tokenizer and chat-template identity.
    pub tokenizer_sha256: String,
    /// Native evaluator implementation that emitted the metrics.
    pub evaluator: String,
    /// Pinned evaluator package version.
    pub evaluator_version: String,
    /// Complete resolved evaluation settings digest.
    pub parameters_sha256: String,
    /// Explicit fixed seed or independently qualified deterministic policy.
    pub seed_policy: String,
    /// Pinned serving and framework compatibility identity.
    pub runtime_sha256: String,
    /// Comparable device, precision and resource configuration.
    pub resource_sha256: String,
}

/// A report retained from the evaluator; the controller never supplies scores.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    /// Exact native report grammar.
    pub schema: String,
    /// Adapter or qualified base actually evaluated.
    pub artifact_sha256: String,
    /// Complete comparison identity; every field must match the baseline.
    pub context: EvaluationContext,
    /// Number of independently retained evaluated examples.
    pub samples: u32,
    /// Native completion timestamp used for freshness checks.
    pub completed_unix_ms: u64,
    /// Observed metric values; absent and non-finite scores cannot pass.
    pub metrics: BTreeMap<String, f64>,
    /// CAS digest of the original native evaluator report.
    pub native_report_sha256: String,
}

/// Direction is explicit; smaller loss and greater accuracy are different contracts.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Smaller values improve this metric, such as held-out loss.
    Lower,
    /// Larger values improve this metric, such as classification accuracy.
    Higher,
}

/// Every required metric has an absolute bound and a maximum permitted regression.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricBound {
    /// Whether lower or higher values satisfy this metric.
    pub direction: Direction,
    /// Mandatory absolute quality or safety bound.
    pub absolute_bound: f64,
    /// Largest preapproved regression against the accepted baseline.
    pub maximum_regression: f64,
}

/// Operator-approved policy is bound before evaluation and cannot change at promotion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationPolicy {
    /// Predeclared minimum sample count.
    pub minimum_samples: u32,
    /// Finite evidence lifetime; comparisons never renew it.
    pub maximum_age_ms: u64,
    /// Required absolute bounds and regression limits, declared before execution.
    pub metrics: BTreeMap<String, MetricBound>,
}

impl EvaluationPolicy {
    /// Refuse unbounded or non-finite criteria before native work can start.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=1_000_000).contains(&self.minimum_samples)
                && (1..=3_600_000).contains(&self.maximum_age_ms)
                && !self.metrics.is_empty()
                && self.metrics.len() <= 16,
            "invalid_policy evaluation-bounds"
        );
        for (name, bound) in &self.metrics {
            ensure!(
                !name.is_empty()
                    && name.len() <= 64
                    && !name.chars().any(char::is_control)
                    && bound.absolute_bound.is_finite()
                    && bound.maximum_regression.is_finite()
                    && bound.maximum_regression >= 0.0,
                "invalid_policy metric-bound"
            );
        }
        Ok(())
    }
}

/// A first deployment explicitly names the qualified base and retains it for recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentState {
    /// Compare-and-swap identity of the accepted deployment.
    pub generation: u64,
    /// None explicitly means the qualified base before first deployment.
    pub adapter_sha256: Option<String>,
    /// Exact accepted serving artifact, including the first-deployment base.
    pub served_artifact_sha256: String,
    /// Pinned serving and framework compatibility identity.
    pub runtime_sha256: String,
    /// Observed native health, never inferred from a registry rename.
    pub healthy: bool,
    /// Whether the retained recovery target has verified behavior.
    pub rollback_verified: bool,
}

/// Immutable comparison bindings are rechecked against live state before promotion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    /// Exact adapter approved by this comparison.
    pub candidate_sha256: String,
    /// Frozen accepted generation and recovery target.
    pub baseline: DeploymentState,
    /// Digest of the full candidate evaluator report.
    pub candidate_report_sha256: String,
    /// Digest of the full baseline evaluator report.
    pub baseline_report_sha256: String,
    /// Digest of the predeclared acceptance policy.
    pub policy_sha256: String,
    /// Digest of all comparison compatibility fields.
    pub context_sha256: String,
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Canonical serialization binds the complete policy, report or native state.
pub fn identity(value: &impl Serialize) -> Result<String> {
    Ok(digest(&serde_json::to_vec(value)?))
}

impl EvaluationContext {
    fn validate(&self) -> Result<()> {
        for hash in [
            &self.dataset_sha256,
            &self.preprocessing_sha256,
            &self.base_sha256,
            &self.tokenizer_sha256,
            &self.parameters_sha256,
            &self.runtime_sha256,
            &self.resource_sha256,
        ] {
            ensure!(sha256(hash), "invalid_evidence evaluation-context-digest");
        }
        for label in [
            &self.split,
            &self.evaluator,
            &self.evaluator_version,
            &self.seed_policy,
        ] {
            ensure!(
                !label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control),
                "invalid_evidence evaluation-context-label"
            );
        }
        Ok(())
    }
}

fn validate_report(report: &Evaluation, policy: &EvaluationPolicy, now: u64) -> Result<()> {
    report.context.validate()?;
    ensure!(
        report.schema == "cohesix-native-evaluation/v1"
            && sha256(&report.artifact_sha256)
            && sha256(&report.native_report_sha256)
            && report.samples >= policy.minimum_samples
            && report.completed_unix_ms > 0
            && report.completed_unix_ms <= now
            && now - report.completed_unix_ms < policy.maximum_age_ms
            && !report.metrics.is_empty()
            && report.metrics.len() <= 16
            && report.metrics.values().all(|v| v.is_finite()),
        "invalid_evidence evaluation-freshness-samples-or-metrics"
    );
    Ok(())
}

/// Compare actual evaluator reports. A passing result is evidence, never a grant.
pub fn compare(
    candidate: &Evaluation,
    baseline: &Evaluation,
    policy: &EvaluationPolicy,
    current: &DeploymentState,
    now: u64,
) -> Result<Comparison> {
    policy.validate()?;
    validate_report(candidate, policy, now)?;
    validate_report(baseline, policy, now)?;
    ensure!(
        current.healthy
            && current.rollback_verified
            && sha256(&current.served_artifact_sha256)
            && current.runtime_sha256 == candidate.context.runtime_sha256
            && baseline.artifact_sha256 == current.served_artifact_sha256
            && candidate.context == baseline.context
            && candidate.samples == baseline.samples,
        "invalid_evidence incomparable-or-unrecoverable-baseline"
    );
    match &current.adapter_sha256 {
        Some(adapter) => ensure!(
            adapter == &current.served_artifact_sha256,
            "invalid_evidence accepted-adapter-identity"
        ),
        None => ensure!(
            current.generation == 0 && baseline.artifact_sha256 == baseline.context.base_sha256,
            "invalid_evidence first-deployment-base"
        ),
    }
    for (metric, bound) in &policy.metrics {
        let value = candidate.metrics.get(metric);
        let reference = baseline.metrics.get(metric);
        ensure!(
            value.is_some() && reference.is_some(),
            "invalid_evidence missing-metric"
        );
        if let (Some(value), Some(reference)) = (value, reference) {
            let (absolute, regression) = match bound.direction {
                Direction::Lower => (*value <= bound.absolute_bound, value - reference),
                Direction::Higher => (*value >= bound.absolute_bound, reference - value),
            };
            ensure!(
                absolute && regression.is_finite() && regression <= bound.maximum_regression,
                "refused candidate-regression {metric}"
            );
        }
    }
    Ok(Comparison {
        candidate_sha256: candidate.artifact_sha256.clone(),
        baseline: current.clone(),
        candidate_report_sha256: identity(candidate)?,
        baseline_report_sha256: identity(baseline)?,
        policy_sha256: identity(policy)?,
        context_sha256: identity(&candidate.context)?,
    })
}

/// Renewed authority is checked by the admitted executor; old evidence grants nothing.
pub fn recheck(
    comparison: &Comparison,
    candidate: &Evaluation,
    baseline: &Evaluation,
    policy: &EvaluationPolicy,
    current: &DeploymentState,
    now: u64,
) -> Result<()> {
    ensure!(
        comparison.baseline == *current,
        "conflict deployment-changed-renew-comparison-and-approval"
    );
    let renewed = compare(candidate, baseline, policy, current, now)?;
    ensure!(
        identity(&renewed)? == identity(comparison)?,
        "invalid_evidence comparison-substitution"
    );
    Ok(())
}
