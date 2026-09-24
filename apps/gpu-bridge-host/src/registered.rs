// Author: Lukas Bower
// Purpose: Admit privately enrolled, digest-pinned CUDA programs with typed inputs and bounded native output.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{reference, workload};
use anyhow::{anyhow, bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const MAX_REGISTRATION: usize = 8192;
const MAX_PACKAGE: usize = 16 * 1024 * 1024;
const MAX_INPUT: u64 = 262_144;
const MAX_OUTPUT: u64 = 262_144;
const HEADROOM: u64 = 2 * 1024 * 1024 * 1024;

/// User values are data to a fixed program, never shell tokens or path components.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredRequest {
    /// Versioned registered request grammar.
    pub schema: String,
    /// Original admitted ticket.
    pub ticket_id: String,
    /// Immutable administrative registration record.
    pub registration_sha256: String,
    /// Input file named by its content digest under the enrolled root.
    pub input_sha256: String,
    /// Data values checked against the registration's typed bounds.
    pub parameters: BTreeMap<String, Value>,
    /// Exact native ordinal in the selected visible device set.
    pub device_ordinal: u32,
    /// Exact selected CUDA UUID.
    pub device_uuid: String,
    /// Native inventory time, subject to the five-second freshness fence.
    pub inventory_observed_unix_ms: u64,
    /// Generated provider contract graph digest.
    pub provider_graph_sha256: String,
    /// Root-reserved maximum native allocation.
    pub memory_budget_bytes: u64,
    /// Whole native child deadline.
    pub deadline_ms: u32,
}

impl RegisteredRequest {
    /// Validate all request controlled arithmetic before reading a package or dataset.
    pub fn validate(&self, now: u64, graph: &str) -> Result<()> {
        cohesix_authority::validate_id(&self.ticket_id)
            .map_err(|_| anyhow!("invalid_registered_ticket"))?;
        ensure!(
            self.schema == "cohesix-registered-cuda-request/v1"
                && hash(&self.registration_sha256)
                && hash(&self.input_sha256)
                && self.device_ordinal == 0
                && self.device_uuid.len() == 32
                && self.device_uuid.bytes().all(hex_digit)
                && self.device_uuid.bytes().any(|b| b != b'0')
                && self.provider_graph_sha256 == graph
                && self.inventory_observed_unix_ms <= now
                && now - self.inventory_observed_unix_ms < 5000
                && (1..=64 * 1024 * 1024).contains(&self.memory_budget_bytes)
                && (1..=30_000).contains(&self.deadline_ms)
                && self.parameters.len() <= 8,
            "invalid_registered_request"
        );
        Ok(())
    }
}

/// A private administrative record, named by the SHA-256 of its canonical JSON.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    /// Versioned privileged enrollment grammar.
    pub schema: String,
    /// Stable workload identity.
    pub id: String,
    /// Absolute, canonical package executable selected by an administrator.
    pub package: PathBuf,
    /// Exact package executable digest.
    pub package_sha256: String,
    /// Fixed executable entrypoint, currently `run`.
    pub entrypoint: String,
    /// Absolute, canonical input CAS directory.
    pub input_root: PathBuf,
    /// Largest accepted input artifact.
    pub max_input_bytes: u64,
    /// Largest retained output artifact.
    pub max_output_bytes: u64,
    /// Maximum requested device allocation.
    pub max_memory_bytes: u64,
    /// Package, input, output and metadata disk ceiling.
    pub max_disk_bytes: u64,
    /// Maximum child deadline.
    pub max_deadline_ms: u32,
    /// Only device this registration can use.
    pub device_uuid: String,
    /// Exact permitted parameter names and bounds.
    pub parameters: BTreeMap<String, Parameter>,
    /// Administrator selected references; values never enter a request or journal.
    pub secret_refs: BTreeMap<String, String>,
}

/// One finite integer or named choice. No value can select an executable or path.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Parameter {
    /// Unsigned integer within an inclusive finite range.
    Integer {
        /// Lowest permitted value.
        minimum: u64,
        /// Highest permitted value.
        maximum: u64,
    },
    /// One exact value from an administrator supplied finite set.
    Choice {
        /// Allowed opaque labels.
        values: Vec<String>,
    },
}

fn hex_digit(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(hex_digit)
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
}

