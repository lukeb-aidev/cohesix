// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side GPU inventory, namespace serialisation, and model lifecycle helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-side GPU bridge utilities. The bridge discovers GPUs (mocked by
//! default) and materialises namespace entries that NineDoor can expose via the
//! `/gpu` mount. When built with the `nvml` feature the bridge performs real
//! discovery through `nvml-wrapper`.

#[cfg(feature = "cuda")]
use anyhow::Context;
use anyhow::{anyhow, ensure, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cohsh_core::MAX_ECHO_LEN;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TELEMETRY_SCHEMA_VERSION: &str = "gpu-telemetry/v1";
const MAX_TELEMETRY_BYTES: usize = 4096;
const REGISTRY_ACTIVE_FILE: &str = "active";
const REGISTRY_AVAILABLE_DIR: &str = "available";
const REGISTRY_MANIFEST_FILE: &str = "manifest.toml";
const MAX_REGISTRY_MANIFEST_BYTES: usize = 8 * 1024;
const MAX_REGISTRY_ID_BYTES: usize = 128;
const GPU_BRIDGE_WIRE_SCHEMA: &str = "gpu-bridge-snapshot/v2";
const GPU_BRIDGE_B64_PREFIX: &str = "b64:";
const DEFAULT_SNAPSHOT_TTL_MS: u64 = 15_000;
const MAX_SNAPSHOT_TTL_MS: u64 = 60_000;
const EMPTY_VALUE: &str = "-";
const PEFT_MODEL_FORMAT: &str = "safetensors+lora";
const PEFT_ADAPTER_FILE: &str = "adapter.safetensors";
const PEFT_LORA_FILE: &str = "lora.json";
const PEFT_METRICS_FILE: &str = "metrics.json";

/// Summary information about a GPU surfaced to the VM namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    /// Identifier used in `/gpu/<id>` paths.
    pub id: String,
    /// Human-friendly name of the GPU.
    pub name: String,
    /// Total memory in mebibytes.
    pub memory_mb: u32,
    /// Streaming multiprocessor count or equivalent.
    pub sm_count: u32,
    /// Driver version string.
    pub driver_version: String,
    /// Runtime version string.
    pub runtime_version: String,
}

impl GpuInfo {
    fn to_info_payload(&self) -> String {
        format!(
            "{{\n    \"id\": \"{}\",\n    \"name\": \"{}\",\n    \"memory_mb\": {},\n    \"sm_count\": {},\n    \"driver_version\": \"{}\",\n    \"runtime_version\": \"{}\"\n}}",
            escape_json_string(&self.id),
            escape_json_string(&self.name),
            self.memory_mb,
            self.sm_count,
            escape_json_string(&self.driver_version),
            escape_json_string(&self.runtime_version)
        )
    }
}

/// Namespace representation created by the bridge for each GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuNamespace {
    /// GPU metadata.
    pub info: GpuInfo,
    /// Initial control buffer contents.
    pub ctl_seed: String,
    /// Initial lease buffer contents.
    pub lease_seed: String,
    /// Initial status stream contents.
    pub status_seed: String,
}

impl GpuNamespace {
    /// Serialise the info node as JSON.
    #[must_use]
    pub fn info_payload(&self) -> String {
        self.info.to_info_payload()
    }

    /// Retrieve the initial control payload.
    #[must_use]
    pub fn ctl_payload(&self) -> &str {
        &self.ctl_seed
    }

    /// Retrieve the initial lease payload.
    #[must_use]
    pub fn lease_payload(&self) -> &str {
        &self.lease_seed
    }

    /// Retrieve the initial status payload.
    #[must_use]
    pub fn status_payload(&self) -> &str {
        &self.status_seed
    }
}

/// Model manifest mirrored into `/gpu/models/available/<id>/manifest.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    /// Identifier for the model, used in paths and telemetry.
    pub model_id: String,
    /// TOML manifest content documenting the model artefact.
    pub manifest_toml: String,
    /// SHA-256 of the exact manifest bytes.
    pub manifest_sha256: String,
    /// CAS digest of the model artefact named by the manifest.
    pub cas_sha256: String,
    /// Base-model identity for an adapter model.
    pub base_model_id: Option<String>,
    /// CAS digest of the adapter artefact, when present.
    pub adapter_sha256: Option<String>,
}

/// Host-side model catalog with an active pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuModelCatalog {
    /// Available models exported into `/gpu/models/available`.
    pub available: Vec<ModelManifest>,
    /// Active model identifier referenced by `/gpu/models/active`.
    pub active: String,
    /// Monotonic activation generation, or zero when no model is active.
    pub activation_generation: u64,
    /// Receipt binding the active model to its source/catalog generation.
    pub activation_receipt: String,
}

impl GpuModelCatalog {
    /// Payload for the active pointer file.
    #[must_use]
    pub fn active_pointer_payload(&self) -> String {
        format!("{}\n", self.active)
    }
}

/// Structured telemetry schema for LoRA/PEFT feedback loops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySchema {
    /// Schema version tag.
    pub version: String,
    /// Maximum size in bytes for a single record.
    pub max_record_bytes: usize,
    /// Required fields enforced by the bridge.
    pub required_fields: Vec<String>,
    /// Optional fields accepted by the bridge.
    pub optional_fields: Vec<String>,
}

impl TelemetrySchema {
    /// Construct the default LoRA-aware telemetry schema.
    #[must_use]
    pub fn lora_v1() -> Self {
        Self {
            version: TELEMETRY_SCHEMA_VERSION.to_string(),
            max_record_bytes: MAX_TELEMETRY_BYTES,
            required_fields: vec![
                "schema_version".to_string(),
                "device_id".to_string(),
                "model_id".to_string(),
                "time_window".to_string(),
                "token_count".to_string(),
                "latency_histogram".to_string(),
            ],
            optional_fields: vec![
                "lora_id".to_string(),
                "confidence".to_string(),
                "entropy".to_string(),
                "drift".to_string(),
                "feedback_flags".to_string(),
            ],
        }
    }

    /// Serialise the schema into a JSON descriptor for `/gpu/telemetry/schema.json`.
    #[must_use]
    pub fn descriptor_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        write!(
            &mut out,
            "\"schema_version\":\"{}\",",
            escape_json_string(&self.version)
        )
        .expect("write to string");
        write!(
            &mut out,
            "\"max_record_bytes\":{},\"required_fields\":[{}],\"optional_fields\":[{}]}}",
            self.max_record_bytes,
            self.required_fields
                .iter()
                .map(|field| format!("\"{}\"", escape_json_string(field)))
                .collect::<Vec<_>>()
                .join(","),
            self.optional_fields
                .iter()
                .map(|field| format!("\"{}\"", escape_json_string(field)))
                .collect::<Vec<_>>()
                .join(",")
        )
        .expect("write to string");
        out
    }
}

/// Telemetry record emitted by GPU workers.
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryRecord {
    /// Device identifier.
    pub device_id: String,
    /// Active model identifier.
    pub model_id: String,
    /// Optional LoRA adapter identifier.
    pub lora_id: Option<String>,
    /// Bounded time window label (e.g. ISO8601 interval).
    pub time_window: String,
    /// Token count processed in the window.
    pub token_count: u64,
    /// Latency histogram buckets in microseconds.
    pub latency_histogram: Vec<u64>,
    /// Optional confidence / entropy score.
    pub confidence: Option<f32>,
    /// Optional entropy measurement.
    pub entropy: Option<f32>,
    /// Optional drift indicator.
    pub drift: Option<String>,
    /// Optional operator feedback flags.
    pub feedback_flags: Option<String>,
}

