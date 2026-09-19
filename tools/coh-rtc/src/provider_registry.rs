// Author: Lukas Bower
// Purpose: Validate the provider extension of the frozen host integration graph without promoting execution evidence.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! Compiler-owned Release A provider requirements. Registration is not admission
//! or observed execution; all live promotion remains an evidence obligation.

use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Versioned extension carried inside `host-integration-dependency/v1`.
pub const SCHEMA: &str = "cohesix-provider-contract/v1";

/// Shared adapter phases; discovery and dispatch cannot stand in for verification.
pub const PHASES: [&str; 7] = [
    "discover",
    "preflight",
    "execute",
    "observe",
    "verify",
    "compensate",
    "export_evidence",
];

/// Provider IR embedded in the existing host-integration source.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRegistry {
    pub schema: String,
    pub profiles: Vec<ReferenceProfile>,
    pub families: Vec<Provider>,
    pub integration_surfaces: Vec<IntegrationSurface>,
    pub read_visibility: Vec<ReadVisibility>,
    pub export: ExportPolicy,
    pub relay_credentials: Vec<RelayCredential>,
    pub identity_mappings: Vec<cohesix_identity::Policy>,
    pub siem_delivery: SiemDelivery,
    pub gpu_executor: cohesix_authority::gpu::ExecutorContract,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_bus: Vec<cohesix_authority::bus::Endpoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launchd_targets: Vec<cohesix_authority::macos::LaunchdTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macos_targets: Vec<cohesix_authority::mac_release::Target>,
    pub deployment_profiles: Vec<cohesix_authority::package::Profile>,
}

/// A single deployment-owned destination; endpoints and secrets never come from event payloads.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SiemDelivery {
    pub schema: String,
    pub enabled: bool,
    pub endpoint: String,
    pub credential_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_certificate_path_ref: Option<String>,
    pub maximum_entries: usize,
    pub maximum_wal_bytes: usize,
    pub maximum_attempts: u32,
    pub timeout_ms: u32,
    pub maximum_backoff_ms: u64,
}

/// Host-only credential references; no values or new VM authority are generated.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RelayCredential {
    pub peer: String,
    pub request_auth_ref: String,
    pub delegated_ticket_ref: String,
}

/// Derived exporters share a finite field and format contract; none issue receipts.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPolicy {
    pub schema: String,
    pub formats: Vec<String>,
    pub maximum_bytes: usize,
    pub field_allowlist: Vec<String>,
    pub build_actions: Vec<String>,
}

/// Component-prefix read policy; an explicit root rule covers every exposed path.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadVisibility {
    pub prefix: String,
    pub class: String,
    pub owner_binding: String,
}

/// A public projection retains one stable graph owner and explicit execution mode.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationSurface {
    pub id: String,
    pub integration_id: String,
    pub owner: String,
    pub owning_task: String,
    pub topology: String,
    pub transports: Vec<String>,
    pub posture: String,
    pub required_actions: Vec<String>,
    pub worker_roles: Vec<String>,
    pub worker_evidence_tier: String,
    pub identity_requirement: String,
    pub secret_requirement: String,
    pub durability_owner: String,
    pub package_artifacts: Vec<String>,
    pub conformance_runner: String,
    pub observed_mode: String,
    pub read_visibility: String,
}

/// Exact reference identity, never an open-ended compatibility range.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceProfile {
    pub id: String,
    pub os: String,
    pub architecture: String,
    pub board: String,
    pub ubuntu: String,
    pub l4t: String,
    pub jetpack: String,
    pub cuda_toolkit: String,
    pub compute_capability: String,
    pub shared_memory: bool,
    pub host_headroom_bytes: u64,
    pub observation_ttl_ms: u32,
    pub unsupported: Vec<String>,
    pub execution_lanes: Vec<String>,
}

/// A family extends one stable 26e integration row.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub integration_id: String,
    pub owner: String,
    pub owning_task: String,
    pub required_for_release_a: bool,
    pub availability: Availability,
    pub actions: Vec<Action>,
}

