// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-side GPU bridge utilities for Cohesix, including mock/NVML discovery,
// Author: Lukas Bower
// namespace serialisation, and telemetry/model lifecycle helpers.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-side GPU bridge utilities. The bridge discovers GPUs (mocked by
//! default) and materialises namespace entries that NineDoor can expose via the
//! `/gpu` mount. When built with the `nvml` feature the bridge performs real
//! discovery through `nvml-wrapper`.

use anyhow::{anyhow, ensure, Result};
use std::fmt::Write;

const TELEMETRY_SCHEMA_VERSION: &str = "gpu-telemetry/v1";
const MAX_TELEMETRY_BYTES: usize = 4096;

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
}

/// Host-side model catalog with an active pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuModelCatalog {
    /// Available models exported into `/gpu/models/available`.
    pub available: Vec<ModelManifest>,
    /// Active model identifier referenced by `/gpu/models/active`.
    pub active: String,
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
        use nvml_wrapper::NVML;
        let nvml = NVML::init()?;
        let device_count = nvml.device_count()?;
        let mut gpus = Vec::new();
        for index in 0..device_count {
            let device = nvml.device_by_index(index)?;
            let memory = device.memory_info()?;
            let info = GpuInfo {
                id: format!("GPU-{index}"),
                name: device.name()?.to_string(),
                memory_mb: (memory.total / (1024 * 1024)) as u32,
                sm_count: device.multiprocessor_count()? as u32,
                driver_version: nvml.sys_driver_version()?.to_string(),
                runtime_version: nvml.sys_cuda_version()?.to_string(),
            };
            gpus.push(info);
        }
        Ok(gpus)
    }
}

/// Host bridge entry point.
pub struct GpuBridge {
    inventory: Box<dyn Inventory + Send + Sync>,
}

/// Serialised GPU topology (nodes, models, telemetry schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuNamespaceSnapshot {
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
            inventory: Box::new(MockInventory),
        }
    }

    /// Create a bridge using the NVML backend when the feature is enabled.
    #[allow(clippy::new_without_default)]
    #[cfg(feature = "nvml")]
    pub fn new_nvml() -> Self {
        Self {
            inventory: Box::new(NvmlInventory::default()),
        }
    }

    /// Discover GPUs and build namespace descriptors.
    pub fn build_namespace(&self) -> Result<Vec<GpuNamespace>> {
        let infos = self.inventory.discover()?;
        Ok(infos
            .into_iter()
            .map(|info| GpuNamespace {
                ctl_seed: format!("LEASE {}\n", info.id),
                status_seed: String::new(),
                info,
            })
            .collect())
    }

    /// Construct JSON payloads ready for NineDoor ingestion, including models and telemetry schema.
    pub fn serialise_namespace(&self) -> Result<GpuNamespaceSnapshot> {
        let models = self.build_model_catalog();
        let telemetry_schema = TelemetrySchema::lora_v1();
        self.build_namespace()?
            .into_iter()
            .map(|namespace| {
                let info_payload = namespace.info.to_info_payload();
                let ctl_payload = namespace.ctl_seed;
                let status_payload = namespace.status_seed;
                Ok(SerialisedGpuNode {
                    id: namespace.info.id,
                    info_payload,
                    ctl_payload,
                    status_payload,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|nodes| GpuNamespaceSnapshot {
                nodes,
                models,
                telemetry_schema,
            })
    }

    fn build_model_catalog(&self) -> GpuModelCatalog {
        let available = vec![
            ModelManifest {
                model_id: "vision-base-v1".into(),
                manifest_toml: r#"
[model]
id = "vision-base-v1"
source = "s3://artifacts/models/vision-base-v1/"
format = "gguf"

[metadata]
tokens = 4096
owner = "mlops"
activation = "cold-reload"
"#
                .trim()
                .to_string(),
            },
            ModelManifest {
                model_id: "vision-lora-edge".into(),
                manifest_toml: r#"
[model]
id = "vision-lora-edge"
base = "vision-base-v1"
lora = "s3://artifacts/models/lora/edge-pack-01/"
format = "gguf+lora"

[metadata]
tokens = 4096
owner = "mlops"
activation = "hot-swap"
"#
                .trim()
                .to_string(),
            },
        ];
        GpuModelCatalog {
            active: "vision-lora-edge".into(),
            available,
        }
    }
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
    out.push_str("{\n  \"nodes\": [\n");
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
            "        \"manifest_toml\": \"{}\"\n",
            escape_json_string(&manifest.manifest_toml)
        ));
        out.push_str("      }");
    }
    out.push_str("\n    ]\n  },\n");
    out.push_str(&format!(
        "  \"telemetry_schema\": {}\n",
        snapshot.telemetry_schema.descriptor_json()
    ));
    out.push('}');
    out
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
        #[cfg(feature = "nvml")]
        {
            Ok(GpuBridge::new_nvml())
        }
        #[cfg(not(feature = "nvml"))]
        {
            Err(anyhow!(
                "nvml feature disabled; rebuild gpu-bridge-host with --features nvml or use --mock"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let snapshot = GpuNamespaceSnapshot {
            nodes: vec![SerialisedGpuNode {
                id: "GPU-0".into(),
                info_payload: "{\"id\":\"GPU-0\"}".into(),
                ctl_payload: "LEASE GPU-0".into(),
                status_payload: "ready".into(),
            }],
            models: GpuModelCatalog {
                available: vec![ModelManifest {
                    model_id: "foo".into(),
                    manifest_toml: "base = \"foo\"".into(),
                }],
                active: "foo".into(),
            },
            telemetry_schema: TelemetrySchema::lora_v1(),
        };
        let json = namespace_to_json_pretty(&snapshot);
        assert!(
            json.contains("\"telemetry_schema\""),
            "telemetry schema missing: {json}"
        );
        assert!(json.contains("\"active\": \"foo\""));
        assert!(json.contains("\"ctl_payload\": \"LEASE GPU-0\""));
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