impl TelemetryRecord {
    /// Encode the telemetry record as JSON under the provided schema with size validation.
    pub fn to_json(&self, schema: &TelemetrySchema) -> Result<String> {
        ensure!(
            schema.version == TELEMETRY_SCHEMA_VERSION,
            "unsupported telemetry schema version: {}",
            schema.version
        );
        let mut json = String::new();
        write!(
            &mut json,
            "{{\"schema_version\":\"{}\",\"device_id\":\"{}\",\"model_id\":\"{}\",\"time_window\":\"{}\",\"token_count\":{},\"latency_histogram\":[{}]",
            escape_json_string(&schema.version),
            escape_json_string(&self.device_id),
            escape_json_string(&self.model_id),
            escape_json_string(&self.time_window),
            self.token_count,
            self.latency_histogram
                .iter()
                .map(|bucket| bucket.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
        .expect("write to string");
        if let Some(lora_id) = &self.lora_id {
            write!(
                &mut json,
                ",\"lora_id\":\"{}\"",
                escape_json_string(lora_id)
            )
            .expect("write to string");
        }
        if let Some(confidence) = self.confidence {
            write!(&mut json, ",\"confidence\":{confidence:.6}").expect("write to string");
        }
        if let Some(entropy) = self.entropy {
            write!(&mut json, ",\"entropy\":{entropy:.6}").expect("write to string");
        }
        if let Some(drift) = &self.drift {
            write!(&mut json, ",\"drift\":\"{}\"", escape_json_string(drift))
                .expect("write to string");
        }
        if let Some(flags) = &self.feedback_flags {
            write!(
                &mut json,
                ",\"feedback_flags\":\"{}\"",
                escape_json_string(flags)
            )
            .expect("write to string");
        }
        json.push('}');
        ensure!(
            json.len() <= schema.max_record_bytes,
            "telemetry record exceeds max size: {} > {}",
            json.len(),
            schema.max_record_bytes
        );
        Ok(json)
    }
}

/// Abstraction over GPU inventory sources.
trait Inventory {
    fn discover(&self) -> Result<Vec<GpuInfo>>;
}

/// Backend used to discover GPU inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryBackend {
    /// Deterministic mock inventory.
    Mock,
    /// NVML-backed inventory (dGPU hosts).
    Nvml,
    /// CUDA driver/runtime inventory (Jetson).
    Cuda,
}

impl InventoryBackend {
    /// Return the stable backend label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Nvml => "nvml",
            Self::Cuda => "cuda",
        }
    }
}

/// Report describing which backend produced the inventory.
#[derive(Debug, Clone)]
pub struct InventoryReport {
    /// Backend that produced the inventory.
    pub backend: InventoryBackend,
    /// Primary backend that was attempted first, if a fallback was used.
    pub fallback_from: Option<InventoryBackend>,
    /// Reason for the fallback, if available.
    pub fallback_reason: Option<String>,
}

struct InventoryCandidate {
    backend: InventoryBackend,
    inventory: Box<dyn Inventory + Send + Sync>,
}

impl InventoryCandidate {
    fn new(backend: InventoryBackend, inventory: Box<dyn Inventory + Send + Sync>) -> Self {
        Self { backend, inventory }
    }
}

#[derive(Debug, Default)]
struct MockInventory;

impl Inventory for MockInventory {
    fn discover(&self) -> Result<Vec<GpuInfo>> {
        Ok(vec![
            GpuInfo {
                id: "GPU-0".into(),
                name: "Mock 4090".into(),
                memory_mb: 24_576,
                sm_count: 144,
                driver_version: "555.0".into(),
                runtime_version: "12.4".into(),
            },
            GpuInfo {
                id: "GPU-1".into(),
                name: "Mock 4060".into(),
                memory_mb: 8_192,
                sm_count: 64,
                driver_version: "555.0".into(),
                runtime_version: "12.4".into(),
            },
        ])
    }
}

#[cfg(feature = "nvml")]
#[derive(Debug, Default)]
struct NvmlInventory;

#[cfg(feature = "nvml")]
impl Inventory for NvmlInventory {
    fn discover(&self) -> Result<Vec<GpuInfo>> {
        use nvml_wrapper::{cuda_driver_version_major, cuda_driver_version_minor, Nvml};
        let nvml = Nvml::init()?;
        let device_count = nvml.device_count()?;
        let runtime_version = match nvml.sys_cuda_driver_version() {
            Ok(version) => format!(
                "{}.{}",
                cuda_driver_version_major(version),
                cuda_driver_version_minor(version)
            ),
            Err(_) => "unknown".to_string(),
        };
        let mut gpus = Vec::new();
        for index in 0..device_count {
            let device = nvml.device_by_index(index)?;
            let memory = device.memory_info()?;
            let info = GpuInfo {
                id: format!("GPU-{index}"),
                name: device.name()?.to_string(),
                memory_mb: (memory.total / (1024 * 1024)) as u32,
                sm_count: device
                    .attributes()
                    .map(|attrs| attrs.multiprocessor_count)
                    .unwrap_or(0),
                driver_version: nvml.sys_driver_version()?.to_string(),
                runtime_version: runtime_version.clone(),
            };
            gpus.push(info);
        }
        Ok(gpus)
    }
}

#[cfg(feature = "cuda")]
#[derive(Debug, Default)]
struct CudaInventory;

#[cfg(feature = "cuda")]
impl Inventory for CudaInventory {
    fn discover(&self) -> Result<Vec<GpuInfo>> {
        let devices = host_cuda::enumerate_devices().context("cuda inventory")?;
        if devices.is_empty() {
            return Err(anyhow!("cuda returned no devices"));
        }
        let mut gpus = Vec::new();
        for (index, device) in devices.into_iter().enumerate() {
            let memory_mb = device.total_memory_bytes / (1024 * 1024);
            let memory_mb = u32::try_from(memory_mb).with_context(|| {
                format!(
                    "cuda memory bytes overflow for GPU-{index}: {}",
                    device.total_memory_bytes
                )
            })?;
            let driver_version = if device.driver_version.trim().is_empty() {
                "unknown".to_owned()
            } else {
                device.driver_version
            };
            let runtime_version = if device.runtime_version.trim().is_empty() {
                "unknown".to_owned()
            } else {
                device.runtime_version
            };
            gpus.push(GpuInfo {
                id: format!("GPU-{index}"),
                name: device.name,
                memory_mb,
                sm_count: device.sm_count,
                driver_version,
                runtime_version,
            });
        }
        Ok(gpus)
    }
}

/// Host bridge entry point.
pub struct GpuBridge {
    inventories: Vec<InventoryCandidate>,
    model_registry: Option<PathBuf>,
    fixture_mode: bool,
    epoch: u64,
    sequence: AtomicU64,
}

/// Identity and freshness contract carried by every published snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSnapshotIdentity {
    /// Authenticated publisher identity (transport authentication is enforced by Cohesix).
    pub source_id: String,
    /// `fixture` for explicit test data or `production` for real/empty live state.
    pub source_mode: String,
    /// Publisher epoch; a restart creates a newer epoch.
    pub epoch: u64,
    /// Strictly increasing sequence within the epoch.
    pub sequence: u64,
    /// Host wall-clock observation time in Unix milliseconds.
    pub observed_unix_ms: u64,
    /// Maximum target retention time after validated receipt.
    pub ttl_ms: u64,
    /// Digest of the canonical model catalog identities.
    pub catalog_sha256: String,
    /// Whether the registry contains validated live/fixture model state.
    pub available: bool,
}

/// Serialised GPU topology (nodes, models, telemetry schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuNamespaceSnapshot {
    /// Publisher, freshness, and catalog identity.
    pub identity: GpuSnapshotIdentity,
    /// Per-GPU nodes.
    pub nodes: Vec<SerialisedGpuNode>,
    /// Model lifecycle metadata.
    pub models: GpuModelCatalog,
    /// Telemetry schema descriptor.
    pub telemetry_schema: TelemetrySchema,
}

impl GpuBridge {
    /// Create a bridge using the mock inventory.
    pub fn mock() -> Self {
        Self {
            inventories: vec![InventoryCandidate::new(
                InventoryBackend::Mock,
                Box::new(MockInventory),
            )],
            model_registry: None,
            fixture_mode: true,
            epoch: current_unix_ms(),
            sequence: AtomicU64::new(0),
        }
    }

    /// Create a bridge using the NVML backend when the feature is enabled.
    #[allow(clippy::new_without_default)]
    #[cfg(feature = "nvml")]
    pub fn new_nvml() -> Self {
        Self {
            inventories: vec![InventoryCandidate::new(
                InventoryBackend::Nvml,
                Box::new(NvmlInventory),
            )],
            model_registry: None,
            fixture_mode: false,
            epoch: current_unix_ms(),
            sequence: AtomicU64::new(0),
        }
    }