impl Registration {
    /// Check the administrative schema without fetching secret contents.
    pub fn validate(&self) -> Result<()> {
        cohesix_authority::validate_id(&self.id).map_err(|_| anyhow!("invalid_registration_id"))?;
        ensure!(
            self.schema == "cohesix-cuda-registration/v1"
                && self.entrypoint == "run"
                && hash(&self.package_sha256)
                && self.package.is_absolute()
                && self.input_root.is_absolute()
                && self.package.canonicalize()? == self.package
                && self.input_root.canonicalize()? == self.input_root
                && self.input_root.is_dir()
                && (1..=MAX_INPUT).contains(&self.max_input_bytes)
                && (1..=MAX_OUTPUT).contains(&self.max_output_bytes)
                && (1..=64 * 1024 * 1024).contains(&self.max_memory_bytes)
                && (1..=30_000).contains(&self.max_deadline_ms)
                && self.max_disk_bytes <= 32 * 1024 * 1024
                && self.device_uuid.len() == 32
                && self.device_uuid.bytes().all(hex_digit)
                && self.parameters.len() <= 8
                && self.secret_refs.len() <= 8,
            "invalid_registration"
        );
        let package_size = fs::symlink_metadata(&self.package)?.len();
        ensure!(
            package_size > 0
                && package_size <= MAX_PACKAGE as u64
                && package_size
                    .checked_add(self.max_input_bytes)
                    .and_then(|size| size.checked_add(self.max_output_bytes))
                    .and_then(|size| size.checked_add(16_384))
                    .is_some_and(|size| size <= self.max_disk_bytes),
            "registration_disk_bound"
        );
        for (key, bound) in &self.parameters {
            ensure!(name(key), "registration_parameter_name");
            match bound {
                Parameter::Integer { minimum, maximum } => {
                    ensure!(
                        minimum <= maximum && *maximum <= 1_000_000,
                        "registration_integer_bound"
                    );
                }
                Parameter::Choice { values } => {
                    ensure!(
                        !values.is_empty()
                            && values.len() <= 16
                            && values.iter().all(|value| {
                                !value.is_empty()
                                    && value.len() <= 64
                                    && value
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                            }),
                        "registration_choice_bound"
                    );
                }
            }
        }
        for (key, reference) in &self.secret_refs {
            ensure!(
                key.starts_with("COH_USER_") && name(key),
                "registration_secret_name"
            );
            cohesix_authority::secret::validate_reference(reference)?;
        }
        Ok(())
    }

    fn check_request(&self, request: &RegisteredRequest) -> Result<()> {
        ensure!(
            self.device_uuid == request.device_uuid
                && request.memory_budget_bytes <= self.max_memory_bytes
                && request.deadline_ms <= self.max_deadline_ms
                && request.parameters.len() == self.parameters.len(),
            "registration_request_bound"
        );
        for (key, bound) in &self.parameters {
            let value = request
                .parameters
                .get(key)
                .ok_or_else(|| anyhow!("missing_parameter"))?;
            match bound {
                Parameter::Integer { minimum, maximum } => {
                    ensure!(
                        value
                            .as_u64()
                            .is_some_and(|v| (*minimum..=*maximum).contains(&v)),
                        "parameter_bound"
                    );
                }
                Parameter::Choice { values } => {
                    ensure!(
                        value
                            .as_str()
                            .is_some_and(|v| values.iter().any(|choice| choice == v)),
                        "parameter_choice"
                    );
                }
            }
        }
        Ok(())
    }
}

fn load(root: &Path, digest: &str) -> Result<Registration> {
    ensure!(hash(digest), "invalid_registration_digest");
    let directory = root.join("registrations");
    ensure!(directory.exists(), "registration_not_enrolled");
    private_directory(&directory)?;
    let path = directory.join(format!("{digest}.json"));
    ensure!(path.exists(), "registration_not_enrolled");
    let bytes = workload::read_file(&path, MAX_REGISTRATION)?;
    ensure!(
        workload::digest(&bytes) == digest,
        "registration_digest_mismatch"
    );
    let registration: Registration = serde_json::from_slice(&bytes)?;
    registration.validate()?;
    let package = workload::read_file(&registration.package, MAX_PACKAGE)?;
    ensure!(
        workload::digest(&package) == registration.package_sha256,
        "package_digest_mismatch"
    );
    Ok(registration)
}

