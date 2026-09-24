// Author: Lukas Bower
// Purpose: Own bounded isolated CUDA reference children, device/freshness checks, cancellation and independent output verification without claiming admission or Worker authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_METADATA: u64 = 8192;
const MAX_HELPER: u64 = 16 * 1024 * 1024;
const HEADROOM: u64 = 2 * 1024 * 1024 * 1024;

/// Only these compiled reference entrypoints can execute; no command or inline code.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Entrypoint {
    /// c[i] = (i mod 1024) + 2(i mod 1024).
    Vadd,
    /// C[i,j] = sum_k (i+1)(j+1), for square matrices.
    Matmul,
}

impl Entrypoint {
    fn name(self) -> &'static str {
        match self {
            Self::Vadd => "vadd",
            Self::Matmul => "matmul",
        }
    }
}

/// An explicit diagnostic request. It does not confer ticket, lease or Worker authority.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRequest {
    /// Exact local reference request schema.
    pub schema: String,
    /// Correlation only; production admission is a separate outer contract.
    pub ticket_id: String,
    /// Compiled allowlisted entrypoint.
    pub entrypoint: Entrypoint,
    /// Vector length or square matrix side length.
    pub dimension: u32,
    /// Finite repetitions for cancellation/deadline observations.
    pub iterations: u32,
    /// Native ordinal, rechecked against the UUID in the child.
    pub device_ordinal: u32,
    /// Exact 16-byte CUDA device UUID in lowercase hex.
    pub device_uuid: String,
    /// Timestamp of the selected native inventory snapshot.
    pub inventory_observed_unix_ms: u64,
    /// Exact generated provider contract used to construct this request.
    pub provider_graph_sha256: String,
    /// Maximum total device buffer allocation for this request.
    pub memory_budget_bytes: u64,
    /// Whole child deadline, including CUDA context initialization.
    pub deadline_ms: u32,
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn now_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

impl ReferenceRequest {
    /// Validate dimensions, arithmetic, exact graph and freshness before GPU dispatch.
    pub fn validate(&self, now: u64, graph: &str) -> Result<usize> {
        cohesix_authority::validate_id(&self.ticket_id)
            .map_err(|_| anyhow!("invalid_request ticket_id"))?;
        let max_dimension = match self.entrypoint {
            Entrypoint::Vadd => 65_536,
            Entrypoint::Matmul => 128,
        };
        if self.schema != "cohesix-cuda-reference-request/v1"
            || self.device_ordinal > 31
            || self.device_uuid.len() != 32
            || !self
                .device_uuid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.device_uuid.bytes().all(|b| b == b'0')
            || self.provider_graph_sha256 != graph
            || self.dimension == 0
            || self.dimension > max_dimension
            || self.iterations == 0
            || self.iterations > 10_000
            || self.deadline_ms == 0
            || self.deadline_ms > 30_000
            || self.inventory_observed_unix_ms > now
            || now - self.inventory_observed_unix_ms >= 5000
        {
            bail!("invalid_request bounds_identity_or_freshness");
        }
        let count = match self.entrypoint {
            Entrypoint::Vadd => self.dimension as u64,
            Entrypoint::Matmul => u64::from(self.dimension).pow(2),
        };
        let allocation = count * 4 * 3;
        if self.memory_budget_bytes == 0
            || self.memory_budget_bytes > 64 * 1024 * 1024
            || allocation > self.memory_budget_bytes
        {
            bail!("memory_admission request_budget");
        }
        Ok((count * 4) as usize)
    }
}

/// Create a fresh private invocation directory and pin the exact executable bytes.
pub(crate) fn prepare(helper: &Path, expected_hash: &str, state: &Path) -> Result<PathBuf> {
    let metadata = fs::metadata(helper)?;
    if !metadata.is_file() || metadata.len() > MAX_HELPER {
        bail!("invalid_helper size_or_kind");
    }
    let mut executable = Vec::new();
    File::open(helper)?
        .take(MAX_HELPER + 1)
        .read_to_end(&mut executable)?;
    if executable.len() as u64 > MAX_HELPER || hash(&executable) != expected_hash {
        bail!("invalid_helper digest");
    }
    let mut directory = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(state)?;
    let selected = state.join("executor");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&selected)?;
    file.write_all(&executable)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o500))?;
    }
    Ok(selected.canonicalize()?)
}