    /// Create a bridge using the CUDA backend when the feature is enabled.
    #[allow(clippy::new_without_default)]
    #[cfg(feature = "cuda")]
    pub fn new_cuda() -> Self {
        Self {
            inventories: vec![InventoryCandidate::new(
                InventoryBackend::Cuda,
                Box::new(CudaInventory),
            )],
            model_registry: None,
            fixture_mode: false,
            epoch: current_unix_ms(),
            sequence: AtomicU64::new(0),
        }
    }

    fn from_candidates(candidates: Vec<InventoryCandidate>) -> Self {
        Self {
            inventories: candidates,
            model_registry: None,
            fixture_mode: false,
            epoch: current_unix_ms(),
            sequence: AtomicU64::new(0),
        }
    }

    /// Attach a model registry root used to populate `/gpu/models/available`.
    #[must_use]
    pub fn with_registry_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.model_registry = Some(root.into());
        self
    }

    fn discover_inventory(&self) -> Result<(Vec<GpuInfo>, InventoryReport)> {
        let primary = self
            .inventories
            .first()
            .map(|candidate| candidate.backend)
            .ok_or_else(|| anyhow!("no GPU inventory backends configured"))?;
        let mut first_error: Option<String> = None;
        for (idx, candidate) in self.inventories.iter().enumerate() {
            match candidate.inventory.discover() {
                Ok(infos) => {
                    let report = if idx == 0 {
                        InventoryReport {
                            backend: candidate.backend,
                            fallback_from: None,
                            fallback_reason: None,
                        }
                    } else {
                        InventoryReport {
                            backend: candidate.backend,
                            fallback_from: Some(primary),
                            fallback_reason: first_error,
                        }
                    };
                    return Ok((infos, report));
                }
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err.to_string());
                    }
                    if idx + 1 == self.inventories.len() {
                        return Err(err);
                    }
                }
            }
        }
        Err(anyhow!("no GPU inventory backends configured"))
    }

    /// Discover GPUs and build namespace descriptors.
    pub fn build_namespace(&self) -> Result<Vec<GpuNamespace>> {
        let (infos, _report) = self.discover_inventory()?;
        Ok(infos
            .into_iter()
            .map(|info| GpuNamespace {
                ctl_seed: format!("LEASE {}\n", info.id),
                lease_seed: String::new(),
                status_seed: String::new(),
                info,
            })
            .collect())
    }

    fn build_namespace_with_report(&self) -> Result<(Vec<GpuNamespace>, InventoryReport)> {
        let (infos, report) = self.discover_inventory()?;
        let namespaces = infos
            .into_iter()
            .map(|info| GpuNamespace {
                ctl_seed: format!("LEASE {}\n", info.id),
                lease_seed: String::new(),
                status_seed: String::new(),
                info,
            })
            .collect();
        Ok((namespaces, report))
    }

    /// Construct JSON payloads ready for NineDoor ingestion, including models and telemetry schema.
    pub fn serialise_namespace(&self) -> Result<GpuNamespaceSnapshot> {
        self.serialise_namespace_with_report()
            .map(|(snapshot, _report)| snapshot)
    }

    /// Construct JSON payloads ready for NineDoor ingestion, including models and telemetry schema.
    pub fn serialise_namespace_with_report(
        &self,
    ) -> Result<(GpuNamespaceSnapshot, InventoryReport)> {
        let mut models = self.build_model_catalog()?;
        let telemetry_schema = TelemetrySchema::lora_v1();
        let (namespaces, report) = self.build_namespace_with_report()?;
        let source_id = format!("gpu-bridge-host/{}", report.backend.label());
        let catalog_sha256 = catalog_sha256(&models);
        let sequence = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if !models.active.is_empty() {
            let active_manifest = models
                .available
                .iter()
                .find(|manifest| manifest.model_id == models.active)
                .ok_or_else(|| anyhow!("active model is absent from validated catalog"))?;
            models.activation_generation = sequence;
            models.activation_receipt = activation_receipt(
                &source_id,
                self.epoch,
                models.activation_generation,
                &models.active,
                &active_manifest.manifest_sha256,
                &catalog_sha256,
            );
        }
        let nodes = namespaces
            .into_iter()
            .map(|namespace| {
                let info_payload = namespace.info.to_info_payload();
                let ctl_payload = namespace.ctl_seed;
                let lease_payload = namespace.lease_seed;
                let status_payload = namespace.status_seed;
                Ok(SerialisedGpuNode {
                    id: namespace.info.id,
                    info_payload,
                    ctl_payload,
                    lease_payload,
                    status_payload,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((
            GpuNamespaceSnapshot {
                identity: GpuSnapshotIdentity {
                    source_id,
                    source_mode: if self.fixture_mode {
                        "fixture".to_owned()
                    } else {
                        "production".to_owned()
                    },
                    epoch: self.epoch,
                    sequence,
                    observed_unix_ms: current_unix_ms(),
                    ttl_ms: DEFAULT_SNAPSHOT_TTL_MS,
                    catalog_sha256,
                    available: !models.available.is_empty(),
                },
                nodes,
                models,
                telemetry_schema,
            },
            report,
        ))
    }

    fn build_model_catalog(&self) -> Result<GpuModelCatalog> {
        if let Some(root) = self.model_registry.as_ref() {
            return load_registry_catalog(root)
                .map(|catalog| catalog.unwrap_or_else(empty_model_catalog));
        }
        if self.fixture_mode {
            Ok(fixture_model_catalog())
        } else {
            Ok(empty_model_catalog())
        }
    }
}

fn empty_model_catalog() -> GpuModelCatalog {
    GpuModelCatalog {
        available: Vec::new(),
        active: String::new(),
        activation_generation: 0,
        activation_receipt: String::new(),
    }
}

fn fixture_model_catalog() -> GpuModelCatalog {
    let base_cas = sha256_hex(b"fixture:vision-base-v1");
    let adapter_cas = sha256_hex(b"fixture:vision-lora-edge:adapter");
    let lora_cas = sha256_hex(b"fixture:vision-lora-edge");
    let base_manifest = format!(
        r#"[model]
id = "vision-base-v1"
cas_sha256 = "{base_cas}"
format = "gguf"

[metadata]
tokens = 4096
owner = "fixture"
activation = "cold-reload""#
    );
    let lora_manifest = format!(
        r#"[model]
id = "vision-lora-edge"
cas_sha256 = "{lora_cas}"
base = "vision-base-v1"
adapter_sha256 = "{adapter_cas}"
format = "gguf+lora"

[metadata]
tokens = 4096
owner = "fixture"
activation = "hot-swap""#
    );
    let available = vec![
        model_manifest("vision-base-v1", base_manifest, base_cas, None, None),
        model_manifest(
            "vision-lora-edge",
            lora_manifest,
            lora_cas,
            Some("vision-base-v1".to_owned()),
            Some(adapter_cas),
        ),
    ];
    GpuModelCatalog {
        active: "vision-lora-edge".into(),
        available,
        activation_generation: 0,
        activation_receipt: String::new(),
    }
}

fn model_manifest(
    model_id: &str,
    manifest_toml: String,
    cas_sha256: String,
    base_model_id: Option<String>,
    adapter_sha256: Option<String>,
) -> ModelManifest {
    let manifest_sha256 = sha256_hex(manifest_toml.as_bytes());
    ModelManifest {
        model_id: model_id.to_owned(),
        manifest_toml,
        manifest_sha256,
        cas_sha256,
        base_model_id,
        adapter_sha256,
    }
}

fn load_registry_catalog(root: &Path) -> Result<Option<GpuModelCatalog>> {
    let available_root = root.join(REGISTRY_AVAILABLE_DIR);
    if !available_root.is_dir() {
        return Ok(None);
    }
    let mut manifests = Vec::new();
    for entry in fs::read_dir(&available_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let model_id = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) if valid_registry_id(name) => name.to_owned(),
            _ => continue,
        };
        let manifest_path = path.join(REGISTRY_MANIFEST_FILE);
        if !manifest_path.is_file() {
            continue;
        }
        let manifest_bytes = read_bounded_file(&manifest_path, MAX_REGISTRY_MANIFEST_BYTES)?;
        let manifest_toml = String::from_utf8(manifest_bytes)
            .map_err(|_| anyhow!("manifest.toml for {model_id} is not UTF-8"))?;
        let document: RegistryModelDocument = toml::from_str(&manifest_toml)
            .map_err(|err| anyhow!("manifest.toml for {model_id}: {err}"))?;
        ensure!(
            document.model.id == model_id,
            "manifest model id does not match registry directory: {} != {model_id}",
            document.model.id
        );
        ensure!(
            valid_sha256(&document.model.cas_sha256),
            "manifest for {model_id} has invalid cas_sha256"
        );
        ensure!(
            !document.model.format.trim().is_empty(),
            "manifest for {model_id} has empty format"
        );
        if let Some(base) = document.model.base.as_deref() {
            ensure!(
                valid_registry_id(base),
                "manifest for {model_id} has invalid base model id"
            );
        }
        if let Some(adapter) = document.model.adapter_sha256.as_deref() {
            ensure!(
                valid_sha256(adapter),
                "manifest for {model_id} has invalid adapter_sha256"
            );
            ensure!(
                document.model.base.is_some(),
                "manifest for {model_id} declares an adapter without a base model"
            );
        }
        validate_peft_registry_extension(&model_id, &document)?;
        manifests.push(model_manifest(
            &model_id,
            manifest_toml,
            document.model.cas_sha256,
            document.model.base,
            document.model.adapter_sha256,
        ));
    }
    if manifests.is_empty() {
        return Ok(None);
    }
    manifests.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    for manifest in &manifests {
        if let Some(base) = manifest.base_model_id.as_deref() {
            ensure!(
                manifests.iter().any(|candidate| candidate.model_id == base),
                "manifest for {} references unavailable base model {base}",
                manifest.model_id
            );
        }
    }
    let active_path = root.join(REGISTRY_ACTIVE_FILE);
    let active = if active_path.is_file() {
        read_first_line(&active_path, MAX_REGISTRY_ID_BYTES)?
    } else {
        String::new()
    };
    ensure!(
        active.is_empty() || valid_registry_id(&active),
        "registry active model id is invalid"
    );
    ensure!(
        active.is_empty() || manifests.iter().any(|manifest| manifest.model_id == active),
        "registry active model id is not present in available catalog"
    );
    Ok(Some(GpuModelCatalog {
        available: manifests,
        active,
        activation_generation: 0,
        activation_receipt: String::new(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryModelDocument {
    model: RegistryModelIdentity,
    #[serde(default, rename = "metadata")]
    _metadata: Option<toml::Value>,
    #[serde(default)]
    provenance: Option<RegistryPeftProvenance>,
    #[serde(default)]
    hashes: Option<RegistryPeftHashes>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryModelIdentity {
    id: String,
    cas_sha256: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    adapter_sha256: Option<String>,
    format: String,
    #[serde(default)]
    adapter: Option<String>,
    #[serde(default)]
    lora: Option<String>,
    #[serde(default)]
    metrics: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryPeftProvenance {
    job_id: String,
    approval: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryPeftHashes {
    adapter_sha256: String,
    adapter_bytes: u64,
    lora_sha256: String,
    lora_bytes: u64,
    #[serde(default)]
    metrics_sha256: Option<String>,
    #[serde(default)]
    metrics_bytes: Option<u64>,
    policy_sha256: String,
    policy_bytes: u64,
    telemetry_sha256: String,
    telemetry_bytes: u64,
}

fn validate_peft_registry_extension(
    model_id: &str,
    document: &RegistryModelDocument,
) -> Result<()> {
    let model = &document.model;
    let extension_present = model.adapter.is_some()
        || model.lora.is_some()
        || model.metrics.is_some()
        || document.provenance.is_some()
        || document.hashes.is_some();
    if !extension_present {
        return Ok(());
    }

    ensure!(
        model.format == PEFT_MODEL_FORMAT,
        "PEFT manifest for {model_id} has unsupported format"
    );
    ensure!(
        model.adapter.as_deref() == Some(PEFT_ADAPTER_FILE),
        "PEFT manifest for {model_id} has invalid adapter path"
    );
    ensure!(
        model.lora.as_deref() == Some(PEFT_LORA_FILE),
        "PEFT manifest for {model_id} has invalid LoRA metadata path"
    );
    ensure!(
        model.base.is_some(),
        "PEFT manifest for {model_id} is missing its base model"
    );

    let provenance = document
        .provenance
        .as_ref()
        .ok_or_else(|| anyhow!("PEFT manifest for {model_id} is missing provenance"))?;
    ensure!(
        valid_registry_id(&provenance.job_id),
        "PEFT manifest for {model_id} has invalid job id"
    );
    ensure!(
        valid_registry_status(&provenance.approval),
        "PEFT manifest for {model_id} has invalid approval status"
    );

    let hashes = document
        .hashes
        .as_ref()
        .ok_or_else(|| anyhow!("PEFT manifest for {model_id} is missing artifact hashes"))?;
    for (label, digest) in [
        ("adapter", hashes.adapter_sha256.as_str()),
        ("LoRA metadata", hashes.lora_sha256.as_str()),
        ("policy", hashes.policy_sha256.as_str()),
        ("telemetry", hashes.telemetry_sha256.as_str()),
    ] {
        ensure!(
            valid_sha256(digest),
            "PEFT manifest for {model_id} has invalid {label} digest"
        );
    }
    for (label, bytes) in [
        ("adapter", hashes.adapter_bytes),
        ("LoRA metadata", hashes.lora_bytes),
        ("policy", hashes.policy_bytes),
        ("telemetry", hashes.telemetry_bytes),
    ] {
        ensure!(
            bytes > 0,
            "PEFT manifest for {model_id} has empty {label} artifact"
        );
    }

    ensure!(
        model.cas_sha256 == hashes.adapter_sha256,
        "PEFT manifest for {model_id} CAS digest does not match the adapter artifact"
    );
    ensure!(
        model.adapter_sha256.as_deref() == Some(hashes.adapter_sha256.as_str()),
        "PEFT manifest for {model_id} adapter identity mismatch"
    );

    match (
        model.metrics.as_deref(),
        hashes.metrics_sha256.as_deref(),
        hashes.metrics_bytes,
    ) {
        (None, None, None) => {}
        (Some(PEFT_METRICS_FILE), Some(digest), Some(bytes)) => {
            ensure!(
                valid_sha256(digest),
                "PEFT manifest for {model_id} has invalid metrics digest"
            );
            ensure!(
                bytes > 0,
                "PEFT manifest for {model_id} has empty metrics artifact"
            );
        }
        _ => {
            return Err(anyhow!(
                "PEFT manifest for {model_id} has inconsistent metrics identity"
            ));
        }
    }
    Ok(())
}

fn valid_registry_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REGISTRY_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_registry_status(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn catalog_sha256(catalog: &GpuModelCatalog) -> String {
    let mut hasher = Sha256::new();
    for manifest in &catalog.available {
        hasher.update(manifest.model_id.as_bytes());
        hasher.update([0]);
        hasher.update(manifest.manifest_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(manifest.cas_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(
            manifest
                .base_model_id
                .as_deref()
                .unwrap_or(EMPTY_VALUE)
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(
            manifest
                .adapter_sha256
                .as_deref()
                .unwrap_or(EMPTY_VALUE)
                .as_bytes(),
        );
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn activation_receipt(
    source_id: &str,
    epoch: u64,
    generation: u64,
    model_id: &str,
    manifest_sha256: &str,
    catalog_sha256: &str,
) -> String {
    let mut hasher = Sha256::new();
    let epoch = epoch.to_string();
    let generation = generation.to_string();
    for field in [
        source_id,
        epoch.as_str(),
        generation.as_str(),
        model_id,
        manifest_sha256,
        catalog_sha256,
    ] {
        hasher.update(field.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn current_unix_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn read_bounded_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let read = file.read(&mut tmp)?;
        if read == 0 {
            break;
        }
        if buffer.len().saturating_add(read) > max_bytes {
            return Err(anyhow!(
                "registry file {} exceeds max bytes {}",
                path.display(),
                max_bytes
            ));
        }
        buffer.extend_from_slice(&tmp[..read]);
    }
    Ok(buffer)
}

fn read_first_line(path: &Path, max_len: usize) -> Result<String> {
    let bytes = read_bounded_file(path, max_len + 1)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow!("registry file {} is not UTF-8", path.display()))?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("registry file {} is empty", path.display()))?;
    ensure!(
        line.len() <= max_len,
        "registry value exceeds max length {}",
        max_len
    );
    Ok(line.to_owned())
}

/// Serialised GPU node representation exported by the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialisedGpuNode {
    /// GPU identifier used in path segments.
    pub id: String,
    /// Contents for `/gpu/<id>/info`.
    pub info_payload: String,
    /// Contents for `/gpu/<id>/ctl`.
    pub ctl_payload: String,
    /// Contents for `/gpu/<id>/lease`.
    pub lease_payload: String,
    /// Contents for `/gpu/<id>/status`.
    pub status_payload: String,
}

/// Produce a minimal job status JSON entry.
pub fn status_entry(job: &str, state: &str, detail: &str) -> String {
    format!(
        "{{\"job\":\"{}\",\"state\":\"{}\",\"detail\":\"{}\"}}",
        escape_json_string(job),
        escape_json_string(state),
        escape_json_string(detail)
    )
}

/// Format a namespace snapshot as pretty JSON, including models and telemetry schema.
#[must_use]
pub fn namespace_to_json_pretty(snapshot: &GpuNamespaceSnapshot) -> String {
    let mut out = String::new();
    out.push_str("{\n  \"identity\": {\n");
    out.push_str(&format!(
        "    \"source_id\": \"{}\",\n    \"source_mode\": \"{}\",\n    \"epoch\": {},\n    \"sequence\": {},\n    \"observed_unix_ms\": {},\n    \"ttl_ms\": {},\n    \"catalog_sha256\": \"{}\",\n    \"available\": {}\n  }},\n",
        escape_json_string(&snapshot.identity.source_id),
        escape_json_string(&snapshot.identity.source_mode),
        snapshot.identity.epoch,
        snapshot.identity.sequence,
        snapshot.identity.observed_unix_ms,
        snapshot.identity.ttl_ms,
        snapshot.identity.catalog_sha256,
        snapshot.identity.available,
    ));
    out.push_str("  \"nodes\": [\n");
    for (index, node) in snapshot.nodes.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"id\": \"{}\",\n",
            escape_json_string(&node.id)
        ));
        out.push_str(&format!(
            "      \"info_payload\": \"{}\",\n",
            escape_json_string(&node.info_payload)
        ));
        out.push_str(&format!(
            "      \"ctl_payload\": \"{}\",\n",
            escape_json_string(&node.ctl_payload)
        ));
        out.push_str(&format!(
            "      \"lease_payload\": \"{}\",\n",
            escape_json_string(&node.lease_payload)
        ));
        out.push_str(&format!(
            "      \"status_payload\": \"{}\"\n",
            escape_json_string(&node.status_payload)
        ));
        out.push_str("    }");
    }
    out.push_str("\n  ],\n");
    out.push_str("  \"models\": {\n");
    out.push_str(&format!(
        "    \"active\": \"{}\",\n",
        escape_json_string(&snapshot.models.active)
    ));
    out.push_str("    \"available\": [\n");
    for (index, manifest) in snapshot.models.available.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("      {\n");
        out.push_str(&format!(
            "        \"model_id\": \"{}\",\n",
            escape_json_string(&manifest.model_id)
        ));
        out.push_str(&format!(
            "        \"manifest_toml\": \"{}\",\n        \"manifest_sha256\": \"{}\",\n        \"cas_sha256\": \"{}\",\n        \"base_model_id\": {},\n        \"adapter_sha256\": {}\n",
            escape_json_string(&manifest.manifest_toml),
            manifest.manifest_sha256,
            manifest.cas_sha256,
            manifest
                .base_model_id
                .as_deref()
                .map(|value| format!("\"{}\"", escape_json_string(value)))
                .unwrap_or_else(|| "null".to_owned()),
            manifest
                .adapter_sha256
                .as_deref()
                .map(|value| format!("\"{}\"", escape_json_string(value)))
                .unwrap_or_else(|| "null".to_owned()),
        ));
        out.push_str("      }");
    }
    out.push_str(&format!(
        "\n    ],\n    \"activation_generation\": {},\n    \"activation_receipt\": \"{}\"\n  }},\n",
        snapshot.models.activation_generation, snapshot.models.activation_receipt,
    ));
    out.push_str(&format!(
        "  \"telemetry_schema\": {}\n",
        snapshot.telemetry_schema.descriptor_json()
    ));
    out.push('}');
    out
}

/// Snapshot publish envelope for `/gpu/bridge/ctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuBridgePublish {
    /// Wire-format snapshot bytes.
    pub bytes: Vec<u8>,
    /// SHA-256 of the wire payload.
    pub sha256: String,
    /// Line payloads (each <= max echo len) to send to `/gpu/bridge/ctl`.
    pub lines: Vec<String>,
}

/// Format a namespace snapshot as a compact wire payload for publish.
#[must_use]
pub fn namespace_to_wire(snapshot: &GpuNamespaceSnapshot) -> Vec<u8> {
    let mut out = String::new();
    let _ = writeln!(out, "schema {GPU_BRIDGE_WIRE_SCHEMA}");
    let _ = writeln!(
        out,
        "snapshot source={} mode={} epoch={} sequence={} observed_unix_ms={} ttl_ms={} catalog_sha256={} available={}",
        snapshot.identity.source_id,
        snapshot.identity.source_mode,
        snapshot.identity.epoch,
        snapshot.identity.sequence,
        snapshot.identity.observed_unix_ms,
        snapshot.identity.ttl_ms,
        snapshot.identity.catalog_sha256,
        u8::from(snapshot.identity.available),
    );
    for node in &snapshot.nodes {
        let info = BASE64_STANDARD.encode(node.info_payload.as_bytes());
        let ctl = BASE64_STANDARD.encode(node.ctl_payload.as_bytes());
        let lease = BASE64_STANDARD.encode(node.lease_payload.as_bytes());
        let status = BASE64_STANDARD.encode(node.status_payload.as_bytes());
        let _ = writeln!(
            out,
            "node id={} info={} ctl={} lease={} status={}",
            node.id, info, ctl, lease, status
        );
    }
    for manifest in &snapshot.models.available {
        let manifest_b64 = BASE64_STANDARD.encode(manifest.manifest_toml.as_bytes());
        let _ = writeln!(
            out,
            "model id={} manifest={} manifest_sha256={} cas_sha256={} base={} adapter_sha256={}",
            manifest.model_id,
            manifest_b64,
            manifest.manifest_sha256,
            manifest.cas_sha256,
            manifest.base_model_id.as_deref().unwrap_or(EMPTY_VALUE),
            manifest.adapter_sha256.as_deref().unwrap_or(EMPTY_VALUE),
        );
    }
    let active_manifest_sha256 = snapshot
        .models
        .available
        .iter()
        .find(|manifest| manifest.model_id == snapshot.models.active)
        .map(|manifest| manifest.manifest_sha256.as_str())
        .unwrap_or(EMPTY_VALUE);
    let _ = writeln!(
        out,
        "active id={} generation={} receipt={} manifest_sha256={}",
        if snapshot.models.active.is_empty() {
            EMPTY_VALUE
        } else {
            snapshot.models.active.as_str()
        },
        snapshot.models.activation_generation,
        if snapshot.models.activation_receipt.is_empty() {
            EMPTY_VALUE
        } else {
            snapshot.models.activation_receipt.as_str()
        },
        active_manifest_sha256,
    );
    let schema_b64 = BASE64_STANDARD.encode(snapshot.telemetry_schema.descriptor_json().as_bytes());
    let _ = writeln!(out, "telemetry schema={schema_b64}");
    let _ = writeln!(out, "end");
    out.into_bytes()
}

/// Build publish lines for `/gpu/bridge/ctl` using the default echo limit.
pub fn build_publish_lines(snapshot: &GpuNamespaceSnapshot) -> Result<GpuBridgePublish> {
    build_publish_lines_with_limit(snapshot, MAX_ECHO_LEN)
}

/// Build publish lines for `/gpu/bridge/ctl` with a custom echo payload limit.
pub fn build_publish_lines_with_limit(
    snapshot: &GpuNamespaceSnapshot,
    max_echo_len: usize,
) -> Result<GpuBridgePublish> {
    ensure!(max_echo_len >= 8, "max echo len too small");
    let bytes = namespace_to_wire(snapshot);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = hex::encode(hasher.finalize());
    let mut lines = Vec::new();
    lines.push(format!("begin bytes={} sha256={}", bytes.len(), sha256));

    let encoded = BASE64_STANDARD.encode(&bytes);
    let chunk_len = ((max_echo_len.saturating_sub(GPU_BRIDGE_B64_PREFIX.len())) / 4) * 4;
    ensure!(chunk_len >= 4, "max echo len too small for base64 chunks");
    for chunk in encoded.as_bytes().chunks(chunk_len) {
        let chunk_str =
            core::str::from_utf8(chunk).map_err(|_| anyhow!("base64 chunk is not valid UTF-8"))?;
        lines.push(format!("{GPU_BRIDGE_B64_PREFIX}{chunk_str}"));
    }
    lines.push("end".to_owned());
    Ok(GpuBridgePublish {
        bytes,
        sha256,
        lines,
    })
}

/// Parse a wire-format GPU namespace snapshot.
pub fn parse_wire_snapshot(bytes: &[u8]) -> Result<GpuNamespaceSnapshot> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| anyhow!("wire payload must be UTF-8 text"))?;
    let mut schema_seen = false;
    let mut identity: Option<GpuSnapshotIdentity> = None;
    let mut nodes = Vec::new();
    let mut models = Vec::new();
    let mut active: Option<String> = None;
    let mut active_contract: Option<(u64, String, String)> = None;
    let mut telemetry_schema: Option<TelemetrySchema> = None;
    let mut ended = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if ended {
            return Err(anyhow!("wire payload contains data after end marker"));
        }
        if line == "end" {
            ended = true;
            continue;
        }
        let mut parts = line.split_whitespace();
        let keyword = parts
            .next()
            .ok_or_else(|| anyhow!("wire line missing keyword"))?;
        match keyword {
            "schema" => {
                let schema = parts
                    .next()
                    .ok_or_else(|| anyhow!("schema missing value"))?;
                if schema != GPU_BRIDGE_WIRE_SCHEMA {
                    return Err(anyhow!("unsupported wire schema: {schema}"));
                }
                if parts.next().is_some() {
                    return Err(anyhow!("schema line has unexpected tokens"));
                }
                schema_seen = true;
            }
            "snapshot" => {
                ensure!(identity.is_none(), "duplicate snapshot identity line");
                let mut source_id = None;
                let mut source_mode = None;
                let mut epoch = None;
                let mut sequence = None;
                let mut observed_unix_ms = None;
                let mut ttl_ms = None;
                let mut catalog_sha256 = None;
                let mut available = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow!("snapshot field missing '=': {part}"))?;
                    match key {
                        "source" => source_id = Some(value),
                        "mode" => source_mode = Some(value),
                        "epoch" => epoch = Some(parse_positive_u64("epoch", value)?),
                        "sequence" => sequence = Some(parse_positive_u64("sequence", value)?),
                        "observed_unix_ms" => {
                            observed_unix_ms = Some(parse_positive_u64("observed_unix_ms", value)?)
                        }
                        "ttl_ms" => ttl_ms = Some(parse_positive_u64("ttl_ms", value)?),
                        "catalog_sha256" => catalog_sha256 = Some(value),
                        "available" => {
                            available = Some(match value {
                                "0" => false,
                                "1" => true,
                                _ => return Err(anyhow!("snapshot available must be 0 or 1")),
                            })
                        }
                        _ => return Err(anyhow!("unsupported snapshot field: {key}")),
                    }
                }
                let source_id = source_id.ok_or_else(|| anyhow!("snapshot source missing"))?;
                ensure!(valid_source_id(source_id), "snapshot source id is invalid");
                let source_mode = source_mode.ok_or_else(|| anyhow!("snapshot mode missing"))?;
                ensure!(
                    matches!(source_mode, "production" | "fixture"),
                    "snapshot mode is invalid"
                );
                let ttl_ms = ttl_ms.ok_or_else(|| anyhow!("snapshot ttl missing"))?;
                ensure!(
                    ttl_ms <= MAX_SNAPSHOT_TTL_MS,
                    "snapshot ttl exceeds maximum"
                );
                let catalog_sha256 =
                    catalog_sha256.ok_or_else(|| anyhow!("snapshot catalog_sha256 missing"))?;
                ensure!(
                    valid_sha256(catalog_sha256),
                    "snapshot catalog digest is invalid"
                );
                identity = Some(GpuSnapshotIdentity {
                    source_id: source_id.to_owned(),
                    source_mode: source_mode.to_owned(),
                    epoch: epoch.ok_or_else(|| anyhow!("snapshot epoch missing"))?,
                    sequence: sequence.ok_or_else(|| anyhow!("snapshot sequence missing"))?,
                    observed_unix_ms: observed_unix_ms
                        .ok_or_else(|| anyhow!("snapshot observation time missing"))?,
                    ttl_ms,
                    catalog_sha256: catalog_sha256.to_owned(),
                    available: available.ok_or_else(|| anyhow!("snapshot availability missing"))?,
                });
            }
            "node" => {
                let mut id = None;
                let mut info = None;
                let mut ctl = None;
                let mut lease = None;
                let mut status = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow!("node field missing '=': {part}"))?;
                    match key {
                        "id" => id = Some(value),
                        "info" => info = Some(value),
                        "ctl" => ctl = Some(value),
                        "lease" => lease = Some(value),
                        "status" => status = Some(value),
                        _ => return Err(anyhow!("unsupported node field: {key}")),
                    }
                }
                let id = id.ok_or_else(|| anyhow!("node id missing"))?;
                let info_payload = decode_b64_string("node info", info)?;
                let ctl_payload = decode_b64_string("node ctl", ctl)?;
                let lease_payload = decode_b64_string("node lease", lease)?;
                let status_payload = decode_b64_string("node status", status)?;
                nodes.push(SerialisedGpuNode {
                    id: id.to_owned(),
                    info_payload,
                    ctl_payload,
                    lease_payload,
                    status_payload,
                });
            }
            "model" => {
                let mut id = None;
                let mut manifest = None;
                let mut manifest_sha256 = None;
                let mut cas_sha256 = None;
                let mut base_model_id = None;
                let mut adapter_sha256 = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow!("model field missing '=': {part}"))?;
                    match key {
                        "id" => id = Some(value),
                        "manifest" => manifest = Some(value),
                        "manifest_sha256" => manifest_sha256 = Some(value),
                        "cas_sha256" => cas_sha256 = Some(value),
                        "base" => base_model_id = Some(value),
                        "adapter_sha256" => adapter_sha256 = Some(value),
                        _ => return Err(anyhow!("unsupported model field: {key}")),
                    }
                }
                let id = id.ok_or_else(|| anyhow!("model id missing"))?;
                ensure!(valid_registry_id(id), "model id is invalid");
                let manifest_toml = decode_b64_string("model manifest", manifest)?;
                let manifest_sha256 =
                    manifest_sha256.ok_or_else(|| anyhow!("model manifest_sha256 missing"))?;
                ensure!(
                    valid_sha256(manifest_sha256)
                        && sha256_hex(manifest_toml.as_bytes()) == manifest_sha256,
                    "model manifest digest mismatch"
                );
                let cas_sha256 = cas_sha256.ok_or_else(|| anyhow!("model cas_sha256 missing"))?;
                ensure!(valid_sha256(cas_sha256), "model CAS digest is invalid");
                let base_model_id = decode_optional_wire_value(
                    "base model id",
                    base_model_id.ok_or_else(|| anyhow!("model base missing"))?,
                    valid_registry_id,
                )?;
                let adapter_sha256 = decode_optional_wire_value(
                    "adapter digest",
                    adapter_sha256.ok_or_else(|| anyhow!("model adapter digest missing"))?,
                    valid_sha256,
                )?;
                ensure!(
                    adapter_sha256.is_none() || base_model_id.is_some(),
                    "adapter model is missing base identity"
                );
                models.push(ModelManifest {
                    model_id: id.to_owned(),
                    manifest_toml,
                    manifest_sha256: manifest_sha256.to_owned(),
                    cas_sha256: cas_sha256.to_owned(),
                    base_model_id,
                    adapter_sha256,
                });
            }
            "active" => {
                let mut id = None;
                let mut generation = None;
                let mut receipt = None;
                let mut manifest_sha256 = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow!("active field missing '=': {part}"))?;
                    match key {
                        "id" => id = Some(value),
                        "generation" => {
                            generation = Some(value.parse::<u64>().map_err(|_| {
                                anyhow!("active generation is not an unsigned integer")
                            })?)
                        }
                        "receipt" => receipt = Some(value),
                        "manifest_sha256" => manifest_sha256 = Some(value),
                        _ => return Err(anyhow!("unsupported active field: {key}")),
                    }
                }
                let id = id.ok_or_else(|| anyhow!("active id missing"))?;
                let generation = generation.ok_or_else(|| anyhow!("active generation missing"))?;
                let receipt = receipt.ok_or_else(|| anyhow!("active receipt missing"))?;
                let manifest_sha256 =
                    manifest_sha256.ok_or_else(|| anyhow!("active manifest_sha256 missing"))?;
                if id == EMPTY_VALUE {
                    ensure!(
                        generation == 0 && receipt == EMPTY_VALUE && manifest_sha256 == EMPTY_VALUE,
                        "empty active model must not carry activation evidence"
                    );
                    active = Some(String::new());
                } else {
                    ensure!(valid_registry_id(id), "active model id is invalid");
                    ensure!(generation > 0, "active generation must be positive");
                    ensure!(valid_sha256(receipt), "active receipt is invalid");
                    ensure!(
                        valid_sha256(manifest_sha256),
                        "active manifest digest is invalid"
                    );
                    active = Some(id.to_owned());
                }
                active_contract =
                    Some((generation, receipt.to_owned(), manifest_sha256.to_owned()));
            }
            "telemetry" => {
                let mut schema = None;
                for part in parts {
                    let (key, value) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow!("telemetry field missing '=': {part}"))?;
                    match key {
                        "schema" => schema = Some(value),
                        _ => return Err(anyhow!("unsupported telemetry field: {key}")),
                    }
                }
                let schema_b64 = schema.ok_or_else(|| anyhow!("telemetry schema missing"))?;
                let schema_json = decode_b64_string("telemetry schema", Some(schema_b64))?;
                telemetry_schema = Some(parse_telemetry_schema(&schema_json)?);
            }
            _ => return Err(anyhow!("unsupported wire line: {keyword}")),
        }
    }

    if !schema_seen {
        return Err(anyhow!("wire payload missing schema line"));
    }
    if !ended {
        return Err(anyhow!("wire payload missing end marker"));
    }
    let identity = identity.ok_or_else(|| anyhow!("wire payload missing snapshot identity"))?;
    ensure!(
        identity.available == !models.is_empty(),
        "snapshot availability disagrees with catalog"
    );
    ensure!(
        catalog_sha256(&GpuModelCatalog {
            available: models.clone(),
            active: String::new(),
            activation_generation: 0,
            activation_receipt: String::new(),
        }) == identity.catalog_sha256,
        "snapshot catalog digest mismatch"
    );
    for model in &models {
        if let Some(base) = model.base_model_id.as_deref() {
            ensure!(
                models.iter().any(|candidate| candidate.model_id == base),
                "model references unavailable base model"
            );
        }
    }
    let active = active.ok_or_else(|| anyhow!("wire payload missing active model id"))?;
    let (activation_generation, activation_receipt_value, active_manifest_sha256) =
        active_contract.ok_or_else(|| anyhow!("wire payload missing active contract"))?;
    if !active.is_empty() && !models.iter().any(|entry| entry.model_id == active) {
        return Err(anyhow!("active model id not found in available catalog"));
    }
    if !active.is_empty() {
        let manifest = models
            .iter()
            .find(|entry| entry.model_id == active)
            .ok_or_else(|| anyhow!("active model id not found"))?;
        ensure!(
            manifest.manifest_sha256 == active_manifest_sha256,
            "active model manifest identity mismatch"
        );
        ensure!(
            activation_receipt(
                &identity.source_id,
                identity.epoch,
                activation_generation,
                &active,
                &manifest.manifest_sha256,
                &identity.catalog_sha256,
            ) == activation_receipt_value,
            "active model receipt mismatch"
        );
    }
    let telemetry_schema =
        telemetry_schema.ok_or_else(|| anyhow!("wire payload missing telemetry schema"))?;
    Ok(GpuNamespaceSnapshot {
        identity,
        nodes,
        models: GpuModelCatalog {
            available: models,
            active,
            activation_generation,
            activation_receipt: activation_receipt_value,
        },
        telemetry_schema,
    })
}