fn private_directory(path: &Path) -> Result<()> {
    let info = fs::symlink_metadata(path)?;
    ensure!(
        info.is_dir() && !info.file_type().is_symlink() && info.permissions().mode() & 0o077 == 0,
        "registration_directory_not_private"
    );
    Ok(())
}

/// Install validated registration bytes under the executor owner's private root.
pub fn install(source: &Path, state_root: &Path) -> Result<String> {
    private_directory(state_root)?;
    let bytes = workload::read_file(source, MAX_REGISTRATION)?;
    let record: Registration = serde_json::from_slice(&bytes)?;
    record.validate()?;
    ensure!(
        workload::digest(&workload::read_file(&record.package, MAX_PACKAGE)?)
            == record.package_sha256,
        "package_digest_mismatch"
    );
    let directory = state_root.join("registrations");
    if directory.exists() {
        private_directory(&directory)?;
    } else {
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
    }
    let digest = workload::digest(&bytes);
    let target = directory.join(format!("{digest}.json"));
    if target.exists() {
        ensure!(
            workload::read_file(&target, MAX_REGISTRATION)? == bytes,
            "registration_conflict"
        );
        return Ok(digest);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    File::open(&directory)?.sync_all()?;
    Ok(digest)
}

/// Validate package enrollment without exposing environment or secret values.
pub fn inspect(source: &Path) -> Result<Value> {
    let bytes = workload::read_file(source, MAX_REGISTRATION)?;
    let record: Registration = serde_json::from_slice(&bytes)?;
    record.validate()?;
    ensure!(
        workload::digest(&workload::read_file(&record.package, MAX_PACKAGE)?)
            == record.package_sha256,
        "package_digest_mismatch"
    );
    Ok(json!({
        "schema":"cohesix-cuda-registration-inspection/v1",
        "id":record.id,
        "registration_sha256":workload::digest(&bytes),
        "package_sha256":record.package_sha256,
        "device_uuid":record.device_uuid,
        "entrypoint":record.entrypoint,
        "parameters":record.parameters,
        "limits":{
            "input_bytes":record.max_input_bytes,
            "output_bytes":record.max_output_bytes,
            "memory_bytes":record.max_memory_bytes,
            "disk_bytes":record.max_disk_bytes,
            "deadline_ms":record.max_deadline_ms,
        },
        "secret_names":record.secret_refs.keys().collect::<Vec<_>>(),
        "authoritative":false,
    }))
}

/// Measure the selected native device and show admission headroom separately from caps.
pub fn diagnose(config_path: &Path) -> Result<Value> {
    let config = workload::Config::load(config_path)?;
    private_directory(&config.state_root)?;
    let state = tempfile::Builder::new()
        .prefix("workload-diagnostic-")
        .tempdir_in(&config.state_root)?;
    let inventory = reference::inventory_selected(
        &config.helper,
        &config.helper_sha256,
        &state.path().join("inventory"),
        0,
        config.mig.as_ref(),
    )?;
    let native = &inventory["native"];
    let free = native["free_bytes"]
        .as_u64()
        .ok_or_else(|| anyhow!("device_free_unavailable"))?;
    ensure!(
        native["device_uuid"] == config.device_uuid,
        "diagnostic_device_changed"
    );
    let thermal = thermal_observation(&config.device_uuid);
    Ok(json!({
        "schema":"cohesix-cuda-workload-diagnostic/v1",
        "authoritative":false,
        "proof_class":"native_discovery",
        "device_uuid":config.device_uuid,
        "inventory_observed_unix_ms":inventory["observed_unix_ms"],
        "topology_sha256":inventory["topology_sha256"],
        "free_bytes_measured":free,
        "total_bytes_measured":native["total_bytes"],
        "os_headroom_bytes":HEADROOM,
        "admission_capacity_bytes_estimate":free.saturating_sub(HEADROOM),
        "request_cap_bytes_enforced":64 * 1024 * 1024,
        "runtime_version":native["runtime_version"],
        "driver_version":native["driver_version"],
        "compute_major":native["compute_major"],
        "compute_minor":native["compute_minor"],
        "health_observation":"native_inventory_succeeded",
        "thermal":thermal,
        "queue_state":"use original admitted job status; diagnostic does not inspect private WAL",
    }))
}

#[cfg(feature = "nvml")]
fn thermal_observation(expected_uuid: &str) -> Value {
    use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};
    let observed = (|| -> Result<Value> {
        let nvml = Nvml::init()?;
        let device = nvml.device_by_index(0)?;
        let uuid = device
            .uuid()?
            .trim_start_matches("GPU-")
            .replace('-', "")
            .to_lowercase();
        ensure!(uuid == expected_uuid, "thermal_device_changed");
        Ok(json!({
            "status":"observed",
            "temperature_celsius":device.temperature(TemperatureSensor::Gpu).ok(),
            "power_milliwatts":device.power_usage().ok(),
        }))
    })();
    observed.unwrap_or_else(|_| json!({"status":"unavailable"}))
}

