// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-side GPU bridge utilities. The bridge discovers GPUs (mocked by
//! default) and materialises namespace entries that NineDoor can expose via the
//! `/gpu` mount. When built with the `nvml` feature the bridge performs real
//! discovery through `nvml-wrapper`.

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::json;

/// Summary information about a GPU surfaced to the VM namespace.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
        serde_json::to_string_pretty(self).expect("serialize gpu info")
    }
}

/// Namespace representation created by the bridge for each GPU.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

impl GpuBridge {
    /// Create a bridge using the mock inventory.
    pub fn mock() -> Self {
        Self {
            inventory: Box::new(MockInventory::default()),
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

    /// Construct JSON payloads ready for NineDoor ingestion.
    pub fn serialise_namespace(&self) -> Result<Vec<SerialisedGpuNode>> {
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
            .collect()
    }
}

/// Serialised GPU node representation exported by the bridge.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    json!({
        "job": job,
        "state": state,
        "detail": detail,
    })
    .to_string()
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
        let nodes = bridge.build_namespace().unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].info_payload().contains("GPU-0"));
    }
}