fn decode_b64_string(label: &str, value: Option<&str>) -> Result<String> {
    let value = value.ok_or_else(|| anyhow!("{label} missing"))?;
    let bytes = BASE64_STANDARD
        .decode(value.as_bytes())
        .map_err(|_| anyhow!("{label} is not valid base64"))?;
    String::from_utf8(bytes).map_err(|_| anyhow!("{label} is not UTF-8"))
}

fn parse_positive_u64(label: &str, value: &str) -> Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("{label} is not an unsigned integer"))?;
    ensure!(parsed > 0, "{label} must be positive");
    Ok(parsed)
}

fn decode_optional_wire_value(
    label: &str,
    value: &str,
    validate: impl FnOnce(&str) -> bool,
) -> Result<Option<String>> {
    if value == EMPTY_VALUE {
        return Ok(None);
    }
    ensure!(validate(value), "{label} is invalid");
    Ok(Some(value.to_owned()))
}

fn valid_source_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REGISTRY_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

#[derive(Debug, Deserialize)]
struct TelemetrySchemaJson {
    schema_version: String,
    max_record_bytes: usize,
    required_fields: Vec<String>,
    optional_fields: Vec<String>,
}

fn parse_telemetry_schema(payload: &str) -> Result<TelemetrySchema> {
    let schema: TelemetrySchemaJson =
        serde_json::from_str(payload).map_err(|err| anyhow!("telemetry schema json: {err}"))?;
    if schema.schema_version.is_empty() {
        return Err(anyhow!("telemetry schema version missing"));
    }
    if schema.max_record_bytes == 0 {
        return Err(anyhow!("telemetry schema max_record_bytes must be > 0"));
    }
    Ok(TelemetrySchema {
        version: schema.schema_version,
        max_record_bytes: schema.max_record_bytes,
        required_fields: schema.required_fields,
        optional_fields: schema.optional_fields,
    })
}

