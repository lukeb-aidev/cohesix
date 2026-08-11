// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compile and validate the Milestone 26e implementation-surface inventory.
// Author: Lukas Bower

//! Compiler-owned implementation-surface classification and drift validation.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INVENTORY_SCHEMA: &str = "cohesix-implementation-surface-inventory/v1";
const SOURCE_SCHEMA: &str = "cohesix-implementation-surface-source/v1";
const ALLOWED_CLASSES: &[&str] = &[
    "production_live",
    "fixture",
    "host_model",
    "diagnostic",
    "contract",
    "not_enabled",
    "deferred",
    "retired",
    "model_only",
];
const CLAIM_INELIGIBLE_CLASSES: &[&str] = &[
    "fixture",
    "host_model",
    "diagnostic",
    "contract",
    "not_enabled",
    "deferred",
    "retired",
    "model_only",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventorySource {
    schema: String,
    milestone: String,
    fixture_features: Vec<String>,
    diagnostic_features: Vec<String>,
    forbidden_runtime_features: Vec<String>,
    packages: Vec<PackageSource>,
    #[serde(default)]
    surfaces: Vec<SurfaceSource>,
    #[serde(default)]
    tracked_rules: Vec<TrackedRule>,
    runtime_closures: Vec<RuntimeClosureSource>,
    release: ReleaseSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSource {
    path: String,
    class: String,
    owner: String,
    production_reachable: bool,
    selection_source: String,
    package_disposition: String,
    evidence_requirement: String,
    current_observed_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceSource {
    id: String,
    kind: String,
    path: String,
    class: String,
    owner: String,
    production_reachable: bool,
    selection_source: String,
    package_disposition: String,
    evidence_requirement: String,
    current_observed_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackedRule {
    id: String,
    #[serde(default)]
    exact: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    suffix: Option<String>,
    #[serde(default)]
    exclude_exact: Vec<String>,
    #[serde(default)]
    exclude_prefix: Vec<String>,
    class: String,
    owner: String,
    production_reachable: bool,
    selection_source: String,
    package_disposition: String,
    evidence_requirement: String,
    current_observed_mode: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeClosureSource {
    name: String,
    target: String,
    package: String,
    features: Vec<String>,
    selected_entrypoints: Vec<String>,
    expected_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleaseSource {
    schema: String,
    version: String,
    host_tools: Vec<String>,
    target_images: Vec<String>,
    generated_configs: Vec<String>,
    public_documents: Vec<String>,
    host_assets: Vec<String>,
    operator_scripts: Vec<String>,
    python_artifacts: Vec<String>,
    trace_fixtures: Vec<String>,
    transcript_fixtures: Vec<String>,
    ui_assets: Vec<String>,
    support_files: Vec<String>,
    versioned_migrations: Vec<String>,
    generated_bundle_files: Vec<String>,
    forbidden_paths: Vec<String>,
    #[serde(default)]
    expected_bundle_files: Vec<String>,
    #[serde(default)]
    asset_records: Vec<SurfaceRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct Inventory {
    schema: &'static str,
    source_sha256: String,
    milestone: String,
    forbidden_runtime_features: Vec<String>,
    packages: Vec<PackageRecord>,
    surfaces: Vec<SurfaceRecord>,
    tracked_surfaces: Vec<SurfaceRecord>,
    runtime_closures: Vec<RuntimeClosureSource>,
    release: ReleaseSource,
}

#[derive(Debug, Clone, Serialize)]
struct PackageRecord {
    id: String,
    kind: &'static str,
    name: String,
    path: String,
    #[serde(flatten)]
    classification: Classification,
    targets: Vec<SurfaceRecord>,
    features: Vec<SurfaceRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SurfaceRecord {
    id: String,
    kind: String,
    path: String,
    #[serde(flatten)]
    classification: Classification,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Classification {
    implementation_class: String,
    owner: String,
    milestone: String,
    production_reachable: bool,
    selection_source: String,
    package_disposition: String,
    evidence_requirement: String,
    current_observed_mode: String,
    evidence_eligible: bool,
}

#[derive(Debug)]
struct CargoPackage {
    name: String,
    path: String,
    targets: Vec<(String, String)>,
    features: BTreeMap<String, Vec<String>>,
}

/// Compile the human-authored classification source into deterministic JSON.
pub fn compile_inventory(source_path: &Path, output_path: &Path) -> Result<PathBuf> {
    let source_path = source_path
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", source_path.display()))?;
    let source_bytes = fs::read(&source_path)
        .with_context(|| format!("failed to read {}", source_path.display()))?;
    let source_text = std::str::from_utf8(&source_bytes)
        .with_context(|| format!("{} is not UTF-8", source_path.display()))?;
    let source: InventorySource = toml::from_str(source_text)
        .with_context(|| format!("failed to parse {}", source_path.display()))?;
    if source.schema != SOURCE_SCHEMA {
        bail!(
            "implementation surface source schema must be {SOURCE_SCHEMA}, got {}",
            source.schema
        );
    }
    require_nonempty("milestone", &source.milestone)?;
    let repo_root = source_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("surface source must live under <repo>/configs"))?;
    let cargo_packages = load_workspace_packages(repo_root)?;
    let package_sources = validate_package_sources(&source, &cargo_packages)?;
    validate_runtime_closures(&source, &cargo_packages)?;
    validate_release(&source.release)?;

    let mut packages = Vec::with_capacity(cargo_packages.len());
    for cargo in &cargo_packages {
        let declared = package_sources
            .get(cargo.path.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing classification for {}", cargo.path))?;
        let package_classification = classification_from_package(declared, &source.milestone)?;
        let mut targets = Vec::new();
        for (kind, name) in &cargo.targets {
            let class = package_classification.clone();
            targets.push(SurfaceRecord {
                id: format!("cargo-target:{}:{kind}:{name}", cargo.name),
                kind: "cargo_target".to_owned(),
                path: format!("{}#{kind}:{name}", cargo.path),
                classification: class,
            });
        }
        let mut features = Vec::new();
        for name in cargo.features.keys() {
            let (implementation_class, observed_mode, eligible) =
                if source.fixture_features.iter().any(|value| value == name) {
                    ("fixture", "explicit_fixture_feature", false)
                } else if source.diagnostic_features.iter().any(|value| value == name) {
                    ("diagnostic", "explicit_diagnostic_feature", false)
                } else {
                    (
                        package_classification.implementation_class.as_str(),
                        package_classification.current_observed_mode.as_str(),
                        package_classification.evidence_eligible,
                    )
                };
            features.push(SurfaceRecord {
                id: format!("cargo-feature:{}:{name}", cargo.name),
                kind: "cargo_feature".to_owned(),
                path: format!("{}#feature:{name}", cargo.path),
                classification: Classification {
                    implementation_class: implementation_class.to_owned(),
                    owner: declared.owner.clone(),
                    milestone: source.milestone.clone(),
                    production_reachable: declared.production_reachable
                        && implementation_class != "fixture"
                        && implementation_class != "diagnostic",
                    selection_source: format!(
                        "{}; Cargo feature `{name}`",
                        declared.selection_source
                    ),
                    package_disposition: "feature_only".to_owned(),
                    evidence_requirement: declared.evidence_requirement.clone(),
                    current_observed_mode: observed_mode.to_owned(),
                    evidence_eligible: eligible,
                },
            });
        }
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        features.sort_by(|a, b| a.id.cmp(&b.id));
        packages.push(PackageRecord {
            id: format!("workspace:{}", cargo.name),
            kind: "workspace_member",
            name: cargo.name.clone(),
            path: cargo.path.clone(),
            classification: package_classification,
            targets,
            features,
        });
    }
    packages.sort_by(|a, b| a.id.cmp(&b.id));

    let mut surfaces = source
        .surfaces
        .iter()
        .map(|entry| surface_from_source(entry, &source.milestone))
        .collect::<Result<Vec<_>>>()?;
    surfaces.sort_by(|a, b| a.id.cmp(&b.id));
    let tracked_surfaces = compile_tracked_surfaces(repo_root, &source)?;
    validate_unique_ids(&packages, &surfaces, &tracked_surfaces)?;
    let release = compile_release(
        repo_root,
        source.release.clone(),
        &source.milestone,
        &packages,
        &surfaces,
        &tracked_surfaces,
    )?;

    let inventory = Inventory {
        schema: INVENTORY_SCHEMA,
        source_sha256: hex::encode(Sha256::digest(&source_bytes)),
        milestone: source.milestone,
        forbidden_runtime_features: sorted_unique(source.forbidden_runtime_features),
        packages,
        surfaces,
        tracked_surfaces,
        runtime_closures: sorted_closures(source.runtime_closures),
        release,
    };
    let mut output = serde_json::to_vec_pretty(&inventory)?;
    output.push(b'\n');
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(output_path, output)
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    Ok(output_path.to_path_buf())
}

fn validate_package_sources<'a>(
    source: &'a InventorySource,
    cargo: &[CargoPackage],
) -> Result<BTreeMap<&'a str, &'a PackageSource>> {
    let mut by_path = BTreeMap::new();
    for package in &source.packages {
        validate_classification(
            &package.class,
            &package.owner,
            &package.selection_source,
            &package.package_disposition,
            &package.evidence_requirement,
            &package.current_observed_mode,
            package.production_reachable,
        )?;
        if by_path.insert(package.path.as_str(), package).is_some() {
            bail!("duplicate package classification for {}", package.path);
        }
    }
    let actual = cargo
        .iter()
        .map(|package| package.path.as_str())
        .collect::<BTreeSet<_>>();
    let declared = by_path.keys().copied().collect::<BTreeSet<_>>();
    if actual != declared {
        let missing = actual.difference(&declared).copied().collect::<Vec<_>>();
        let stale = declared.difference(&actual).copied().collect::<Vec<_>>();
        bail!("workspace surface coverage drift: missing={missing:?} stale={stale:?}");
    }
    let model_only = source
        .packages
        .iter()
        .filter(|package| package.class == "model_only")
        .map(|package| package.path.as_str())
        .collect::<Vec<_>>();
    if model_only != ["apps/worker-bus"] {
        bail!("WorkerBus must be the sole model_only package, got {model_only:?}");
    }
    Ok(by_path)
}

fn validate_runtime_closures(source: &InventorySource, cargo: &[CargoPackage]) -> Result<()> {
    let packages = cargo
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let forbidden = source
        .forbidden_runtime_features
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    for closure in &source.runtime_closures {
        if !names.insert(closure.name.as_str()) {
            bail!("duplicate runtime closure {}", closure.name);
        }
        require_nonempty("runtime closure target", &closure.target)?;
        let package = packages.get(closure.package.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "runtime closure {} names unknown package {}",
                closure.name,
                closure.package
            )
        })?;
        for feature in &closure.features {
            if !package.features.contains_key(feature) {
                bail!(
                    "runtime closure {} names unknown {} feature {}",
                    closure.name,
                    closure.package,
                    feature
                );
            }
            let mut expanded = BTreeSet::new();
            expand_feature(feature, &package.features, &mut expanded)?;
            let rejected = expanded
                .intersection(&forbidden)
                .copied()
                .collect::<Vec<_>>();
            if !rejected.is_empty() {
                bail!(
                    "runtime closure {} selects forbidden fixture/diagnostic features {:?}",
                    closure.name,
                    rejected
                );
            }
        }
        if closure.selected_entrypoints.is_empty() || closure.expected_artifacts.is_empty() {
            bail!(
                "runtime closure {} must name entrypoints and artifacts",
                closure.name
            );
        }
    }
    if !names.contains("qemu-aarch64-gicv3-mcs") || !names.contains("pi4-aarch64-mcs") {
        bail!("runtime closures must include QEMU GICv3 MCS and Pi 4 MCS");
    }
    Ok(())
}

fn expand_feature<'a>(
    feature: &'a str,
    features: &'a BTreeMap<String, Vec<String>>,
    expanded: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if !expanded.insert(feature) {
        return Ok(());
    }
    let Some(values) = features.get(feature) else {
        return Ok(());
    };
    for value in values {
        let local = value.strip_prefix("dep:").unwrap_or(value);
        if local.contains('/') || local.contains('?') || !features.contains_key(local) {
            continue;
        }
        expand_feature(local, features, expanded)?;
    }
    Ok(())
}

fn validate_release(release: &ReleaseSource) -> Result<()> {
    if release.schema != "cohesix-runtime-release-manifest/v1" {
        bail!("unsupported release manifest schema {}", release.schema);
    }
    require_nonempty("release version", &release.version)?;
    if !release.expected_bundle_files.is_empty() || !release.asset_records.is_empty() {
        bail!("release expected_bundle_files and asset_records are compiler-owned outputs");
    }
    for (label, values) in [
        ("host_tools", &release.host_tools),
        ("target_images", &release.target_images),
        ("generated_configs", &release.generated_configs),
        ("public_documents", &release.public_documents),
        ("host_assets", &release.host_assets),
        ("operator_scripts", &release.operator_scripts),
        ("python_artifacts", &release.python_artifacts),
        ("trace_fixtures", &release.trace_fixtures),
        ("transcript_fixtures", &release.transcript_fixtures),
        ("ui_assets", &release.ui_assets),
        ("support_files", &release.support_files),
        ("generated_bundle_files", &release.generated_bundle_files),
    ] {
        if values.is_empty() {
            bail!("release {label} must not be empty");
        }
        let unique = values.iter().collect::<BTreeSet<_>>();
        if unique.len() != values.len() {
            bail!("release {label} contains duplicate paths");
        }
    }
    let mut selected = BTreeSet::new();
    for path in release
        .host_tools
        .iter()
        .chain(&release.target_images)
        .chain(&release.generated_configs)
        .chain(&release.public_documents)
        .chain(&release.host_assets)
        .chain(&release.operator_scripts)
        .chain(&release.python_artifacts)
        .chain(&release.trace_fixtures)
        .chain(&release.transcript_fixtures)
        .chain(&release.ui_assets)
        .chain(&release.support_files)
        .chain(&release.versioned_migrations)
        .chain(&release.generated_bundle_files)
    {
        if path.starts_with('/')
            || Path::new(path)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            bail!("release path must be relative and traversal-free: {path}");
        }
        if !selected.insert(path) {
            bail!("release path selected by multiple categories: {path}");
        }
    }
    if !release
        .forbidden_paths
        .iter()
        .any(|path| path == "resources/fixtures/cas_signing_key.hex")
    {
        bail!("release must explicitly forbid the fixture CAS signing key");
    }
    Ok(())
}

fn compile_release(
    repo_root: &Path,
    mut release: ReleaseSource,
    milestone: &str,
    packages: &[PackageRecord],
    surfaces: &[SurfaceRecord],
    tracked: &[SurfaceRecord],
) -> Result<ReleaseSource> {
    let expected_notes = format!("releases/RELEASE_NOTES-{}.md", release.version);
    if !release
        .support_files
        .iter()
        .any(|path| path == &expected_notes)
    {
        bail!("release support_files must select the version-bound notes {expected_notes}");
    }

    let mut records = Vec::new();
    let mut expected = BTreeSet::new();
    let release_class = surfaces
        .iter()
        .find(|record| record.id == "release:m26e-runtime")
        .map(|record| record.classification.clone())
        .ok_or_else(|| anyhow::anyhow!("missing release:m26e-runtime classification"))?;

    for source in &release.host_tools {
        let name = Path::new(source)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid host tool path {source}"))?;
        let suffix = format!("#bin:{name}");
        let class = packages
            .iter()
            .flat_map(|package| package.targets.iter())
            .find(|record| record.path.ends_with(&suffix))
            .map(|record| record.classification.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("release host tool has no Cargo binary row: {source}")
            })?;
        push_release_asset(
            &mut records,
            &mut expected,
            source,
            source,
            "host_tool",
            class,
        )?;
    }
    for destination in &release.target_images {
        push_release_asset(
            &mut records,
            &mut expected,
            "selected QEMU build output",
            destination,
            "target_image",
            release_class.clone(),
        )?;
    }
    for source in &release.generated_configs {
        let class = generated_release_classification(milestone, "generated_contract", false);
        push_release_asset(
            &mut records,
            &mut expected,
            source,
            source,
            "generated_config",
            class,
        )?;
    }

    for (kind, sources) in [
        ("public_document", release.public_documents.as_slice()),
        ("host_asset", release.host_assets.as_slice()),
        ("operator_script", release.operator_scripts.as_slice()),
        ("python_artifact", release.python_artifacts.as_slice()),
        ("trace_fixture", release.trace_fixtures.as_slice()),
        ("transcript_fixture", release.transcript_fixtures.as_slice()),
        ("ui_asset", release.ui_assets.as_slice()),
        ("support_file", release.support_files.as_slice()),
        (
            "versioned_migration",
            release.versioned_migrations.as_slice(),
        ),
    ] {
        for source in sources {
            let source_path = repo_root.join(source);
            if !source_path.is_file() {
                bail!("release source is missing or not a regular file: {source}");
            }
            let class = tracked
                .iter()
                .find(|record| record.path == *source)
                .map(|record| record.classification.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!("release source has no classified tracked row: {source}")
                })?;
            let destination = release_destination(kind, source, &release.version)?;
            push_release_asset(
                &mut records,
                &mut expected,
                source,
                &destination,
                kind,
                class,
            )?;
        }
    }

    for destination in &release.generated_bundle_files {
        let production = destination == "qemu/run.sh";
        let class = if production {
            release_class.clone()
        } else {
            generated_release_classification(milestone, "generated_release_metadata", false)
        };
        push_release_asset(
            &mut records,
            &mut expected,
            "release builder",
            destination,
            "generated_bundle_file",
            class,
        )?;
    }

    records.sort_by(|a, b| a.id.cmp(&b.id));
    release.expected_bundle_files = expected.into_iter().collect();
    release.asset_records = records;
    Ok(release)
}

fn release_destination(kind: &str, source: &str, version: &str) -> Result<String> {
    let destination = match kind {
        "public_document" if source == "docs/QUICKSTART.md" => "QUICKSTART.md".to_owned(),
        "python_artifact" => format!(
            "python/cohesix-py/{}",
            source
                .strip_prefix("tools/cohesix-py/")
                .ok_or_else(|| anyhow::anyhow!(
                    "Python release path is outside tools/cohesix-py: {source}"
                ))?
        ),
        "trace_fixture" => format!(
            "traces/{}",
            source
                .strip_prefix("tests/fixtures/traces/")
                .ok_or_else(|| anyhow::anyhow!(
                    "trace fixture is outside tests/fixtures/traces: {source}"
                ))?
        ),
        "transcript_fixture" => format!(
            "tests/fixtures/transcripts/{}",
            source
                .strip_prefix("tests/fixtures/transcripts/")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "transcript fixture is outside tests/fixtures/transcripts: {source}"
                    )
                })?
        ),
        "ui_asset" => format!(
            "ui/swarmui/{}",
            source
                .strip_prefix("apps/swarmui/frontend/")
                .ok_or_else(|| anyhow::anyhow!(
                    "UI asset is outside apps/swarmui/frontend: {source}"
                ))?
        ),
        "support_file" if source == format!("releases/RELEASE_NOTES-{version}.md") => {
            "RELEASE_NOTES.md".to_owned()
        }
        _ => source.to_owned(),
    };
    Ok(destination)
}

