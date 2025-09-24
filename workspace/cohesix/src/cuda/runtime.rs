// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.17
// Author: Lukas Bower
// Date Modified: 2029-02-21

use crate::telemetry::core::{emit_kv, GpuTelemetry};
use ninep::client::TcpClient;
use std::fs;
use std::time::Instant;

/// Simple executor that forwards CUDA jobs to a remote Secure9P server.
pub struct CudaExecutor {
    addr: String,
    last_telemetry: GpuTelemetry,
}

impl CudaExecutor {
    /// Create a new executor using the address stored in `/srv/cuda` or the
    /// `CUDA_SERVER` environment variable.
    pub fn new() -> Self {
        let addr = std::env::var("CUDA_SERVER")
            .or_else(|_| fs::read_to_string("/srv/cuda"))
            .unwrap_or_default();
        Self {
            addr: addr.trim().to_string(),
            last_telemetry: GpuTelemetry {
                cuda_present: false,
                fallback_reason: "uninitialized".into(),
                ..Default::default()
            },
        }
    }

    /// Loading a kernel is a no-op for remote dispatch.
    pub fn load_kernel(&mut self, _ptx: Option<&[u8]>) -> Result<(), String> {
        Ok(())
    }

    /// Send a dispatch request to the remote CUDA service.
    pub fn launch(&mut self) -> Result<(), String> {
        emit_kv("cuda", &[("msg", "remote CUDA dispatch requested")]);
        let trimmed = self.addr.trim();
        if trimmed.is_empty() {
            return self.record_fallback(
                "missing_remote_endpoint",
                "no CUDA endpoint configured; set CUDA_SERVER or /srv/cuda",
            );
        }

        let Some(tcp) = trimmed.strip_prefix("tcp:") else {
            return self.record_fallback(
                "unsupported_endpoint_scheme",
                "CUDA endpoint must use tcp:<host>:<port>",
            );
        };

        let endpoint = tcp.trim();
        if endpoint.is_empty() {
            return self.record_fallback(
                "invalid_endpoint",
                "CUDA endpoint missing host:port after tcp: prefix",
            );
        }

        let start = Instant::now();
        match TcpClient::new_tcp("cuda".into(), endpoint, "/") {
            Ok(mut client) => {
                if let Err(err) = client.write("/dispatch", 0, b"run") {
                    return self.record_fallback(
                        "remote_dispatch_failed",
                        &format!("remote CUDA dispatch write failed for {endpoint}: {err}"),
                    );
                }

                let elapsed_ns = start.elapsed().as_nanos();
                let exec_time_ns = elapsed_ns.min(u128::from(u64::MAX)) as u64;
                self.last_telemetry = GpuTelemetry {
                    cuda_present: true,
                    driver_version: String::new(),
                    mem_total: 0,
                    mem_free: 0,
                    exec_time_ns,
                    fallback_reason: "remote_dispatch".into(),
                    temperature: None,
                    gpu_utilization: None,
                };
                emit_kv(
                    "cuda",
                    &[
                        ("msg", "remote CUDA dispatch completed"),
                        ("fallback", "remote_dispatch"),
                    ],
                );
                Ok(())
            }
            Err(err) => self.record_fallback(
                "remote_connection_failed",
                &format!("unable to connect to remote CUDA endpoint {endpoint}: {err}"),
            ),
        }
    }

    /// Basic telemetry stub returning a placeholder structure.
    pub fn telemetry(&self) -> Result<GpuTelemetry, String> {
        Ok(self.last_telemetry.clone())
    }

    fn record_fallback(
        &mut self,
        fallback_reason: &'static str,
        detail: &str,
    ) -> Result<(), String> {
        emit_kv(
            "cuda",
            &[
                ("msg", "remote CUDA unavailable; activating fallback"),
                ("fallback", fallback_reason),
            ],
        );
        self.last_telemetry = GpuTelemetry {
            cuda_present: false,
            driver_version: String::new(),
            mem_total: 0,
            mem_free: 0,
            exec_time_ns: 0,
            fallback_reason: fallback_reason.into(),
            temperature: None,
            gpu_utilization: None,
        };
        self.run_cpu_fallback();
        Err(detail.to_string())
    }

    fn run_cpu_fallback(&self) {
        emit_kv(
            "cuda",
            &[
                ("msg", "CPU fallback engaged for CUDA workload"),
                ("fallback", "cpu"),
            ],
        );
        println!("[CUDA] Falling back to CPU execution path");
    }
}

impl Default for CudaExecutor {
    fn default() -> Self {
        Self::new()
    }
}
