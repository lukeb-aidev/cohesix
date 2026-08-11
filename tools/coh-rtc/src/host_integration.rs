// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compile and validate the Milestone 26e host-integration dependency graph.
// Author: Lukas Bower

//! Compiler-owned host-integration, use-case, and playbook dependency truth.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_SCHEMA: &str = "host-integration-dependency-source/v1";
const GRAPH_SCHEMA: &str = "host-integration-dependency/v1";
const IMPLEMENTATION_INVENTORY_SCHEMA: &str = "cohesix-implementation-surface-inventory/v1";
const MAX_DEPENDENCIES: usize = 64;
const MAX_LIST_ITEMS: usize = 128;
const REQUIRED_TARGET_ROWS: [&str; 3] = ["gpu-receipt-path", "peft-receipt-path", "worker-control"];

/// Result paths emitted by [`compile_graph`].
#[derive(Debug, Clone)]
pub struct HostIntegrationOutput {
    /// Generated dependency graph.
    pub graph: PathBuf,
    /// Generated Markdown support table.
    pub doc_snippet: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    schema: String,
    milestone: String,
    graph_id: String,
    host_profiles: Vec<HostProfile>,
    advertised_packages: Vec<String>,
    advertised_documents: Vec<String>,
    dependencies: Vec<Dependency>,
    use_cases: Vec<UseCase>,
    playbooks: Vec<Playbook>,
    surface_bindings: Vec<SurfaceBinding>,
    lanes: Vec<Lane>,
    conformance_vectors: Vec<ConformanceVector>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HostProfile {
    id: String,
    os: String,
    architectures: Vec<String>,
    allowed_modes: Vec<ObservedMode>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum ObservedMode {
    Unknown,
    Missing,
    Disabled,
    Fixture,
    Mock,
    DryRun,
    Live,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Obligation {
    RoleRequired,
    ReleaseRequired,
    UseCaseRequired,
    Optional,
    Future,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DependencyClass {
    TargetRuntime,
    HostProjection,
    ExternalProvider,
    Packaging,
    FutureProvider,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    id: String,
    owner: String,
    owning_milestone: String,
    class: DependencyClass,
    obligation: Obligation,
    host_profiles: Vec<String>,
    worker_roles: Vec<String>,
    use_cases: Vec<String>,
    playbooks: Vec<String>,
    namespace_paths: Vec<String>,
    actions: Vec<String>,
    schemas: Vec<String>,
    dependencies: Vec<String>,
    required_modes: Vec<ObservedMode>,
    allowed_modes: Vec<ObservedMode>,
    unavailable_owner: String,
    auth_refs: Vec<String>,
    timeout_ms: u32,
    retry_limit: u8,
    cancel: String,
    idempotency: String,
    fencing: String,
    readiness: String,
    degraded: String,
    package_requirements: Vec<String>,
    artifact_requirements: Vec<String>,
    evidence_lane: String,
    mandatory_target_session: bool,
    advertised_terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UseCase {
    id: String,
    title: String,
    dependencies: Vec<String>,
    promotion: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Playbook {
    id: String,
    use_case: String,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceBinding {
    path: String,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Lane {
    id: String,
    mode: ObservedMode,
    dependencies: Vec<String>,
    target: Option<String>,
    expected_result: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConformanceVector {
    id: String,
    mutation: String,
    expected_error: String,
}

#[derive(Debug, Clone, Serialize)]
struct Graph {
    schema: &'static str,
    meta: GraphMeta,
    host_profiles: Vec<HostProfile>,
    dependencies: Vec<Dependency>,
    use_cases: Vec<UseCase>,
    playbooks: Vec<Playbook>,
    advertised_surfaces: Vec<AdvertisedSurface>,
    lanes: Vec<Lane>,
    conformance_vectors: Vec<ConformanceVector>,
}

#[derive(Debug, Clone, Serialize)]
struct GraphMeta {
    author: &'static str,
    purpose: &'static str,
    milestone: String,
    graph_id: String,
    source_sha256: String,
    resolved_manifest_sha256: String,
    implementation_surface_inventory_sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AdvertisedSurface {
    id: String,
    kind: String,
    path: String,
    implementation_class: String,
    production_reachable: bool,
    dependencies: Vec<String>,
}

/// Compile a strict graph from the human-authored matrix and existing generated
/// manifest/surface truth.
pub fn compile_graph(
    source_path: &Path,
    resolved_manifest_path: &Path,
    implementation_inventory_path: &Path,
    build_plan_path: &Path,
    output_path: &Path,
    doc_snippet_path: &Path,
) -> Result<HostIntegrationOutput> {
    let source_bytes = fs::read(source_path)
        .with_context(|| format!("failed to read {}", source_path.display()))?;
    let source_text = std::str::from_utf8(&source_bytes)
        .with_context(|| format!("{} is not UTF-8", source_path.display()))?;
    let mut source: Source = toml::from_str(source_text)
        .with_context(|| format!("failed to parse {}", source_path.display()))?;
    let manifest_bytes = fs::read(resolved_manifest_path)
        .with_context(|| format!("failed to read {}", resolved_manifest_path.display()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("failed to parse {}", resolved_manifest_path.display()))?;
    let inventory_bytes = fs::read(implementation_inventory_path)
        .with_context(|| format!("failed to read {}", implementation_inventory_path.display()))?;
    let inventory: Value = serde_json::from_slice(&inventory_bytes).with_context(|| {
        format!(
            "failed to parse {}",
            implementation_inventory_path.display()
        )
    })?;
    let build_plan = fs::read_to_string(build_plan_path)
        .with_context(|| format!("failed to read {}", build_plan_path.display()))?;

    validate_and_sort(&mut source, &manifest, &inventory, &build_plan)?;
    let advertised_surfaces = build_advertised_surfaces(&source, &inventory)?;
    let graph = Graph {
        schema: GRAPH_SCHEMA,
        meta: GraphMeta {
            author: "Lukas Bower",
            purpose:
                "Bind advertised host surfaces and use cases to exact integration obligations.",
            milestone: source.milestone,
            graph_id: source.graph_id,
            source_sha256: sha256(&source_bytes),
            resolved_manifest_sha256: sha256(&manifest_bytes),
            implementation_surface_inventory_sha256: sha256(&inventory_bytes),
        },
        host_profiles: source.host_profiles,
        dependencies: source.dependencies,
        use_cases: source.use_cases,
        playbooks: source.playbooks,
        advertised_surfaces,
        lanes: source.lanes,
        conformance_vectors: source.conformance_vectors,
    };
    let mut graph_bytes = serde_json::to_vec_pretty(&graph)?;
    graph_bytes.push(b'\n');
    write_bytes(output_path, &graph_bytes)?;
    write_bytes(doc_snippet_path, render_doc(&graph).as_bytes())?;
    Ok(HostIntegrationOutput {
        graph: output_path.to_path_buf(),
        doc_snippet: doc_snippet_path.to_path_buf(),
    })
}

fn validate_and_sort(
    source: &mut Source,
    manifest: &Value,
    inventory: &Value,
    build_plan: &str,
) -> Result<()> {
    if source.schema != SOURCE_SCHEMA {
        bail!("host integration source schema must be {SOURCE_SCHEMA}");
    }
    if inventory.get("schema").and_then(Value::as_str) != Some(IMPLEMENTATION_INVENTORY_SCHEMA) {
        bail!("implementation surface inventory has wrong schema");
    }
    require_id(&source.graph_id, "graph id")?;
    require_text(&source.milestone, "milestone")?;
    sort_unique_by(
        &mut source.host_profiles,
        |value| value.id.as_str(),
        "host profiles",
    )?;
    sort_unique_strings(&mut source.advertised_packages, "advertised packages")?;
    sort_unique_strings(&mut source.advertised_documents, "advertised documents")?;
    sort_unique_by(
        &mut source.dependencies,
        |value| value.id.as_str(),
        "dependencies",
    )?;
    sort_unique_by(
        &mut source.use_cases,
        |value| value.id.as_str(),
        "use cases",
    )?;
    sort_unique_by(
        &mut source.playbooks,
        |value| value.id.as_str(),
        "playbooks",
    )?;
    sort_unique_by(
        &mut source.surface_bindings,
        |value| value.path.as_str(),
        "surface bindings",
    )?;
    sort_unique_by(&mut source.lanes, |value| value.id.as_str(), "lanes")?;
    sort_unique_by(
        &mut source.conformance_vectors,
        |value| value.id.as_str(),
        "conformance vectors",
    )?;

    let profiles: BTreeSet<_> = source
        .host_profiles
        .iter()
        .map(|value| value.id.clone())
        .collect();
    for profile in &mut source.host_profiles {
        require_id(&profile.id, "host profile")?;
        require_text(&profile.os, "host profile OS")?;
        sort_unique_strings(&mut profile.architectures, "host profile architectures")?;
        sort_unique(&mut profile.allowed_modes, "host profile modes")?;
        if profile.allowed_modes.is_empty() {
            bail!("host profile {} has no allowed modes", profile.id);
        }
    }

    let dependency_ids: BTreeSet<_> = source
        .dependencies
        .iter()
        .map(|value| value.id.clone())
        .collect();
    let use_case_ids: BTreeSet<_> = source
        .use_cases
        .iter()
        .map(|value| value.id.clone())
        .collect();
    let playbook_ids: BTreeSet<_> = source
        .playbooks
        .iter()
        .map(|value| value.id.clone())
        .collect();
    if source.use_cases.len() != 6 {
        bail!("host integration graph must classify exactly six docs/USE_CASES.md scenarios");
    }
    if source.playbooks.len() != 9 {
        bail!("host integration graph must classify exactly nine built-in playbooks");
    }

    let (manifest_paths, manifest_actions, manifest_schemas) = manifest_references(manifest);
    for dependency in &mut source.dependencies {
        validate_dependency(
            dependency,
            &profiles,
            &dependency_ids,
            &use_case_ids,
            &playbook_ids,
            &manifest_paths,
            &manifest_actions,
            &manifest_schemas,
            build_plan,
        )?;
    }
    validate_acyclic(&source.dependencies)?;
    for required in REQUIRED_TARGET_ROWS {
        let row = source
            .dependencies
            .iter()
            .find(|value| value.id == required)
            .ok_or_else(|| anyhow!("missing mandatory live target-session row {required}"))?;
        if row.obligation != Obligation::RoleRequired
            || row.class != DependencyClass::TargetRuntime
            || !row.mandatory_target_session
            || row.required_modes != vec![ObservedMode::Live]
        {
            bail!("mandatory row {required} is not an exact role-required live target session");
        }
    }

    for use_case in &mut source.use_cases {
        require_id(&use_case.id, "use-case id")?;
        require_text(&use_case.title, "use-case title")?;
        require_text(&use_case.promotion, "use-case promotion")?;
        sort_unique_strings(&mut use_case.dependencies, "use-case dependencies")?;
        require_known(
            &use_case.dependencies,
            &dependency_ids,
            "use-case dependency",
        )?;
        if use_case.dependencies.is_empty() {
            bail!("use case {} has no dependencies", use_case.id);
        }
    }
    for playbook in &mut source.playbooks {
        require_id(&playbook.id, "playbook id")?;
        if !use_case_ids.contains(&playbook.use_case) {
            bail!(
                "playbook {} references unknown use case {}",
                playbook.id,
                playbook.use_case
            );
        }
        sort_unique_strings(&mut playbook.dependencies, "playbook dependencies")?;
        require_known(
            &playbook.dependencies,
            &dependency_ids,
            "playbook dependency",
        )?;
        if playbook.dependencies.is_empty() {
            bail!("playbook {} has no dependencies", playbook.id);
        }
    }

    let advertised_paths: BTreeSet<_> = source
        .advertised_packages
        .iter()
        .chain(source.advertised_documents.iter())
        .map(String::as_str)
        .collect();
    let binding_paths: BTreeSet<_> = source
        .surface_bindings
        .iter()
        .map(|value| value.path.as_str())
        .collect();
    if advertised_paths != binding_paths {
        bail!("every advertised package/document must have exactly one surface binding");
    }
    for binding in &mut source.surface_bindings {
        sort_unique_strings(&mut binding.dependencies, "surface dependencies")?;
        require_known(&binding.dependencies, &dependency_ids, "surface dependency")?;
        if binding.dependencies.is_empty() {
            bail!("advertised surface {} has no dependency", binding.path);
        }
    }
    for lane in &mut source.lanes {
        require_id(&lane.id, "lane id")?;
        require_text(&lane.expected_result, "lane expected result")?;
        sort_unique_strings(&mut lane.dependencies, "lane dependencies")?;
        require_known(&lane.dependencies, &dependency_ids, "lane dependency")?;
        match lane.target.as_deref() {
            None => {}
            Some("qemu" | "pi4") if lane.mode == ObservedMode::Live => {}
            Some(_) => bail!("lane {} has invalid target/mode", lane.id),
        }
    }
    for vector in &source.conformance_vectors {
        require_id(&vector.id, "conformance vector")?;
        require_text(&vector.mutation, "conformance mutation")?;
        require_text(&vector.expected_error, "conformance expected error")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_dependency(
    row: &mut Dependency,
    profiles: &BTreeSet<String>,
    dependency_ids: &BTreeSet<String>,
    use_case_ids: &BTreeSet<String>,
    playbook_ids: &BTreeSet<String>,
    manifest_paths: &BTreeSet<String>,
    manifest_actions: &BTreeSet<String>,
    manifest_schemas: &BTreeSet<String>,
    build_plan: &str,
) -> Result<()> {
    require_id(&row.id, "dependency id")?;
    require_text(&row.owner, "dependency owner")?;
    require_id(&row.owning_milestone, "owning milestone")?;
    if !build_plan.lines().any(|line| {
        line.strip_prefix("Title/ID: ")
            .is_some_and(|value| value.trim() == row.owning_milestone)
    }) {
        bail!(
            "dependency {} has unknown owning milestone {}",
            row.id,
            row.owning_milestone
        );
    }
    for value in [
        &row.unavailable_owner,
        &row.cancel,
        &row.idempotency,
        &row.fencing,
        &row.readiness,
        &row.degraded,
        &row.evidence_lane,
    ] {
        require_text(value, "dependency posture")?;
    }
    if row.timeout_ms == 0 || row.timeout_ms > 300_000 || row.retry_limit > 16 {
        bail!("dependency {} has unbounded timeout/retry posture", row.id);
    }
    sort_unique_strings(&mut row.host_profiles, "dependency host profiles")?;
    require_known(&row.host_profiles, profiles, "host profile")?;
    sort_unique_strings(&mut row.worker_roles, "Worker roles")?;
    for role in &row.worker_roles {
        if !matches!(
            role.as_str(),
            "worker-heartbeat" | "worker-gpu" | "worker-lora"
        ) {
            bail!(
                "dependency {} references non-executable Worker role {role}",
                row.id
            );
        }
    }
    sort_unique_strings(&mut row.use_cases, "dependency use cases")?;
    require_known(&row.use_cases, use_case_ids, "dependency use case")?;
    sort_unique_strings(&mut row.playbooks, "dependency playbooks")?;
    require_known(&row.playbooks, playbook_ids, "dependency playbook")?;
    sort_unique_strings(&mut row.namespace_paths, "namespace paths")?;
    for path in &row.namespace_paths {
        if !manifest_paths.contains(path) {
            bail!(
                "dependency {} references unknown generated namespace path {path}",
                row.id
            );
        }
    }
    sort_unique_strings(&mut row.actions, "actions")?;
    for action in &row.actions {
        if !manifest_actions.contains(action) {
            bail!(
                "dependency {} references unknown generated host-ticket action {action}",
                row.id
            );
        }
    }
    sort_unique_strings(&mut row.schemas, "schemas")?;
    for schema in &row.schemas {
        if !manifest_schemas.contains(schema) {
            bail!(
                "dependency {} references unknown generated schema {schema}",
                row.id
            );
        }
    }
    sort_unique_strings(&mut row.dependencies, "transitive dependencies")?;
    require_known(&row.dependencies, dependency_ids, "transitive dependency")?;
    if row.dependencies.iter().any(|value| value == &row.id) {
        bail!("dependency {} depends on itself", row.id);
    }
    sort_unique(&mut row.required_modes, "required modes")?;
    sort_unique(&mut row.allowed_modes, "allowed modes")?;
    if row.required_modes.is_empty()
        || row
            .required_modes
            .iter()
            .any(|value| !row.allowed_modes.contains(value))
    {
        bail!("dependency {} has invalid required/allowed modes", row.id);
    }
    sort_unique_strings(&mut row.auth_refs, "auth refs")?;
    for auth_ref in &row.auth_refs {
        if !auth_ref.starts_with("env:") && !auth_ref.starts_with("file-ref:") {
            bail!("dependency {} auth refs must be indirections", row.id);
        }
    }
    sort_unique_strings(&mut row.package_requirements, "package requirements")?;
    sort_unique_strings(&mut row.artifact_requirements, "artifact requirements")?;
    sort_unique_strings(&mut row.advertised_terms, "advertised terms")?;
    match row.obligation {
        Obligation::RoleRequired => {
            if row.worker_roles.is_empty()
                || row.required_modes != vec![ObservedMode::Live]
                || !row.mandatory_target_session
                || row.evidence_lane != "target-session"
            {
                bail!(
                    "role-required dependency {} lacks a live target lane",
                    row.id
                );
            }
        }
        Obligation::ReleaseRequired => {
            if row.package_requirements.is_empty() || row.artifact_requirements.is_empty() {
                bail!(
                    "release-required dependency {} lacks package/evidence rules",
                    row.id
                );
            }
        }
        Obligation::UseCaseRequired => {
            if row.use_cases.is_empty() {
                bail!("use-case-required dependency {} names no use case", row.id);
            }
        }
        Obligation::Optional => {}
        Obligation::Future => {
            if row.allowed_modes.contains(&ObservedMode::Live)
                || row.required_modes.contains(&ObservedMode::Live)
                || row.mandatory_target_session
                || row.class != DependencyClass::FutureProvider
                || row.owning_milestone.starts_with("m26e-")
            {
                bail!("future dependency {} is selectable as current/live", row.id);
            }
        }
    }
    if row.class == DependencyClass::ExternalProvider && row.mandatory_target_session {
        bail!(
            "external provider {} cannot use target-session proof",
            row.id
        );
    }
    Ok(())
}

fn manifest_references(manifest: &Value) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let mut paths = BTreeSet::new();
    let mut actions = BTreeSet::new();
    let mut schemas = BTreeSet::new();
    collect_manifest_refs(manifest, None, &mut paths, &mut actions, &mut schemas);
    (paths, actions, schemas)
}

fn collect_manifest_refs(
    value: &Value,
    key: Option<&str>,
    paths: &mut BTreeSet<String>,
    actions: &mut BTreeSet<String>,
    schemas: &mut BTreeSet<String>,
) {
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                collect_manifest_refs(child, Some(child_key), paths, actions, schemas);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_manifest_refs(child, key, paths, actions, schemas);
            }
        }
        Value::String(text) if text.starts_with('/') => {
            paths.insert(text.clone());
        }
        Value::String(text) => match key {
            Some(name) if name.contains("schema") => {
                schemas.insert(text.clone());
            }
            Some("action_allowlist") => {
                actions.insert(text.clone());
            }
            _ => {}
        },
        _ => {}
    }
}

fn build_advertised_surfaces(source: &Source, inventory: &Value) -> Result<Vec<AdvertisedSurface>> {
    let bindings: BTreeMap<_, _> = source
        .surface_bindings
        .iter()
        .map(|value| (value.path.as_str(), value.dependencies.clone()))
        .collect();
    let mut records = Vec::new();
    let packages = inventory
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("implementation inventory packages missing"))?;
    for package_path in &source.advertised_packages {
        let package = packages
            .iter()
            .find(|value| value.get("path").and_then(Value::as_str) == Some(package_path))
            .ok_or_else(|| {
                anyhow!("advertised package {package_path} missing from implementation inventory")
            })?;
        let dependencies = bindings
            .get(package_path.as_str())
            .ok_or_else(|| anyhow!("advertised package {package_path} has no binding"))?;
        records.push(surface_from_value(package, dependencies)?);
        for target in package
            .get("targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            records.push(surface_from_value(target, dependencies)?);
        }
    }
    let tracked = inventory
        .get("tracked_surfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("implementation inventory tracked surfaces missing"))?;
    for document_path in &source.advertised_documents {
        let document = tracked
            .iter()
            .find(|value| value.get("path").and_then(Value::as_str) == Some(document_path))
            .ok_or_else(|| {
                anyhow!("advertised document {document_path} missing from implementation inventory")
            })?;
        records.push(surface_from_value(
            document,
            bindings
                .get(document_path.as_str())
                .ok_or_else(|| anyhow!("advertised document {document_path} has no binding"))?,
        )?);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let ids: BTreeSet<_> = records.iter().map(|value| value.id.as_str()).collect();
    if ids.len() != records.len() {
        bail!("advertised implementation surface ids are not unique");
    }
    Ok(records)
}

fn surface_from_value(value: &Value, dependencies: &[String]) -> Result<AdvertisedSurface> {
    let get_string = |field| {
        value
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("implementation surface missing {field}"))
    };
    Ok(AdvertisedSurface {
        id: get_string("id")?,
        kind: get_string("kind")?,
        path: get_string("path")?,
        implementation_class: get_string("implementation_class")?,
        production_reachable: value
            .get("production_reachable")
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow!("implementation surface missing production_reachable"))?,
        dependencies: dependencies.to_vec(),
    })
}

fn validate_acyclic(dependencies: &[Dependency]) -> Result<()> {
    let map: BTreeMap<_, _> = dependencies
        .iter()
        .map(|value| (value.id.as_str(), value.dependencies.as_slice()))
        .collect();
    fn visit<'a>(
        id: &'a str,
        map: &BTreeMap<&'a str, &'a [String]>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            bail!("circular host-integration dependency at {id}");
        }
        for dependency in map.get(id).copied().unwrap_or_default() {
            visit(dependency, map, visiting, visited)?;
        }
        visiting.remove(id);
        visited.insert(id);
        Ok(())
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in map.keys() {
        visit(id, &map, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn render_doc(graph: &Graph) -> String {
    let mut output = String::from(
        "<!-- Copyright 2026 Lukas Bower -->\n<!-- SPDX-License-Identifier: Apache-2.0 -->\n<!-- Purpose: Project generated host-integration dependency and support truth. -->\n<!-- Author: Lukas Bower -->\n\n# Generated Host-Integration Dependencies\n\n",
    );
    output.push_str("This table is generated from `configs/host_integration_acceptance.toml`. Worker execution, provider availability, package presence, mock or dry-run success, and use-case promotion are independent states.\n\n");
    output.push_str("| Dependency | Obligation | Required mode | Worker roles | Owner milestone |\n| --- | --- | --- | --- | --- |\n");
    for row in &graph.dependencies {
        let roles = if row.worker_roles.is_empty() {
            "none".to_owned()
        } else {
            row.worker_roles.join(", ")
        };
        output.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` |\n",
            row.id,
            obligation_label(row.obligation),
            row.required_modes
                .iter()
                .map(|value| observed_mode_label(*value))
                .collect::<Vec<_>>()
                .join(", "),
            roles,
            row.owning_milestone,
        ));
    }
    output
}

fn obligation_label(value: Obligation) -> &'static str {
    match value {
        Obligation::RoleRequired => "role_required",
        Obligation::ReleaseRequired => "release_required",
        Obligation::UseCaseRequired => "use_case_required",
        Obligation::Optional => "optional",
        Obligation::Future => "future",
    }
}

fn observed_mode_label(value: ObservedMode) -> &'static str {
    match value {
        ObservedMode::Unknown => "unknown",
        ObservedMode::Missing => "missing",
        ObservedMode::Disabled => "disabled",
        ObservedMode::Fixture => "fixture",
        ObservedMode::Mock => "mock",
        ObservedMode::DryRun => "dry-run",
        ObservedMode::Live => "live",
    }
}

fn require_known<T: AsRef<str>>(values: &[String], known: &BTreeSet<T>, field: &str) -> Result<()> {
    for value in values {
        if !known
            .iter()
            .any(|known_value| known_value.as_ref() == value)
        {
            bail!("unknown {field}: {value}");
        }
    }
    Ok(())
}

fn require_id(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("{field} must be a bounded lowercase kebab-case identifier");
    }
    Ok(())
}

fn require_text(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 512 {
        bail!("{field} must be non-empty and bounded");
    }
    Ok(())
}

fn sort_unique_strings(values: &mut [String], field: &str) -> Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        bail!("{field} exceeds bounded list size");
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("{field} contains duplicates");
    }
    Ok(())
}

fn sort_unique<T: Ord>(values: &mut [T], field: &str) -> Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        bail!("{field} exceeds bounded list size");
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("{field} contains duplicates");
    }
    Ok(())
}

fn sort_unique_by<T, F>(values: &mut [T], key: F, field: &str) -> Result<()>
where
    F: Fn(&T) -> &str,
{
    if values.len() > MAX_DEPENDENCIES {
        bail!("{field} exceeds bounded list size");
    }
    values.sort_by(|left, right| key(left).cmp(key(right)));
    if values.windows(2).any(|pair| key(&pair[0]) == key(&pair[1])) {
        bail!("{field} contains duplicates");
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_modified_source(rewrite: impl FnOnce(String) -> String) -> anyhow::Error {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("coh-rtc must live under <repo>/tools");
        let temp = tempfile::tempdir().expect("tempdir");
        let source = fs::read_to_string(repo_root.join("configs/host_integration_acceptance.toml"))
            .expect("read source");
        let source_path = temp.path().join("host_integration_acceptance.toml");
        fs::write(&source_path, rewrite(source)).expect("write modified source");
        compile_graph(
            &source_path,
            &repo_root.join("configs/generated/root_task_resolved.json"),
            &repo_root.join("configs/generated/implementation_surface_inventory.json"),
            &repo_root.join("docs/BUILD_PLAN.md"),
            &temp.path().join("host_integration_dependency.json"),
            &temp.path().join("host_integration_dependency.md"),
        )
        .expect_err("modified source must fail")
    }

    #[test]
    fn host_integration_manifest_references_are_compiler_owned() {
        let manifest: Value = serde_json::json!({
            "client_paths": {"queen_ctl": "/queen/ctl"},
            "ecosystem": {"host": {"tickets": {
                "request_schema": "host-ticket/v1",
                "action_allowlist": ["gpu.lease.grant"]
            }}}
        });
        let (paths, actions, schemas) = manifest_references(&manifest);
        assert!(paths.contains("/queen/ctl"));
        assert!(actions.contains("gpu.lease.grant"));
        assert!(schemas.contains("host-ticket/v1"));
    }

    #[test]
    fn host_integration_cycle_is_rejected() {
        let make = |id: &str, next: &str| Dependency {
            id: id.to_owned(),
            owner: "owner".to_owned(),
            owning_milestone: "m26e-host-integration-dependency-contract".to_owned(),
            class: DependencyClass::HostProjection,
            obligation: Obligation::Optional,
            host_profiles: Vec::new(),
            worker_roles: Vec::new(),
            use_cases: Vec::new(),
            playbooks: Vec::new(),
            namespace_paths: Vec::new(),
            actions: Vec::new(),
            schemas: Vec::new(),
            dependencies: vec![next.to_owned()],
            required_modes: vec![ObservedMode::Missing],
            allowed_modes: vec![ObservedMode::Missing],
            unavailable_owner: "owner".to_owned(),
            auth_refs: Vec::new(),
            timeout_ms: 1,
            retry_limit: 0,
            cancel: "bounded".to_owned(),
            idempotency: "none".to_owned(),
            fencing: "none".to_owned(),
            readiness: "separate".to_owned(),
            degraded: "explicit".to_owned(),
            package_requirements: Vec::new(),
            artifact_requirements: Vec::new(),
            evidence_lane: "matrix".to_owned(),
            mandatory_target_session: false,
            advertised_terms: Vec::new(),
        };
        let error = validate_acyclic(&[make("one", "two"), make("two", "one")])
            .expect_err("cycle must fail");
        assert!(error.to_string().contains("circular"));
    }

    #[test]
    fn repository_host_integration_contract_compiles() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("coh-rtc must live under <repo>/tools");
        let temp = tempfile::tempdir().expect("tempdir");
        let result = compile_graph(
            &repo_root.join("configs/host_integration_acceptance.toml"),
            &repo_root.join("configs/generated/root_task_resolved.json"),
            &repo_root.join("configs/generated/implementation_surface_inventory.json"),
            &repo_root.join("docs/BUILD_PLAN.md"),
            &temp.path().join("host_integration_dependency.json"),
            &temp.path().join("host_integration_dependency.md"),
        );
        result.expect("repository host-integration source must compile");
    }

    #[test]
    fn host_integration_duplicate_id_is_rejected() {
        let error = compile_modified_source(|source| {
            source.replacen("id = \"peft-receipt-path\"", "id = \"gpu-receipt-path\"", 1)
        });
        assert!(error
            .to_string()
            .contains("dependencies contains duplicates"));
    }

    #[test]
    fn host_integration_unknown_generated_reference_is_rejected() {
        let error = compile_modified_source(|source| {
            source.replacen("\"gpu.lease.grant\"", "\"gpu.lease.unknown\"", 1)
        });
        assert!(error
            .to_string()
            .contains("unknown generated host-ticket action"));
    }

    #[test]
    fn host_integration_future_live_mode_is_rejected() {
        let error = compile_modified_source(|source| {
            source.replacen(
                "allowed_modes = [\"disabled\", \"missing\"]",
                "allowed_modes = [\"disabled\", \"live\", \"missing\"]",
                1,
            )
        });
        assert!(error
            .to_string()
            .contains("future dependency a2a-gateway is selectable as current/live"));
    }
}