#[cfg(not(feature = "nvml"))]
fn thermal_observation(_expected_uuid: &str) -> Value {
    json!({"status":"unavailable"})
}

fn input(registration: &Registration, digest: &str) -> Result<Vec<u8>> {
    let path = registration.input_root.join(digest);
    let bytes = workload::read_file(&path, registration.max_input_bytes as usize)?;
    ensure!(workload::digest(&bytes) == digest, "input_digest_mismatch");
    Ok(bytes)
}

/// Refuse registration, package, data and parameter drift before native start.
pub fn preflight(root: &Path, request: &RegisteredRequest) -> Result<()> {
    let registration = load(root, &request.registration_sha256)?;
    registration.check_request(request)?;
    input(&registration, &request.input_sha256)?;
    Ok(())
}

/// Run one owned pinned child after the bridge has rechecked the CUDA topology.
pub fn execute(
    root: &Path,
    state: &Path,
    request: &RegisteredRequest,
    inventory: &Value,
    cancel: &AtomicBool,
    visibility: Option<&str>,
) -> Result<Value> {
    let registration = load(root, &request.registration_sha256)?;
    registration.check_request(request)?;
    let data = input(&registration, &request.input_sha256)?;
    let free = inventory["native"]["free_bytes"]
        .as_u64()
        .ok_or_else(|| anyhow!("device_free_unavailable"))?;
    ensure!(
        free.checked_sub(HEADROOM)
            .is_some_and(|remaining| remaining >= request.memory_budget_bytes),
        "memory_admission measured_headroom"
    );
    let selected = reference::prepare(&registration.package, &registration.package_sha256, state)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state.join("input.bin"))?;
    file.write_all(&data)?;
    file.sync_all()?;
    File::open(state)?.sync_all()?;
    let mut environment = BTreeMap::new();
    for (key, reference) in &registration.secret_refs {
        environment.insert(
            key.clone(),
            cohesix_authority::secret::resolve_reference(reference)?,
        );
    }
    let mut arguments = vec![
        registration.entrypoint.clone(),
        request.device_uuid.clone(),
        "input.bin".into(),
        "output.bin".into(),
    ];
    for (key, value) in &request.parameters {
        arguments.push(key.clone());
        arguments.push(match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            _ => bail!("parameter_type"),
        });
    }
    let native = reference::invoke_with_env(
        &selected,
        state,
        &arguments,
        Instant::now() + Duration::from_millis(u64::from(request.deadline_ms)),
        cancel,
        visibility,
        8192,
        &environment,
        Some((registration.max_disk_bytes, registration.max_output_bytes)),
    )?;
    ensure!(
        native["schema"] == "cohesix-registered-cuda-result/v1"
            && native["device_uuid"] == request.device_uuid
            && native["workload_id"] == registration.id
            && native["parameters"] == serde_json::to_value(&request.parameters)?
            && native["allocation_bytes"]
                .as_u64()
                .is_some_and(|size| size <= request.memory_budget_bytes)
            && native["free_bytes_before"]
                .as_u64()
                .is_some_and(|bytes| bytes >= HEADROOM + request.memory_budget_bytes)
            && native["driver_version"] == inventory["native"]["driver_version"]
            && native["runtime_version"] == inventory["native"]["runtime_version"],
        "registered_native_identity"
    );
    let output_path = state.join("output.bin");
    let bytes = workload::read_file(&output_path, registration.max_output_bytes as usize)?;
    ensure!(
        native["output_bytes"] == bytes.len() && !bytes.is_empty(),
        "registered_output_bound"
    );
    let output_sha256 = workload::digest(&bytes);
    Ok(json!({
        "schema":"cohesix-registered-cuda-observation/v1",
        "mode":"live", "proof_class":"native_provider_operation",
        "authoritative":false, "worker_proof":false,
        "ticket_id":request.ticket_id,
        "registration_sha256":request.registration_sha256,
        "package_sha256":registration.package_sha256,
        "input_sha256":request.input_sha256,
        "provider_graph_sha256":request.provider_graph_sha256,
        "outcome":"verified", "isolation":"dedicated_process_and_cuda_context",
        "streams":1,
        "output":{"sha256":output_sha256,"bytes":bytes.len(),"path":"output.bin"},
        "native":native,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture(root: &Path) -> (PathBuf, RegisteredRequest) {
        let package = root.join("batch-edges");
        fs::write(&package, b"pinned executable bytes").unwrap();
        let input_root = root.join("inputs");
        fs::create_dir(&input_root).unwrap();
        let input_sha256 = workload::digest(b"image bytes");
        fs::write(input_root.join(&input_sha256), b"image bytes").unwrap();
        let registration = json!({
            "schema":"cohesix-cuda-registration/v1", "id":"batch-edges",
            "package":package, "package_sha256":workload::digest(b"pinned executable bytes"),
            "entrypoint":"run", "input_root":input_root,
            "max_input_bytes":64, "max_output_bytes":64,
            "max_memory_bytes":1048576, "max_disk_bytes":65536,
            "max_deadline_ms":1000, "device_uuid":"a".repeat(32),
            "parameters":{"frames":{"kind":"integer","minimum":1,"maximum":4}},
            "secret_refs":{}
        });
        let source = root.join("registration.json");
        fs::write(&source, serde_json::to_vec(&registration).unwrap()).unwrap();
        let request = RegisteredRequest {
            schema: "cohesix-registered-cuda-request/v1".into(),
            ticket_id: "ticket".into(),
            registration_sha256: workload::digest(&fs::read(&source).unwrap()),
            input_sha256,
            parameters: BTreeMap::from([("frames".into(), json!(2))]),
            device_ordinal: 0,
            device_uuid: "a".repeat(32),
            inventory_observed_unix_ms: 999,
            provider_graph_sha256: "b".repeat(64),
            memory_budget_bytes: 1024,
            deadline_ms: 1000,
        };
        (source, request)
    }

    #[test]
    fn pinned_registration_refuses_unapproved_inputs_before_native_start() {
        let temporary = tempfile::tempdir().unwrap();
        let canonical = temporary.path().canonicalize().unwrap();
        let root = canonical.as_path();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let (source, mut request) = fixture(root);
        request.validate(1000, &"b".repeat(64)).unwrap();
        let digest = install(&source, root).unwrap();
        assert_eq!(digest, request.registration_sha256);
        preflight(root, &request).unwrap();
        request.parameters.insert("frames".into(), json!("2;sh"));
        assert!(preflight(root, &request).is_err());
        request.parameters.insert("frames".into(), json!(5));
        assert!(preflight(root, &request).is_err());
        request.parameters.insert("frames".into(), json!(2));
        request.input_sha256 = "c".repeat(64);
        assert!(preflight(root, &request).is_err());
        request.input_sha256 = workload::digest(b"image bytes");
        fs::remove_file(root.join("inputs").join(&request.input_sha256)).unwrap();
        symlink(&source, root.join("inputs").join(&request.input_sha256)).unwrap();
        assert!(preflight(root, &request).is_err());
        fs::write(root.join("batch-edges"), b"altered package").unwrap();
        assert!(preflight(root, &request).is_err());
    }

    #[test]
    fn freshness_and_private_registration_directory_are_required() {
        let temporary = tempfile::tempdir().unwrap();
        let canonical = temporary.path().canonicalize().unwrap();
        let root = canonical.as_path();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let (source, request) = fixture(root);
        assert!(request.validate(6000, &"b".repeat(64)).is_err());
        assert!(request.validate(1000, &"c".repeat(64)).is_err());
        install(&source, root).unwrap();
        fs::set_permissions(
            root.join("registrations"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(preflight(root, &request).is_err());
    }
}