fn read_pipe(pipe: impl Read, maximum: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.take(maximum + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(crate) fn invoke(
    helper: &Path,
    state: &Path,
    args: &[String],
    deadline: Instant,
    cancel: &AtomicBool,
    visibility: Option<&str>,
    maximum: u64,
) -> Result<Value> {
    invoke_with_env(
        helper,
        state,
        args,
        deadline,
        cancel,
        visibility,
        maximum,
        &BTreeMap::new(),
        None,
    )
}

fn check_declared_disk(state: &Path, max_disk: u64, max_output: u64) -> Result<()> {
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in fs::read_dir(state)? {
        let entry = entry?;
        count += 1;
        ensure!(count <= 4, "disk_bound undeclared_file");
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| anyhow!("disk_bound file_name"))?;
        ensure!(
            matches!(
                name,
                "executor" | "input.bin" | "output.bin" | "started.json"
            ),
            "disk_bound undeclared_file"
        );
        let info = fs::symlink_metadata(entry.path())?;
        ensure!(info.is_file(), "disk_bound file_kind");
        if name == "output.bin" {
            ensure!(info.len() <= max_output, "disk_bound output_bytes");
        }
        total = total
            .checked_add(info.len())
            .ok_or_else(|| anyhow!("disk_bound overflow"))?;
        ensure!(total <= max_disk, "disk_bound total_bytes");
    }
    Ok(())
}

pub(crate) fn invoke_with_env(
    helper: &Path,
    state: &Path,
    args: &[String],
    deadline: Instant,
    cancel: &AtomicBool,
    visibility: Option<&str>,
    maximum: u64,
    environment: &BTreeMap<String, String>,
    disk_limits: Option<(u64, u64)>,
) -> Result<Value> {
    if cancel.load(Ordering::Acquire) {
        bail!("cancelled before_dispatch");
    }
    if Instant::now() >= deadline {
        bail!("timeout before_dispatch");
    }
    let mut command = Command::new(helper);
    command
        .args(args)
        .current_dir(state)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CUDA_CACHE_DISABLE", "1")
        .env("COH_REFERENCE_OWNER_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(visibility) = visibility {
        command.env("CUDA_VISIBLE_DEVICES", visibility);
    }
    command.envs(environment);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("unavailable child_stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("unavailable child_stderr"))?;
    let output = thread::spawn(move || read_pipe(stdout, maximum));
    let errors = thread::spawn(move || read_pipe(stderr, maximum));
    let outcome = loop {
        if let Some((max_disk, max_output)) = disk_limits {
            if let Err(error) = check_declared_disk(state, max_disk, max_output) {
                break Err(error);
            }
        }
        if cancel.load(Ordering::Acquire) {
            break Err(anyhow!("cancelled cuda_reference"));
        }
        if Instant::now() >= deadline {
            break Err(anyhow!("timeout cuda_reference"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(anyhow!("unavailable child_wait: {error}")),
            Ok(None) => {}
        }
        thread::sleep(Duration::from_millis(2));
    };
    if outcome.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let stdout = output
        .join()
        .map_err(|_| anyhow!("unavailable child_stdout"))??;
    let stderr = errors
        .join()
        .map_err(|_| anyhow!("unavailable child_stderr"))??;
    if let Some((max_disk, max_output)) = disk_limits {
        check_declared_disk(state, max_disk, max_output)?;
    }
    let status = outcome?;
    if stdout.len() as u64 > maximum || stderr.len() as u64 > maximum {
        bail!("response_limit cuda_reference");
    }
    if !status.success() {
        let failure: Value = serde_json::from_slice(&stderr)
            .map_err(|_| anyhow!("cuda_failure unclassified_child"))?;
        if status.code() == Some(5)
            && failure["code"] == "cuda_failure"
            && failure["cuda_error"] == 2
        {
            bail!("cuda_out_of_memory bounded_child");
        }
        let code = failure["code"]
            .as_str()
            .ok_or_else(|| anyhow!("cuda_failure unclassified_child"))?;
        if ![
            "wrong_device",
            "memory_admission",
            "cuda_failure",
            "device_identity_unavailable",
            "invalid_bound",
            "profile_mismatch",
            "owner_binding",
            "native_marker_failed",
        ]
        .contains(&code)
        {
            bail!("cuda_failure child_refused");
        }
        bail!("{code} cuda_error={}", failure["cuda_error"]);
    }
    Ok(serde_json::from_slice(&stdout)?)
}

/// Capture actual UUID, memory, device capability and driver/runtime in a bounded child.
pub fn inventory(helper: &Path, helper_hash: &str, state: &Path, ordinal: u32) -> Result<Value> {
    inventory_selected(helper, helper_hash, state, ordinal, None)
}

/// A MIG enrollment selects one native CI UUID; ordinals cannot select another visible device.
pub fn inventory_selected(
    helper: &Path,
    helper_hash: &str,
    state: &Path,
    ordinal: u32,
    selection: Option<&crate::mig::Selection>,
) -> Result<Value> {
    ensure_profile(selection)?;
    if ordinal > 31 {
        bail!("invalid_request device_ordinal");
    }
    let selected = prepare(helper, helper_hash, state)?;
    let mig = if let Some(selection) = selection {
        let observed = crate::mig::discover(
            &selected,
            helper_hash,
            &state.join("mig"),
            selection.parent_ordinal,
        )?;
        selection.require_observed(&observed)?;
        Some(observed)
    } else {
        None
    };
    let native = invoke(
        &selected,
        state,
        &["inventory".into(), ordinal.to_string()],
        Instant::now() + Duration::from_secs(10),
        &AtomicBool::new(false),
        selection.map(|selection| selection.instance.uuid.as_str()),
        MAX_METADATA,
    )?;
    if native["schema"] != "cohesix-cuda-native-observation/v1"
        || native["runtime_version"] != 13_020
        || if let Some(selection) = selection {
            ordinal != 0
                || native["integrated"] != false
                || native["device_uuid"] != selection.cuda_uuid()?
                || !native["compute_major"]
                    .as_u64()
                    .is_some_and(|major| major >= 8)
                || !native["driver_version"]
                    .as_u64()
                    .is_some_and(|version| version >= 13020)
        } else {
            native["compute_major"] != 8
                || native["compute_minor"] != 7
                || native["integrated"] != true
        }
    {
        bail!("profile_mismatch selected_cuda_device");
    }
    let mut value = serde_json::json!({"schema":"cohesix-cuda-reference-inventory/v1",
        "mode":"live", "proof_class":"native_discovery", "authoritative":false, "worker_proof":false,
        "observed_unix_ms":now_ms()?, "provider_graph_sha256":cohesix_authority::provider::registry()?["graph_sha256"],
        "helper_sha256":helper_hash, "native":native});
    if let Some(mig) = mig {
        value["mig"] = serde_json::to_value(mig)?;
    }
    value["topology_sha256"] =
        serde_json::Value::String(crate::workload::topology_hash(&value, helper_hash)?);
    write_record(state, &value)?;
    Ok(value)
}

/// Check every output element against the independent algebraic reference contract.
pub fn verify_output(entrypoint: Entrypoint, dimension: u32, bytes: &[u8]) -> Result<()> {
    if dimension == 0
        || dimension
            > match entrypoint {
                Entrypoint::Vadd => 65_536,
                Entrypoint::Matmul => 128,
            }
    {
        bail!("invalid_output dimension");
    }
    let count = match entrypoint {
        Entrypoint::Vadd => dimension as usize,
        Entrypoint::Matmul => (dimension as usize)
            .checked_mul(dimension as usize)
            .ok_or_else(|| anyhow!("invalid_output dimension"))?,
    };
    if dimension == 0
        || bytes.len()
            != count
                .checked_mul(4)
                .ok_or_else(|| anyhow!("invalid_output size"))?
    {
        bail!("output_mismatch length");
    }
    for (i, raw) in bytes.chunks_exact(4).enumerate() {
        let value = f32::from_le_bytes(raw.try_into()?);
        let expected = match entrypoint {
            Entrypoint::Vadd => (3 * (i % 1024)) as f32,
            Entrypoint::Matmul => {
                (dimension as usize * (i / dimension as usize + 1) * (i % dimension as usize + 1))
                    as f32
            }
        };
        if value != expected {
            bail!("output_mismatch element");
        }
    }
    Ok(())
}

/// Execute the diagnostic reference and retain verified CAS output identity.
pub fn execute(
    helper: &Path,
    helper_hash: &str,
    state: &Path,
    request: &ReferenceRequest,
    cancel: &AtomicBool,
) -> Result<Value> {
    execute_selected(helper, helper_hash, state, request, cancel, None)
}

/// Recheck the exact native MIG selection immediately before the isolated CUDA child.
pub fn execute_selected(
    helper: &Path,
    helper_hash: &str,
    state: &Path,
    request: &ReferenceRequest,
    cancel: &AtomicBool,
    selection: Option<&crate::mig::Selection>,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(request.deadline_ms));
    ensure_profile(selection)?;
    let registry = cohesix_authority::provider::registry()?;
    let graph = registry["graph_sha256"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid_registry"))?;
    let output_bytes = request.validate(now_ms()?, graph)?;
    if cancel.load(Ordering::Acquire) {
        bail!("cancelled before_dispatch");
    }
    let selected = prepare(helper, helper_hash, state)?;
    let mig = if let Some(selection) = selection {
        ensure_selected_request(selection, request)?;
        let observed = crate::mig::discover_until(
            &selected,
            helper_hash,
            &state.join("mig"),
            selection.parent_ordinal,
            deadline,
            cancel,
        )?;
        selection.require_observed(&observed)?;
        Some(observed)
    } else {
        None
    };
    let args = [
        request.entrypoint.name().into(),
        request.device_ordinal.to_string(),
        request.device_uuid.clone(),
        request.dimension.to_string(),
        request.iterations.to_string(),
    ];
    let native = invoke(
        &selected,
        state,
        &args,
        deadline,
        cancel,
        selection.map(|selection| selection.instance.uuid.as_str()),
        MAX_METADATA,
    )?;
    if native["schema"] != "cohesix-cuda-native-result/v1"
        || native["device_uuid"] != request.device_uuid
        || native["device_ordinal"] != request.device_ordinal
        || native["entrypoint"] != request.entrypoint.name()
        || native["dimension"] != request.dimension
        || native["iterations"] != request.iterations
        || native["output_bytes"] != output_bytes as u64
        || native["allocation_bytes"].as_u64() != Some(output_bytes as u64 * 3)
        || !native["free_bytes_before"]
            .as_u64()
            .is_some_and(|free| free >= HEADROOM + output_bytes as u64 * 3)
    {
        bail!("invalid_observation native_result_binding");
    }
    let path = state.join("output.bin");
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() != output_bytes as u64 {
        bail!("output_mismatch size_or_kind");
    }
    let mut file = File::open(&path)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(output_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    verify_output(request.entrypoint, request.dimension, &bytes)?;
    file.sync_all()?;
    let mut value = serde_json::json!({"schema":"cohesix-cuda-reference-observation/v1", "mode":"live",
        "proof_class":"native_provider_operation", "authoritative":false, "worker_proof":false,
        "ticket_id":request.ticket_id, "provider_graph_sha256":graph, "helper_sha256":helper_hash,
        "outcome":"verified", "isolation":"dedicated_process_and_cuda_context", "streams":1,
        "output":{"sha256":hash(&bytes),"bytes":bytes.len(),"path":"output.bin"},"native":native});
    if let Some(mig) = mig {
        // The complete discovery remains in the private invocation's CAS input;
        // one exact instance identity keeps the terminal wire record bounded.
        value["mig"] = serde_json::to_value(selection)?;
        value["mig_topology_sha256"] = Value::String(mig.generation()?);
        value["isolation"] = Value::String("mig_compute_instance_and_cuda_context".into());
        value["memory_isolation"] =
            Value::String("gpu_instance_shared_between_compute_instances".into());
    }
    write_record(state, &value)?;
    Ok(value)
}

fn ensure_profile(selection: Option<&crate::mig::Selection>) -> Result<()> {
    let registry = cohesix_authority::provider::registry()?;
    let profile = registry["contract"]["gpu_executor"]["profile"].as_str();
    match (profile, selection) {
        (Some("jetson-orin-nano-jp7"), None) => Ok(()),
        (Some("nvidia-mig-cuda13"), Some(selection)) => selection.validate(),
        _ => bail!("profile_mismatch executor_selection"),
    }
}

fn ensure_selected_request(
    selection: &crate::mig::Selection,
    request: &ReferenceRequest,
) -> Result<()> {
    if request.device_ordinal != 0 || request.device_uuid != selection.cuda_uuid()? {
        bail!("wrong_device MIG_request");
    }
    Ok(())
}

fn write_record(state: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state.join("observation.json"))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    File::open(state)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn native_oom_mapping_uses_isolated_fault_injection_without_allocating_ram() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let child = state.path().join("owned-fault-child");
        for (cuda_error, expected) in [(2, "cuda_out_of_memory"), (999, "cuda_failure")] {
            std::fs::write(&child,format!("#!/bin/sh\nprintf '%s\\n' '{{\"code\":\"cuda_failure\",\"cuda_error\":{cuda_error}}}' >&2\nexit 5\n")).unwrap();
            std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o700)).unwrap();
            let error = invoke(
                &child,
                state.path(),
                &[],
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
                None,
                4096,
            )
            .unwrap_err();
            assert!(error.to_string().starts_with(expected));
        }
    }

    #[test]
    #[cfg(unix)]
    fn registered_child_exceeding_declared_output_is_reaped() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let child = state.path().join("executor");
        std::fs::write(
            &child,
            "#!/usr/bin/env python3\nfrom pathlib import Path\nimport time\nPath('output.bin').write_bytes(b'x' * 4096)\ntime.sleep(5)\n",
        )
        .unwrap();
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = invoke_with_env(
            &child,
            state.path(),
            &[],
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
            None,
            4096,
            &BTreeMap::new(),
            Some((8192, 64)),
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("disk_bound"));
    }
    #[test]
    fn output_verifier_checks_exact_algebra_and_all_bytes() {
        let vector: Vec<u8> = [0.0f32, 3.0, 6.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        verify_output(Entrypoint::Vadd, 3, &vector).unwrap();
        assert!(verify_output(Entrypoint::Vadd, 2, &vector).is_err());
        let matrix: Vec<u8> = [2.0f32, 4.0, 4.0, 8.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        verify_output(Entrypoint::Matmul, 2, &matrix).unwrap();
        let mut bad = matrix;
        bad[0] ^= 1;
        assert!(verify_output(Entrypoint::Matmul, 2, &bad).is_err());
    }
    #[test]
    fn stale_wrong_identity_and_oversized_requests_refuse_before_dispatch() {
        let mut request = ReferenceRequest {
            schema: "cohesix-cuda-reference-request/v1".into(),
            ticket_id: "reference-1".into(),
            entrypoint: Entrypoint::Vadd,
            dimension: 32,
            iterations: 1,
            device_ordinal: 0,
            device_uuid: "a".repeat(32),
            inventory_observed_unix_ms: 1000,
            provider_graph_sha256: "graph".into(),
            memory_budget_bytes: 384,
            deadline_ms: 1000,
        };
        assert_eq!(request.validate(1000, "graph").unwrap(), 128);
        assert!(request.validate(6000, "graph").is_err());
        assert!(request.validate(999, "graph").is_err());
        assert!(request.validate(1000, "other").is_err());
        request.memory_budget_bytes = 383;
        assert!(request.validate(1000, "graph").is_err());
        request.dimension = 65_537;
        assert!(request.validate(1000, "graph").is_err());
    }
}