fn push_release_asset(
    records: &mut Vec<SurfaceRecord>,
    expected: &mut BTreeSet<String>,
    source: &str,
    destination: &str,
    kind: &str,
    mut classification: Classification,
) -> Result<()> {
    if !expected.insert(destination.to_owned()) {
        bail!("duplicate release bundle destination: {destination}");
    }
    classification.selection_source = format!(
        "{}; exact release source `{source}`",
        classification.selection_source
    );
    records.push(SurfaceRecord {
        id: format!("release-asset:{destination}"),
        kind: kind.to_owned(),
        path: destination.to_owned(),
        classification,
    });
    Ok(())
}

fn generated_release_classification(
    milestone: &str,
    observed_mode: &str,
    production_reachable: bool,
) -> Classification {
    Classification {
        implementation_class: "contract".to_owned(),
        owner: "release-tooling".to_owned(),
        milestone: milestone.to_owned(),
        production_reachable,
        selection_source: "compiler-generated exact release manifest".to_owned(),
        package_disposition: "generated_release_contract".to_owned(),
        evidence_requirement: "exact file-set and SHA-256 validation".to_owned(),
        current_observed_mode: observed_mode.to_owned(),
        evidence_eligible: false,
    }
}

fn load_workspace_packages(repo_root: &Path) -> Result<Vec<CargoPackage>> {
    let workspace_path = repo_root.join("Cargo.toml");
    let workspace_text = fs::read_to_string(&workspace_path)?;
    let workspace: toml::Value = toml::from_str(&workspace_text)?;
    let members = workspace
        .get("workspace")
        .and_then(|value| value.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("workspace.members missing"))?;
    let mut packages = Vec::new();
    for member in members {
        let path = member
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("workspace member is not a string"))?;
        let manifest_path = repo_root.join(path).join("Cargo.toml");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest: toml::Value = toml::from_str(&manifest_text)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| anyhow::anyhow!("{} has no package table", manifest_path.display()))?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("{} has no package.name", manifest_path.display()))?
            .to_owned();
        let mut targets = Vec::new();
        if manifest.get("lib").is_some() || repo_root.join(path).join("src/lib.rs").is_file() {
            let lib_name = manifest
                .get("lib")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| name.replace('-', "_"));
            targets.push(("lib".to_owned(), lib_name));
        }
        let autobins = package
            .get("autobins")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        if autobins && repo_root.join(path).join("src/main.rs").is_file() {
            targets.push(("bin".to_owned(), name.clone()));
        }
        if autobins {
            let bin_dir = repo_root.join(path).join("src/bin");
            if bin_dir.is_dir() {
                for entry in fs::read_dir(&bin_dir)
                    .with_context(|| format!("failed to read {}", bin_dir.display()))?
                {
                    let entry = entry
                        .with_context(|| format!("failed to enumerate {}", bin_dir.display()))?;
                    let entry_path = entry.path();
                    let bin_name = if entry_path.is_file()
                        && entry_path.extension().and_then(|value| value.to_str()) == Some("rs")
                    {
                        entry_path.file_stem().and_then(|value| value.to_str())
                    } else if entry_path.join("main.rs").is_file() {
                        entry_path.file_name().and_then(|value| value.to_str())
                    } else {
                        None
                    };
                    if let Some(bin_name) = bin_name {
                        targets.push(("bin".to_owned(), bin_name.to_owned()));
                    }
                }
            }
        }
        if let Some(bins) = manifest.get("bin").and_then(toml::Value::as_array) {
            for bin in bins {
                let bin_name = bin
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| {
                        anyhow::anyhow!("{} bin missing name", manifest_path.display())
                    })?;
                targets.push(("bin".to_owned(), bin_name.to_owned()));
            }
        }
        targets.sort();
        targets.dedup();
        let mut features = manifest
            .get("features")
            .and_then(toml::Value::as_table)
            .map(|table| {
                table
                    .iter()
                    .map(|(name, values)| {
                        let values = values
                            .as_array()
                            .map(|array| {
                                array
                                    .iter()
                                    .filter_map(toml::Value::as_str)
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        (name.clone(), values)
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        if let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) {
            for (dependency_name, dependency) in dependencies {
                let optional = dependency
                    .as_table()
                    .and_then(|table| table.get("optional"))
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                if !optional {
                    continue;
                }
                let explicitly_suppressed = features.values().flatten().any(|value| {
                    value
                        .strip_prefix("dep:")
                        .is_some_and(|name| name == dependency_name)
                });
                if !explicitly_suppressed {
                    features
                        .entry(dependency_name.clone())
                        .or_insert_with(|| vec![format!("dep:{dependency_name}")]);
                }
            }
        }
        packages.push(CargoPackage {
            name,
            path: path.to_owned(),
            targets,
            features,
        });
    }
    packages.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(packages)
}

fn compile_tracked_surfaces(
    repo_root: &Path,
    source: &InventorySource,
) -> Result<Vec<SurfaceRecord>> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(repo_root)
        .output()
        .context("failed to enumerate tracked implementation surfaces")?;
    if !output.status.success() {
        bail!("git ls-files failed while compiling implementation surfaces");
    }
    let mut records = Vec::new();
    for bytes in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
    {
        let path = std::str::from_utf8(bytes).context("tracked path is not UTF-8")?;
        let matches = source
            .tracked_rules
            .iter()
            .filter(|rule| rule.matches(path))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            continue;
        }
        if matches.len() != 1 {
            bail!(
                "tracked surface {path} matches multiple rules {:?}",
                matches
                    .iter()
                    .map(|rule| rule.id.as_str())
                    .collect::<Vec<_>>()
            );
        }
        let rule = matches[0];
        validate_classification(
            &rule.class,
            &rule.owner,
            &rule.selection_source,
            &rule.package_disposition,
            &rule.evidence_requirement,
            &rule.current_observed_mode,
            rule.production_reachable,
        )?;
        records.push(SurfaceRecord {
            id: format!("tracked:{path}"),
            kind: "tracked_surface".to_owned(),
            path: path.to_owned(),
            classification: Classification {
                implementation_class: rule.class.clone(),
                owner: rule.owner.clone(),
                milestone: source.milestone.clone(),
                production_reachable: rule.production_reachable,
                selection_source: format!("{}; rule={}", rule.selection_source, rule.id),
                package_disposition: rule.package_disposition.clone(),
                evidence_requirement: rule.evidence_requirement.clone(),
                current_observed_mode: rule.current_observed_mode.clone(),
                evidence_eligible: !CLAIM_INELIGIBLE_CLASSES.contains(&rule.class.as_str()),
            },
        });
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}

impl TrackedRule {
    fn matches(&self, path: &str) -> bool {
        if self.exclude_exact.iter().any(|value| value == path)
            || self
                .exclude_prefix
                .iter()
                .any(|value| path.starts_with(value))
        {
            return false;
        }
        if let Some(exact) = &self.exact {
            if path != exact {
                return false;
            }
        }
        if let Some(prefix) = &self.prefix {
            if !path.starts_with(prefix) {
                return false;
            }
        }
        if let Some(suffix) = &self.suffix {
            if !path.ends_with(suffix) {
                return false;
            }
        }
        self.exact.is_some() || self.prefix.is_some() || self.suffix.is_some()
    }
}

fn surface_from_source(source: &SurfaceSource, milestone: &str) -> Result<SurfaceRecord> {
    validate_classification(
        &source.class,
        &source.owner,
        &source.selection_source,
        &source.package_disposition,
        &source.evidence_requirement,
        &source.current_observed_mode,
        source.production_reachable,
    )?;
    require_nonempty("surface id", &source.id)?;
    require_nonempty("surface kind", &source.kind)?;
    require_nonempty("surface path", &source.path)?;
    Ok(SurfaceRecord {
        id: source.id.clone(),
        kind: source.kind.clone(),
        path: source.path.clone(),
        classification: Classification {
            implementation_class: source.class.clone(),
            owner: source.owner.clone(),
            milestone: milestone.to_owned(),
            production_reachable: source.production_reachable,
            selection_source: source.selection_source.clone(),
            package_disposition: source.package_disposition.clone(),
            evidence_requirement: source.evidence_requirement.clone(),
            current_observed_mode: source.current_observed_mode.clone(),
            evidence_eligible: !CLAIM_INELIGIBLE_CLASSES.contains(&source.class.as_str()),
        },
    })
}

fn classification_from_package(source: &PackageSource, milestone: &str) -> Result<Classification> {
    validate_classification(
        &source.class,
        &source.owner,
        &source.selection_source,
        &source.package_disposition,
        &source.evidence_requirement,
        &source.current_observed_mode,
        source.production_reachable,
    )?;
    Ok(Classification {
        implementation_class: source.class.clone(),
        owner: source.owner.clone(),
        milestone: milestone.to_owned(),
        production_reachable: source.production_reachable,
        selection_source: source.selection_source.clone(),
        package_disposition: source.package_disposition.clone(),
        evidence_requirement: source.evidence_requirement.clone(),
        current_observed_mode: source.current_observed_mode.clone(),
        evidence_eligible: !CLAIM_INELIGIBLE_CLASSES.contains(&source.class.as_str()),
    })
}

fn validate_classification(
    implementation_class: &str,
    owner: &str,
    selection_source: &str,
    package_disposition: &str,
    evidence_requirement: &str,
    current_observed_mode: &str,
    production_reachable: bool,
) -> Result<()> {
    if !ALLOWED_CLASSES.contains(&implementation_class) {
        bail!("unsupported implementation class {implementation_class}");
    }
    for (label, value) in [
        ("owner", owner),
        ("selection_source", selection_source),
        ("package_disposition", package_disposition),
        ("evidence_requirement", evidence_requirement),
        ("current_observed_mode", current_observed_mode),
    ] {
        require_nonempty(label, value)?;
    }
    if production_reachable
        && matches!(
            implementation_class,
            "fixture" | "model_only" | "deferred" | "retired" | "not_enabled"
        )
    {
        bail!("production-reachable surface cannot be classed {implementation_class}");
    }
    Ok(())
}

fn validate_unique_ids(
    packages: &[PackageRecord],
    surfaces: &[SurfaceRecord],
    tracked: &[SurfaceRecord],
) -> Result<()> {
    let mut ids = BTreeSet::new();
    for id in packages
        .iter()
        .flat_map(|package| {
            std::iter::once(package.id.as_str())
                .chain(package.targets.iter().map(|record| record.id.as_str()))
                .chain(package.features.iter().map(|record| record.id.as_str()))
        })
        .chain(surfaces.iter().map(|record| record.id.as_str()))
        .chain(tracked.iter().map(|record| record.id.as_str()))
    {
        if !ids.insert(id) {
            bail!("duplicate implementation-surface id {id}");
        }
    }
    Ok(())
}

fn require_nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn sorted_closures(mut values: Vec<RuntimeClosureSource>) -> Vec<RuntimeClosureSource> {
    values.sort_by(|a, b| a.name.cmp(&b.name));
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_reachable_fixture_is_rejected() {
        let err = validate_classification(
            "fixture",
            "test-owner",
            "test selection",
            "not_packaged",
            "fixture_only",
            "fixture",
            true,
        )
        .expect_err("production fixture must fail");
        assert!(err.to_string().contains("production-reachable"));
    }

    #[test]
    fn tracked_rule_honours_exact_exclusion() {
        let rule = TrackedRule {
            id: "scripts".to_owned(),
            exact: None,
            prefix: Some("scripts/cohsh/".to_owned()),
            suffix: Some(".coh".to_owned()),
            exclude_exact: vec!["scripts/cohsh/host_sidecar_mock.coh".to_owned()],
            exclude_prefix: Vec::new(),
            class: "diagnostic".to_owned(),
            owner: "host-tools".to_owned(),
            production_reachable: false,
            selection_source: "tracked scripts".to_owned(),
            package_disposition: "not_packaged".to_owned(),
            evidence_requirement: "diagnostic_only".to_owned(),
            current_observed_mode: "script".to_owned(),
        };
        assert!(rule.matches("scripts/cohsh/tcp_basic.coh"));
        assert!(!rule.matches("scripts/cohsh/host_sidecar_mock.coh"));
    }

    #[test]
    fn runtime_feature_expansion_finds_nested_forbidden_feature() {
        let features = BTreeMap::from([
            ("release".to_owned(), vec!["clock".to_owned()]),
            ("clock".to_owned(), vec!["dummy".to_owned()]),
            ("dummy".to_owned(), Vec::new()),
        ]);
        let mut expanded = BTreeSet::new();
        expand_feature("release", &features, &mut expanded).expect("expand");
        assert!(expanded.contains("dummy"));
    }

    #[test]
    fn repository_inventory_source_compiles() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("coh-rtc must live under <repo>/tools");
        let temp = tempfile::tempdir().expect("tempdir");
        compile_inventory(
            &repo_root.join("configs/implementation_surfaces.toml"),
            &temp.path().join("implementation_surface_inventory.json"),
        )
        .expect("repository implementation-surface source must compile");
    }

    #[test]
    fn repository_inventory_passes_drift_guard() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("coh-rtc must live under <repo>/tools");
        let temp = tempfile::tempdir().expect("tempdir");
        let inventory = temp.path().join("implementation_surface_inventory.json");
        compile_inventory(
            &repo_root.join("configs/implementation_surfaces.toml"),
            &inventory,
        )
        .expect("repository implementation-surface source must compile");
        let status = std::process::Command::new("python3")
            .arg(repo_root.join("scripts/ci/check_implementation_surfaces.py"))
            .arg("--repo-root")
            .arg(repo_root)
            .arg("--inventory")
            .arg(&inventory)
            .status()
            .expect("run implementation-surface drift guard");
        assert!(
            status.success(),
            "implementation-surface drift guard failed"
        );
    }
}
