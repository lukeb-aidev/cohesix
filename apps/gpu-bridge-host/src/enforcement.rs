// Author: Lukas Bower
// Purpose: Measure the native executor cgroup and refuse selected deployment lanes without their actual resource controls.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};

/// Selected native isolation owner. Absence retains an explicitly unqualified local lane.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// A hardened service invocation owned by the system manager.
    Systemd,
    /// A digest-pinned container owned by Docker Engine.
    Docker,
}

fn read(path: &Path) -> Result<String> {
    let mut out = String::new();
    File::open(path)?.take(4097).read_to_string(&mut out)?;
    ensure!(out.len() <= 4096, "ELIMIT cgroup-observation");
    Ok(out)
}
fn limits(memory: &str, cpu: &str, pids: &str) -> Result<Value> {
    let memory: u64 = memory
        .trim()
        .parse()
        .map_err(|_| anyhow!("unenforced cgroup-memory"))?;
    let fields: Vec<_> = cpu.split_whitespace().collect();
    ensure!(fields.len() == 2, "unenforced cgroup-cpu");
    let quota: u64 = fields[0]
        .parse()
        .map_err(|_| anyhow!("unenforced cgroup-cpu"))?;
    let period: u64 = fields[1]
        .parse()
        .map_err(|_| anyhow!("unenforced cgroup-period"))?;
    let pids: u64 = pids
        .trim()
        .parse()
        .map_err(|_| anyhow!("unenforced cgroup-tasks"))?;
    ensure!(
        memory > 0
            && memory <= 1_073_741_824
            && period > 0
            && quota > 0
            && quota <= period.saturating_mul(2)
            && pids > 0
            && pids <= 64,
        "unenforced cgroup-bounds"
    );
    Ok(
        json!({"memory_max_bytes":memory,"cpu_quota_us":quota,"cpu_period_us":period,"pids_max":pids}),
    )
}

/// Read current kernel controls before dispatch and after completion. These bound
/// host process resources; they do not partition GPU memory or prove device isolation.
pub fn observe(lane: Option<&Lane>) -> Result<Value> {
    let Some(lane) = lane else {
        return Ok(json!({"schema":"cohesix-native-enforcement/v1",
        "lane":"local","cgroup_limits":"not_selected","gpu_memory_hard_partition":false,
        "gpu_memory_admission":"measured_free_with_os_headroom","concurrency_limit":1,
        "deadline_mechanism":"owned_child_kill_and_reap"}));
    };
    ensure!(
        cfg!(target_os = "linux"),
        "not_supported native-cgroup-lane"
    );
    let groups = read(Path::new("/proc/self/cgroup"))?;
    let rows: Vec<_> = groups
        .lines()
        .filter_map(|r| r.strip_prefix("0::"))
        .collect();
    ensure!(rows.len() == 1, "unavailable cgroup-v2");
    let relative = Path::new(rows[0]);
    ensure!(
        relative.is_absolute()
            && relative
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "EPERM cgroup-path"
    );
    let leaf = relative
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("unavailable cgroup-owner"))?;
    let owner = match lane {
        Lane::Systemd => {
            ensure!(
                leaf.ends_with(".service") && !leaf.starts_with("docker-"),
                "wrong_native_lane systemd"
            );
            let invocation = std::env::var("INVOCATION_ID")
                .map_err(|_| anyhow!("unavailable systemd-invocation"))?;
            ensure!(
                invocation.len() == 32
                    && invocation.bytes().all(|b| b.is_ascii_hexdigit())
                    && invocation.bytes().any(|b| b != b'0'),
                "unavailable systemd-invocation"
            );
            json!({"kind":"systemd","unit":leaf,"invocation_id":invocation})
        }
        Lane::Docker => {
            let id = leaf
                .strip_prefix("docker-")
                .and_then(|s| s.strip_suffix(".scope"))
                .unwrap_or(leaf);
            ensure!(
                id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "wrong_native_lane docker"
            );
            json!({"kind":"docker","container_id":id})
        }
    };
    let path = Path::new("/sys/fs/cgroup").join(relative.strip_prefix("/")?);
    let constraints = limits(
        &read(&path.join("memory.max"))?,
        &read(&path.join("cpu.max"))?,
        &read(&path.join("pids.max"))?,
    )?;
    let status = read(Path::new("/proc/self/status"))?;
    ensure!(
        status.lines().any(|l| l
            .split_once(':')
            .is_some_and(|(k, v)| k == "NoNewPrivs" && v.trim() == "1")),
        "unenforced no-new-privileges"
    );
    Ok(
        json!({"schema":"cohesix-native-enforcement/v1","owner":owner,"cgroup":rows[0],
        "controls":constraints,"no_new_privileges":true,"gpu_memory_hard_partition":false,
        "gpu_memory_admission":"measured_free_with_os_headroom","concurrency_limit":1,
        "deadline_mechanism":"owned_child_kill_and_reap"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_lane_requires_finite_measured_limits() {
        assert_eq!(
            limits("1073741824\n", "200000 100000\n", "64\n").unwrap(),
            json!({"memory_max_bytes":1073741824u64,"cpu_quota_us":200000,"cpu_period_us":100000,"pids_max":64})
        );
        for (m, c, p) in [
            ("max", "200000 100000", "64"),
            ("1073741825", "200000 100000", "64"),
            ("1024", "max 100000", "64"),
            ("1024", "200001 100000", "64"),
            ("1024", "1 0", "64"),
            ("1024", "1 100000", "max"),
        ] {
            assert!(limits(m, c, p).is_err());
        }
        assert_eq!(observe(None).unwrap()["cgroup_limits"], "not_selected");
    }
}
