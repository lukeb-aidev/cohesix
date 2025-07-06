// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.16
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::telemetry::core::{emit_kv, GpuTelemetry};
use ninep::client::TcpClient;
use std::fs;

/// Simple executor that forwards CUDA jobs to a remote Secure9P server.
pub struct CudaExecutor {
    addr: String,
}

impl CudaExecutor {
    /// Create a new executor using the address stored in `/srv/cuda` or the
    /// `CUDA_SERVER` environment variable.
    pub fn new() -> Self {
        let addr = std::env::var("CUDA_SERVER")
            .or_else(|_| fs::read_to_string("/srv/cuda"))
            .unwrap_or_default();
        Self { addr: addr.trim().to_string() }
    }

    /// Loading a kernel is a no-op for remote dispatch.
    pub fn load_kernel(&mut self, _ptx: Option<&[u8]>) -> Result<(), String> {
        Ok(())
    }

    /// Send a dispatch request to the remote CUDA service.
    pub fn launch(&mut self) -> Result<(), String> {
        emit_kv("cuda", &[("msg", "remote CUDA dispatch started")]);
        if let Some(tcp) = self.addr.strip_prefix("tcp:") {
            if let Ok(mut client) = TcpClient::new_tcp("cuda".into(), tcp, "/") {
                let _ = client.write("/dispatch", 0, b"run");
            }
        }
        Ok(())
    }

    /// Basic telemetry stub returning a placeholder structure.
    pub fn telemetry(&self) -> Result<GpuTelemetry, String> {
        Ok(GpuTelemetry {
            fallback_reason: "remote".into(),
            ..Default::default()
        })
    }
}

impl Default for CudaExecutor {
    fn default() -> Self {
        Self::new()
    }
}