/// Registry availability cannot assert observed live execution.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    NotEnabled,
    NotImplemented,
    Candidate,
}

/// One bounded action contract; clients cannot choose its admission posture.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub legacy_argument_fields: Option<Vec<String>>,
    pub effect: Effect,
    pub target_grammar: String,
    pub argument_schema_ref: String,
    pub receipt_schema_ref: String,
    pub evidence_profile: String,
    pub read_visibility: String,
    pub timeout_ms: u32,
    pub maximum_request_bytes: u32,
    pub idempotency_required: bool,
    pub writer_epoch_required: bool,
    pub operator_approval_required: bool,
    pub admission_mode: String,
    pub decision_requirement: String,
    pub intent_schema_ref: String,
    pub required_fact_schema_ref: String,
    pub policy_id: String,
    pub maximum_grant_scope: Vec<String>,
    pub state_freshness_bound_ms: u32,
    pub reservation_or_recheck_mode: String,
    pub decision_receipt_schema_ref: String,
    pub selected_governance_mode: String,
    pub supported_governance_modes: Vec<String>,
    pub authority_custodian: String,
    pub credential_custodian: String,
    pub bypass_posture: String,
    pub failure_posture: String,
    pub redaction: Vec<String>,
    pub native_identity_fields: Vec<String>,
    pub correlation_fields: Vec<String>,
    pub lifecycle: Vec<Phase>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    ReadOnly,
    Mutation,
}

/// A phase names the owner and observable condition, including non-reversible effects.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub id: String,
    pub owner: String,
    pub postcondition: String,
    pub unavailable: String,
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
}