fn escape_json_string(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c < ' ' => {
                write!(escaped, "\\u{:04x}", c as u32).expect("write to string");
            }
            c => escaped.push(c),
        }
    }
    escaped
}

/// Build a bridge instance with the preferred backend.
pub fn auto_bridge(mock: bool) -> Result<GpuBridge> {
    if mock {
        Ok(GpuBridge::mock())
    } else {
        let candidates = vec![
            #[cfg(feature = "nvml")]
            InventoryCandidate::new(InventoryBackend::Nvml, Box::new(NvmlInventory)),
            #[cfg(feature = "cuda")]
            InventoryCandidate::new(InventoryBackend::Cuda, Box::new(CudaInventory)),
        ];
        if candidates.is_empty() {
            return Err(anyhow!(
                "GPU inventory backends disabled; rebuild gpu-bridge-host with --features nvml or cuda, or use --mock"
            ));
        }
        Ok(GpuBridge::from_candidates(candidates))
    }
}

/// Build a bridge instance with an optional registry root override.
pub fn auto_bridge_with_registry(mock: bool, registry_root: Option<&Path>) -> Result<GpuBridge> {
    let bridge = auto_bridge(mock)?;
    Ok(match registry_root {
        Some(root) => bridge.with_registry_root(root.to_path_buf()),
        None => bridge,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn registry_manifest(model_id: &str) -> String {
        format!(
            "[model]\nid = \"{model_id}\"\ncas_sha256 = \"{}\"\nformat = \"gguf\"\n",
            sha256_hex(format!("cas:{model_id}").as_bytes())
        )
    }

    fn peft_registry_manifest(adapter_sha256: &str) -> String {
        format!(
            r#"[model]
id = "model-lora"
cas_sha256 = "{SHA_A}"
base = "model-base"
adapter_sha256 = "{SHA_A}"
format = "safetensors+lora"
adapter = "adapter.safetensors"
lora = "lora.json"

[provenance]
job_id = "job-1"
approval = "pending"

[hashes]
adapter_sha256 = "{adapter_sha256}"
adapter_bytes = 12
lora_sha256 = "{SHA_B}"
lora_bytes = 10
policy_sha256 = "{SHA_C}"
policy_bytes = 24
telemetry_sha256 = "{SHA_B}"
telemetry_bytes = 13
"#
        )
    }

    fn write_registry_manifest(root: &Path, model_id: &str, manifest: String) {
        let model_dir = root.join("available").join(model_id);
        std::fs::create_dir_all(&model_dir).expect("create model directory");
        std::fs::write(model_dir.join("manifest.toml"), manifest).expect("write model manifest");
    }

    #[test]
    fn mock_inventory_produces_namespace() {
        let bridge = GpuBridge::mock();
        let snapshot = bridge.serialise_namespace().unwrap();
        assert_eq!(snapshot.nodes.len(), 2);
        assert!(snapshot.nodes[0].info_payload.contains("GPU-0"));
        assert_eq!(snapshot.models.active, "vision-lora-edge");
        assert_eq!(snapshot.telemetry_schema.version, TELEMETRY_SCHEMA_VERSION);
    }

    #[test]
    fn mock_inventory_reports_backend() {
        let bridge = GpuBridge::mock();
        let (_snapshot, report) = bridge.serialise_namespace_with_report().unwrap();
        assert_eq!(report.backend, InventoryBackend::Mock);
        assert!(report.fallback_from.is_none());
    }

    #[test]
    fn live_mode_without_registry_publishes_empty_model_state() {
        let bridge = GpuBridge::from_candidates(vec![InventoryCandidate::new(
            InventoryBackend::Mock,
            Box::new(MockInventory),
        )]);
        let snapshot = bridge.serialise_namespace().expect("live snapshot");
        assert!(snapshot.models.available.is_empty());
        assert!(snapshot.models.active.is_empty());
    }

    #[test]
    fn registry_never_selects_first_model_implicitly() {
        let temp = tempfile::tempdir().expect("registry tempdir");
        let model_dir = temp.path().join("available/model-a");
        std::fs::create_dir_all(&model_dir).expect("create model directory");
        std::fs::write(
            model_dir.join("manifest.toml"),
            registry_manifest("model-a"),
        )
        .expect("write model manifest");
        let catalog = load_registry_catalog(temp.path())
            .expect("load registry")
            .expect("catalog");
        assert_eq!(catalog.available.len(), 1);
        assert!(catalog.active.is_empty());
    }

    #[test]
    fn stale_registry_active_model_is_rejected() {
        let temp = tempfile::tempdir().expect("registry tempdir");
        let model_dir = temp.path().join("available/model-a");
        std::fs::create_dir_all(&model_dir).expect("create model directory");
        std::fs::write(
            model_dir.join("manifest.toml"),
            registry_manifest("model-a"),
        )
        .expect("write model manifest");
        std::fs::write(temp.path().join("active"), "missing-model\n")
            .expect("write active pointer");
        let err = load_registry_catalog(temp.path()).expect_err("stale active must fail");
        assert!(err.to_string().contains("not present"));
    }

    #[test]
    fn registry_accepts_strict_coh_peft_manifest() {
        let temp = tempfile::tempdir().expect("registry tempdir");
        write_registry_manifest(temp.path(), "model-base", registry_manifest("model-base"));
        write_registry_manifest(temp.path(), "model-lora", peft_registry_manifest(SHA_A));

        let catalog = load_registry_catalog(temp.path())
            .expect("load registry")
            .expect("catalog");
        let model = catalog
            .available
            .iter()
            .find(|model| model.model_id == "model-lora")
            .expect("PEFT model");
        assert_eq!(model.cas_sha256, SHA_A);
        assert_eq!(model.adapter_sha256.as_deref(), Some(SHA_A));
        assert_eq!(model.base_model_id.as_deref(), Some("model-base"));
    }

    #[test]
    fn registry_rejects_peft_adapter_identity_mismatch() {
        let temp = tempfile::tempdir().expect("registry tempdir");
        write_registry_manifest(temp.path(), "model-base", registry_manifest("model-base"));
        write_registry_manifest(temp.path(), "model-lora", peft_registry_manifest(SHA_C));

        let error = load_registry_catalog(temp.path()).expect_err("mismatch must fail");
        assert!(error.to_string().contains("CAS digest"));
    }

    #[test]
    fn status_entry_serialises_fields() {
        let entry = status_entry("job\"1", "running", "line\nfeed");
        assert_eq!(
            entry,
            "{\"job\":\"job\\\"1\",\"state\":\"running\",\"detail\":\"line\\nfeed\"}"
        );
    }

    #[test]
    fn escape_json_string_handles_control_chars() {
        let escaped = escape_json_string("\u{0007}\"\\");
        assert_eq!(escaped, "\\u0007\\\"\\\\");
    }

    #[test]
    fn namespace_serialises_to_pretty_json() {
        let snapshot = GpuBridge::mock()
            .serialise_namespace()
            .expect("fixture snapshot");
        let json = namespace_to_json_pretty(&snapshot);
        assert!(
            json.contains("\"telemetry_schema\""),
            "telemetry schema missing: {json}"
        );
        assert!(json.contains("\"active\": \"vision-lora-edge\""));
        assert!(json.contains("\"ctl_payload\": \"LEASE GPU-0\\n\""));
        assert!(json.contains("\"lease_payload\": \"\""));
    }

    #[test]
    fn snapshot_wire_round_trip_preserves_identity_and_receipt() {
        let snapshot = GpuBridge::mock()
            .serialise_namespace()
            .expect("fixture snapshot");
        let decoded = parse_wire_snapshot(&namespace_to_wire(&snapshot)).expect("valid wire");
        assert_eq!(decoded, snapshot);
        assert!(valid_sha256(&decoded.models.activation_receipt));
    }

    #[test]
    fn snapshot_rejects_catalog_identity_tamper() {
        let snapshot = GpuBridge::mock()
            .serialise_namespace()
            .expect("fixture snapshot");
        let mut wire = String::from_utf8(namespace_to_wire(&snapshot)).expect("wire text");
        wire = wire.replacen("catalog_sha256=", "catalog_sha256=0", 1);
        assert!(parse_wire_snapshot(wire.as_bytes()).is_err());
    }

    #[test]
    fn telemetry_record_respects_size_limits() {
        let schema = TelemetrySchema::lora_v1();
        let record = TelemetryRecord {
            device_id: "dev-1".into(),
            model_id: "vision-base-v1".into(),
            lora_id: Some("adapter-a".into()),
            time_window: "2025-01-01T00:00:00Z/2025-01-01T00:05:00Z".into(),
            token_count: 1024,
            latency_histogram: vec![1, 2, 3],
            confidence: Some(0.98),
            entropy: None,
            drift: None,
            feedback_flags: Some("hf:pos".into()),
        };
        let encoded = record.to_json(&schema).expect("encode");
        assert!(encoded.len() <= schema.max_record_bytes);
    }
}