fn unique(values: &[String]) -> bool {
    values.len() <= 64
        && values.iter().all(|value| identifier(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

impl ProviderRegistry {
    /// Validate identity, bounded lifecycle, Release A admission and stable graph links.
    pub fn validate(&mut self, integration_ids: &BTreeSet<String>, build_plan: &str) -> Result<()> {
        cohesix_authority::mac_release::validate(&self.macos_targets)
            .map_err(anyhow::Error::msg)?;
        cohesix_authority::macos::validate_launchd(&self.launchd_targets)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            self.gpu_executor.is_supported(),
            "unsupported GPU executor ABI bounds or profile"
        );
        ensure!(self.field_bus.len() <= 64, "field bus endpoint bound");
        let mut bus_ids = BTreeSet::new();
        for endpoint in &self.field_bus {
            endpoint
                .validate()
                .map_err(|error| anyhow::anyhow!(error))?;
            ensure!(bus_ids.insert(&endpoint.id), "duplicate field bus endpoint");
        }
        let delivery = &self.siem_delivery;
        if let Some(reference) = &delivery.ca_certificate_path_ref {
            cohesix_authority::secret::validate_reference(reference)?;
        }
        ensure!(
            delivery.schema == "cohesix-siem-delivery/v1"
                && delivery.endpoint.starts_with("https://")
                && text(&delivery.endpoint)
                && !delivery.endpoint.contains(['@', '?', '#'])
                && delivery.credential_ref.starts_with("env:")
                && delivery.credential_ref[4..].len() > 1
                && delivery.credential_ref[4..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                && delivery.maximum_entries > 0
                && delivery.maximum_entries <= 64
                && delivery.maximum_wal_bytes >= 65536
                && delivery.maximum_wal_bytes <= 4 * 1024 * 1024
                && delivery.maximum_attempts > 0
                && delivery.maximum_attempts <= 8
                && delivery.timeout_ms > 0
                && delivery.timeout_ms <= 5000
                && delivery.maximum_backoff_ms >= 1000
                && delivery.maximum_backoff_ms <= 300000,
            "invalid SIEM delivery policy"
        );
        ensure!(
            self.relay_credentials.len() <= 32,
            "relay credential count bound"
        );
        let mut peers = BTreeSet::new();
        for peer in &self.relay_credentials {
            ensure!(
                identifier(&peer.peer) && peers.insert(&peer.peer),
                "invalid relay credential peer"
            );
            for reference in [&peer.request_auth_ref, &peer.delegated_ticket_ref] {
                ensure!(
                    reference.len() <= 512
                        && reference
                            .strip_prefix("env:")
                            .is_some_and(|name| !name.is_empty()
                                && name.bytes().all(|b| b.is_ascii_uppercase()
                                    || b.is_ascii_digit()
                                    || b == b'_')),
                    "relay credentials require named environment references"
                );
            }
        }
        ensure!(
            self.export.schema == "cohesix-derived-export/v1"
                && self.export.maximum_bytes > 0
                && self.export.maximum_bytes <= 65_536,
            "invalid export contract bounds"
        );
        ensure!(
            self.export.formats
                == [
                    "prometheus",
                    "otel",
                    "cloudevents",
                    "in_toto",
                    "siem",
                    "slsa"
                ],
            "unsupported export formats"
        );
        ensure!(
            self.export.field_allowlist
                == [
                    "ticket_id",
                    "action",
                    "graph_sha256",
                    "provider_graph_sha256",
                    "kind",
                    "outcome",
                    "source",
                    "native_identity_sha256",
                    "artifact_sha256",
                    "record_sha256"
                ],
            "export fields must preserve the bounded redaction contract"
        );
        ensure!(
            self.export.build_actions.is_empty(),
            "no build action currently provides a validated build provenance contract"
        );
        if self.schema != SCHEMA
            || self.families.is_empty()
            || self.families.len() > 64
            || self.profiles.is_empty()
            || self.profiles.len() > 16
        {
            bail!("provider registry schema or collection bound is invalid");
        }
        let mut profiles = BTreeSet::new();
        for profile in &self.profiles {
            if !identifier(&profile.id)
                || !profiles.insert(&profile.id)
                || [
                    &profile.os,
                    &profile.architecture,
                    &profile.board,
                    &profile.ubuntu,
                    &profile.l4t,
                    &profile.jetpack,
                    &profile.cuda_toolkit,
                    &profile.compute_capability,
                ]
                .iter()
                .any(|value| !text(value))
                || profile.host_headroom_bytes == 0
                || profile.host_headroom_bytes > 1 << 40
                || profile.observation_ttl_ms == 0
                || profile.observation_ttl_ms > 60_000
                || !unique(&profile.unsupported)
                || !unique(&profile.execution_lanes)
                || profile.execution_lanes.is_empty()
            {
                bail!("invalid or duplicate provider profile {}", profile.id);
            }
        }
        let mut providers = BTreeSet::new();
        let mut actions = BTreeSet::new();
        for provider in &self.families {
            if !identifier(&provider.id)
                || !providers.insert(&provider.id)
                || !integration_ids.contains(&provider.integration_id)
                || !identifier(&provider.owner)
                || !identifier(&provider.owning_task)
                || !build_plan.contains(&format!("Title/ID: {}\n", provider.owning_task))
                || provider.actions.len() > 64
                || (provider.required_for_release_a && provider.actions.is_empty())
            {
                bail!(
                    "invalid provider identity or integration link {}",
                    provider.id
                );
            }
            for action in &provider.actions {
                if !identifier(&action.id)
                    || !actions.insert(&action.id)
                    || !action.id.starts_with(&format!("{}.", provider.id))
                {
                    bail!("duplicate or mismatched provider action {}", action.id);
                }
                validate_action(action)?;
            }
        }
        if self.integration_surfaces.is_empty() || self.integration_surfaces.len() > 64 {
            bail!("integration surface collection bound is invalid");
        }
        let mut surfaces = BTreeSet::new();
        for surface in &self.integration_surfaces {
            if !identifier(&surface.id)
                || !surfaces.insert(&surface.id)
                || !integration_ids.contains(&surface.integration_id)
                || !identifier(&surface.owner)
                || !identifier(&surface.owning_task)
                || !build_plan.contains(&format!("Title/ID: {}\n", surface.owning_task))
                || !unique(&surface.transports)
                || surface.transports.is_empty()
                || !unique(&surface.required_actions)
                || surface
                    .required_actions
                    .iter()
                    .any(|action| !actions.contains(action))
                || !unique(&surface.worker_roles)
                || !matches!(
                    surface.worker_evidence_tier.as_str(),
                    "none" | "model_or_session" | "m26e_executable" | "m28b_production_bundle"
                )
                || !matches!(
                    surface.posture.as_str(),
                    "read_only" | "ticket_control" | "local_operator"
                )
                || !matches!(
                    surface.observed_mode.as_str(),
                    "unknown" | "missing" | "disabled" | "fixture" | "mock" | "dry-run"
                )
                || !matches!(
                    surface.read_visibility.as_str(),
                    "public" | "ticket_scoped" | "admin_only"
                )
                || [
                    &surface.topology,
                    &surface.identity_requirement,
                    &surface.secret_requirement,
                    &surface.durability_owner,
                    &surface.conformance_runner,
                ]
                .iter()
                .any(|value| !text(value))
                || surface.package_artifacts.is_empty()
                || surface.package_artifacts.len() > 64
                || surface
                    .package_artifacts
                    .iter()
                    .any(|path| !artifact_path(path))
            {
                bail!("invalid integration surface {}", surface.id);
            }
        }
        ensure!(
            !self.deployment_profiles.is_empty() && self.deployment_profiles.len() <= 16,
            "deployment profile count bound"
        );
        let mut package_ids = BTreeSet::new();
        for profile in &mut self.deployment_profiles {
            profile.validate()?;
            ensure!(
                package_ids.insert(profile.id.clone())
                    && profile
                        .integration_surfaces
                        .iter()
                        .all(|id| surfaces.contains(id)),
                "duplicate package profile or unknown integration surface"
            );
            profile.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
            profile.integration_surfaces.sort();
            profile.required_credentials.sort();
        }
        self.deployment_profiles.sort_by(|a, b| a.id.cmp(&b.id));
        let mut prefixes = BTreeSet::new();
        if self.read_visibility.is_empty()
            || self.read_visibility.len() > 64
            || !self
                .read_visibility
                .iter()
                .any(|rule| rule.prefix == "/" && rule.class == "admin_only")
        {
            bail!("read visibility requires an explicit admin root rule");
        }
        for rule in &self.read_visibility {
            if !prefixes.insert(&rule.prefix)
                || !rule.prefix.starts_with('/')
                || (rule.prefix != "/" && !artifact_path(&rule.prefix[1..]))
                || !matches!(
                    rule.class.as_str(),
                    "public" | "ticket_scoped" | "admin_only"
                )
                || !matches!(
                    rule.owner_binding.as_str(),
                    "none" | "worker_subject" | "explicit_scope"
                )
                || (rule.class == "ticket_scoped" && rule.owner_binding == "none")
            {
                bail!("invalid read visibility rule {}", rule.prefix);
            }
        }
        ensure!(
            self.identity_mappings.len() <= 32,
            "identity mapping count bound"
        );
        let identity_actions = self
            .families
            .iter()
            .flat_map(|family| family.actions.iter())
            .map(|action| action.id.clone())
            .collect();
        let mut mapping_ids = BTreeSet::new();
        for mapping in &self.identity_mappings {
            ensure!(
                mapping_ids.insert(&mapping.id),
                "duplicate identity mapping"
            );
            mapping.validate(&identity_actions)?;
        }
        self.identity_mappings.sort_by(|a, b| a.id.cmp(&b.id));
        self.read_visibility.sort_by(|a, b| a.prefix.cmp(&b.prefix));
        self.integration_surfaces.sort_by(|a, b| a.id.cmp(&b.id));
        self.profiles.sort_by(|a, b| a.id.cmp(&b.id));
        self.families.sort_by(|a, b| a.id.cmp(&b.id));
        for family in &mut self.families {
            family.actions.sort_by(|a, b| a.id.cmp(&b.id));
        }
        Ok(())
    }
}

fn artifact_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('/')
        && value.split('/').all(|part| {
            !matches!(part, "" | "." | "..")
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

fn validate_action(action: &Action) -> Result<()> {
    let fields = [
        &action.argument_schema_ref,
        &action.receipt_schema_ref,
        &action.evidence_profile,
        &action.intent_schema_ref,
        &action.required_fact_schema_ref,
        &action.policy_id,
        &action.decision_receipt_schema_ref,
        &action.authority_custodian,
        &action.credential_custodian,
        &action.bypass_posture,
        &action.failure_posture,
    ];
    if action
        .legacy_argument_fields
        .as_ref()
        .is_some_and(|fields| !unique(fields))
        || fields.iter().any(|value| !text(value))
        || !matches!(
            action.target_grammar.as_str(),
            "native_id" | "cas_sha256" | "physical_device" | "none"
        )
        || !matches!(
            action.read_visibility.as_str(),
            "public" | "ticket_scoped" | "admin_only"
        )
        || action.timeout_ms == 0
        || action.timeout_ms > 300_000
        || action.maximum_request_bytes == 0
        || action.maximum_request_bytes > 8192
        || action.state_freshness_bound_ms == 0
        || action.state_freshness_bound_ms > 60_000
        || !unique(&action.maximum_grant_scope)
        || action.maximum_grant_scope != [action.id.clone()]
        || !unique(&action.native_identity_fields)
        || action.native_identity_fields.is_empty()
        || !unique(&action.correlation_fields)
        || !action.correlation_fields.iter().any(|v| v == "ticket_id")
        || !unique(&action.redaction)
        || !action.redaction.iter().any(|v| v == "credentials")
        || action.supported_governance_modes != ["observe", "recommend"]
        || action.selected_governance_mode != "observe"
        || action.reservation_or_recheck_mode != "recheck_before_dispatch"
    {
        bail!("invalid bounded provider action {}", action.id);
    }
    match action.effect {
        Effect::Mutation
            if !action.idempotency_required
                || !action.writer_epoch_required
                || !action.operator_approval_required
                || action.admission_mode != "operator_approved"
                || action.decision_requirement != "unavailable" =>
        {
            bail!("Release A mutation {} weakens admission", action.id)
        }
        Effect::ReadOnly
            if action.operator_approval_required
                || action.admission_mode != "read_only"
                || action.decision_requirement != "not_required" =>
        {
            bail!(
                "read-only provider action {} has invalid admission",
                action.id
            )
        }
        _ => {}
    }
    if action.lifecycle.len() != PHASES.len() {
        bail!("provider action {} lacks complete lifecycle", action.id);
    }
    for (phase, expected) in action.lifecycle.iter().zip(PHASES) {
        if phase.id != expected
            || !identifier(&phase.owner)
            || !text(&phase.postcondition)
            || !matches!(
                phase.unavailable.as_str(),
                "not_enabled" | "not_implemented" | "not_supported"
            )
        {
            bail!("provider action {} has invalid lifecycle phase", action.id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> (ProviderRegistry, BTreeSet<String>, String) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join("configs/host_integration_acceptance.toml"))
            .expect("source matrix");
        let value: toml::Value = toml::from_str(&text).expect("TOML");
        let providers: ProviderRegistry =
            value["providers"].clone().try_into().expect("provider IR");
        let ids = value["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .map(|row| row["id"].as_str().expect("dependency id").to_owned())
            .collect();
        let plan = std::fs::read_to_string(root.join("docs/BUILD_PLAN.md")).expect("plan");
        (providers, ids, plan)
    }

    #[test]
    fn field_bus_maps_require_exact_protocol_and_separately_approved_controls() {
        use cohesix_authority::bus::{Endpoint, Operation, Point, Protocol, Transport};
        let (mut registry, ids, plan) = source();
        let endpoint = Endpoint {
            id: "plc".into(),
            protocol: Protocol::Modbus,
            transport: Transport::Tcp {
                address: "127.0.0.1:502".into(),
            },
            unit: 1,
            master: 0,
            outstation: 0,
            timeout_ms: 1000,
            poll_interval_ms: 1000,
            observation_ttl_ms: 2000,
            points: vec![Point {
                id: "setpoint".into(),
                approval_required: true,
                operation: Operation::ModbusWrite {
                    function: 6,
                    address: 3,
                    value: 42,
                },
            }],
        };
        registry.field_bus = vec![endpoint.clone()];
        registry.validate(&ids, &plan).unwrap();
        registry.field_bus[0].points[0].approval_required = false;
        assert!(registry.validate(&ids, &plan).is_err());
        registry.field_bus = vec![endpoint.clone(), endpoint.clone()];
        assert!(registry.validate(&ids, &plan).is_err());
        registry.field_bus = vec![endpoint];
        registry.field_bus[0].protocol = Protocol::Dnp3;
        assert!(registry.validate(&ids, &plan).is_err());
    }

    fn systemd_action(registry: &mut ProviderRegistry) -> &mut Action {
        registry
            .families
            .iter_mut()
            .find(|p| p.id == "systemd")
            .expect("systemd")
            .actions
            .iter_mut()
            .find(|a| a.id == "systemd.restart")
            .expect("restart")
    }

    #[test]
    fn siem_ca_selection_is_an_explicit_host_reference_without_literal_fallback() {
        let (mut registry, ids, plan) = source();
        registry.siem_delivery.ca_certificate_path_ref = Some("env:COHESIX_SIEM_CA_PATH".into());
        registry.validate(&ids, &plan).unwrap();
        registry.siem_delivery.ca_certificate_path_ref = Some("/etc/ssl/certificate.pem".into());
        assert!(registry.validate(&ids, &plan).is_err());
        registry.siem_delivery.ca_certificate_path_ref =
            Some("file:/etc/cohesix/ca-location".into());
        registry.validate(&ids, &plan).unwrap();
    }

    #[test]
    fn mig_executor_selection_cannot_weaken_the_bounded_local_abi() {
        let (mut registry, ids, plan) = source();
        registry.gpu_executor.profile = "nvidia-mig-cuda13".into();
        registry.validate(&ids, &plan).unwrap();
        registry.gpu_executor.maximum_jobs += 1;
        assert!(registry.validate(&ids, &plan).is_err());
    }

    #[test]
    fn reference_and_mutation_contract_preserve_exact_owner_requirements() {
        let (mut registry, ids, plan) = source();
        registry.validate(&ids, &plan).expect("canonical source");
        let profile = registry
            .profiles
            .iter()
            .find(|p| p.id == "jetson-orin-nano-jp7")
            .expect("reference");
        assert_eq!(profile.cuda_toolkit, "13.2.2");
        assert_eq!(profile.l4t, "39.2.1");
        assert_eq!(profile.jetpack, "7.2.1");
        assert_eq!(profile.compute_capability, "8.7");
        assert_eq!(profile.unsupported, ["mig", "dla", "pva"]);
        let restart = systemd_action(&mut registry);
        assert_eq!(restart.decision_requirement, "unavailable");
        assert_eq!(restart.admission_mode, "operator_approved");
        assert!(restart.operator_approval_required);
        assert_eq!(
            restart
                .lifecycle
                .iter()
                .map(|phase| phase.id.as_str())
                .collect::<Vec<_>>(),
            [
                "discover",
                "preflight",
                "execute",
                "observe",
                "verify",
                "compensate",
                "export_evidence"
            ]
        );
    }

    #[test]
    fn weakening_authority_or_enabling_unimplemented_enforcement_is_rejected() {
        let mutations: [fn(&mut Action); 6] = [
            |a| a.operator_approval_required = false,
            |a| a.writer_epoch_required = false,
            |a| a.idempotency_required = false,
            |a| a.decision_requirement = "not_required".into(),
            |a| a.maximum_grant_scope.push("systemd.stop".into()),
            |a| a.selected_governance_mode = "enforce".into(),
        ];
        for mutate in mutations {
            let (mut registry, ids, plan) = source();
            mutate(systemd_action(&mut registry));
            assert!(registry.validate(&ids, &plan).is_err());
        }
    }

    #[test]
    fn untrusted_targets_and_incomplete_native_evidence_cannot_compile() {
        let mutations: [fn(&mut Action); 7] = [
            |a| a.target_grammar = "shell".into(),
            |a| a.maximum_request_bytes = 8193,
            |a| a.state_freshness_bound_ms = 0,
            |a| {
                a.lifecycle.remove(4);
            },
            |a| a.lifecycle[0].id = "execute".into(),
            |a| a.native_identity_fields.clear(),
            |a| a.correlation_fields.retain(|field| field != "ticket_id"),
        ];
        for mutate in mutations {
            let (mut registry, ids, plan) = source();
            mutate(systemd_action(&mut registry));
            assert!(registry.validate(&ids, &plan).is_err());
        }
    }

    #[test]
    fn orphaned_or_duplicate_provider_identities_fail_generation() {
        let (mut registry, ids, plan) = source();
        let original = registry.clone();
        registry.families[0].integration_id = "not-in-the-26e-graph".into();
        assert!(registry.validate(&ids, &plan).is_err());
        registry = original.clone();
        registry.families.push(registry.families[0].clone());
        assert!(registry.validate(&ids, &plan).is_err());
        registry = original;
        let action = systemd_action(&mut registry).clone();
        registry
            .families
            .iter_mut()
            .find(|p| p.id == "systemd")
            .expect("systemd")
            .actions
            .push(action);
        assert!(registry.validate(&ids, &plan).is_err());
    }

    #[test]
    fn registry_cannot_deserialize_live_availability_or_unknown_fields() {
        let (registry, _, _) = source();
        let mut json = serde_json::to_value(&registry).expect("serialize");
        json["families"][0]["availability"] = serde_json::json!("live");
        assert!(serde_json::from_value::<ProviderRegistry>(json).is_err());
        let mut json = serde_json::to_value(&registry).expect("serialize");
        json["families"][0]["shell"] = serde_json::json!("echo ok");
        assert!(serde_json::from_value::<ProviderRegistry>(json).is_err());
    }
    #[test]
    fn surface_orphans_missing_packages_and_invented_live_mode_are_rejected() {
        for defect in ["action", "integration", "package", "mode", "runner"] {
            let (mut registry, ids, plan) = source();
            let surface = &mut registry.integration_surfaces[0];
            match defect {
                "action" => surface.required_actions.push("unregistered.execute".into()),
                "integration" => surface.integration_id = "orphan".into(),
                "package" => surface.package_artifacts.clear(),
                "mode" => surface.observed_mode = "live".into(),
                _ => surface.conformance_runner.clear(),
            }
            assert!(registry.validate(&ids, &plan).is_err(), "{defect}");
        }
    }
    #[test]
    fn deployment_profiles_reject_unsafe_or_unowned_artifacts() {
        let (mut original, ids, plan) = source();
        original.validate(&ids, &plan).unwrap();
        for defect in [
            "path",
            "depth",
            "duplicate",
            "directory",
            "credential",
            "sbom",
            "schema",
            "surface",
            "architecture",
        ] {
            let mut registry = original.clone();
            let profile = &mut registry.deployment_profiles[0];
            match defect {
                "path" => profile.artifacts[0].path = "../outside".into(),
                "depth" => profile.artifacts[0].path = "a/".repeat(16) + "file",
                "duplicate" => profile.artifacts.push(profile.artifacts[0].clone()),
                "directory" => {
                    let mut artifact = profile.artifacts[0].clone();
                    artifact.path.push_str("/child");
                    profile.artifacts.push(artifact);
                }
                "credential" => profile.required_credentials.push("token=value".into()),
                "sbom" => profile
                    .artifacts
                    .retain(|artifact| artifact.path != "package.sbom.json"),
                "schema" => profile.artifacts[0].schema_pointer = Some("/schema".into()),
                "surface" => profile.integration_surfaces.push("unregistered".into()),
                _ => profile.architecture = "any".into(),
            }
            assert!(registry.validate(&ids, &plan).is_err(), "{defect}");
        }
    }
}
