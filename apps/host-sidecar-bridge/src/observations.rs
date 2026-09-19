// Author: Lukas Bower
// Purpose: Read bounded native host APIs and selected system files without using target fixtures or performing control actions.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{anyhow, bail, Result};
use cohesix_authority::snapshot::{Entry, UnavailableReason};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const MAX_BYTES: usize = 65536;
const MAX_ROWS: usize = 32;

/// One whole-observation deadline covers every selected API and file. A failure
/// returns no entries, causing the publisher to withdraw the previous snapshot.
pub fn collect(
    provider: &str,
    deadline: Instant,
) -> std::result::Result<Vec<Entry>, UnavailableReason> {
    let result = match provider {
        "systemd" if cfg!(target_os = "linux") => systemd(deadline),
        "docker" if cfg!(target_os = "linux") => docker(deadline),
        "k8s" => kubernetes(deadline),
        "network" => network(deadline),
        "jetson" if cfg!(all(target_os = "linux", target_arch = "aarch64")) => jetson(deadline),
        "nvidia" if cfg!(target_os = "linux") => nvidia(deadline),
        "launchd" if cfg!(target_os = "macos") => launchd(deadline),
        "endpoint_compliance" if cfg!(target_os = "macos") => endpoint_compliance(deadline),
        "modbus" | "dnp3" => field_bus(provider),
        _ => return Err(UnavailableReason::NotSupported),
    };
    if Instant::now() >= deadline {
        return Err(UnavailableReason::TimedOut);
    }
    result
        .map(|mut entries| {
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            entries
        })
        .map_err(|error| {
            if error.to_string().starts_with("not_enabled") {
                UnavailableReason::NotEnabled
            } else {
                UnavailableReason::SourceFailed
            }
        })
}

fn entry(path: impl Into<String>, value: impl serde::Serialize) -> Result<Entry> {
    Ok(Entry {
        path: path.into(),
        value: serde_json::to_string(&value)?,
    })
}

fn field_bus(provider: &str) -> Result<Vec<Entry>> {
    let root = std::env::var_os("COHESIX_FIELD_BUS_STATE_ROOT")
        .ok_or_else(|| anyhow!("not_enabled field-bus WAL"))?;
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?;
    Ok(crate::field_bus::collect(provider, Path::new(&root), now)?.0)
}

/// Fixed absolute native utilities only. stdout, child lifetime and reader
/// ownership are bounded, including a child which closes stdout before exiting.
pub(crate) fn command(program: &Path, args: &[&str], deadline: Instant) -> Result<Vec<u8>> {
    if !program.is_absolute() || Instant::now() >= deadline {
        bail!("invalid native-command");
    }
    let mut native = Command::new(program);
    #[cfg(unix)]
    native.process_group(0);
    let mut child = native
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("invalid native-pipe");
        }
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = match std::thread::Builder::new()
        .name("native-observation".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = sender.send(result);
        }) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };
    let mut reaped = false;
    let result = (|| -> Result<Vec<u8>> {
        let bytes = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
        if bytes.len() > MAX_BYTES {
            bail!("limit native-output");
        }
        loop {
            if let Some(status) = child.try_wait()? {
                reaped = true;
                if !status.success() {
                    bail!("failed native-command");
                }
                return Ok(bytes);
            }
            if Instant::now() >= deadline {
                bail!("timeout native-command");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    if result.is_err() && !reaped {
        // An unreaped owned PID cannot be reused for an unrelated process.
        #[cfg(unix)]
        if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = child.kill();
    }
    let _ = child.wait();
    reader.join().map_err(|_| anyhow!("failed native-reader"))?;
    result
}

fn read(path: &Path, maximum: usize, deadline: Instant) -> Result<String> {
    if Instant::now() >= deadline {
        bail!("timeout native-file");
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        bail!("limit native-file");
    }
    Ok(String::from_utf8(bytes)?
        .trim_end_matches(['\n', '\0'])
        .into())
}

fn rows(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .filter(|rows| rows.len() <= MAX_ROWS)
        .ok_or_else(|| anyhow!("limit native-rows"))
}

fn systemd(deadline: Instant) -> Result<Vec<Entry>> {
    let mut entries = vec![entry("source", "systemd-manager-dbus")?];
    for unit in crate::native::discover_systemd_units_before(deadline)? {
        let observation = crate::native::observe_systemd_before(&unit, deadline)?;
        entries.push(entry(format!("units/{unit}"), observation)?);
    }
    Ok(entries)
}

fn docker(deadline: Instant) -> Result<Vec<Entry>> {
    let client = crate::docker::DockerEngine::connect(Path::new("/var/run/docker.sock"), deadline)?;
    let status = client.status()?;
    Ok(vec![
        entry("source", "docker-engine-api")?,
        entry(
            "engine",
            json!({"version":status.version,"containers":status.containers,"running":status.running,"paused":status.paused,"stopped":status.stopped}),
        )?,
    ])
}

fn kubernetes(deadline: Instant) -> Result<Vec<Entry>> {
    if std::env::var_os("COHESIX_K8S_API_URL").is_none() {
        bail!("not_enabled kubernetes");
    }
    let client = crate::kubernetes::Kubernetes::from_environment(
        deadline.saturating_duration_since(Instant::now()),
    )?;
    let mut entries = vec![entry("source", "kubernetes-core-v1")?];
    for node in client.nodes()? {
        entries.push(entry(format!("nodes/{}", node.name), node)?);
    }
    Ok(entries)
}

fn network(deadline: Instant) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    if cfg!(target_os = "linux") {
        entries.push(entry("source", "iproute2-rtnetlink-json")?);
        for (name, args) in [
            ("links", vec!["-json", "-s", "link", "show"]),
            ("addresses", vec!["-json", "address", "show"]),
            ("routes", vec!["-json", "route", "show", "table", "all"]),
        ] {
            let value: Value =
                serde_json::from_slice(&command(Path::new("/usr/sbin/ip"), &args, deadline)?)?;
            entries.extend(linux_network_projection(name, &value)?);
        }
    } else if cfg!(target_os = "macos") {
        let helper = std::env::var_os("COHESIX_MACOS_NETWORK_HELPER")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("not_enabled macos-network-helper"))?;
        let value: Value = serde_json::from_slice(&command(&helper, &[], deadline)?)?;
        if value["schema"] != "cohesix-macos-network/v1" {
            bail!("invalid macos-network-schema");
        }
        entries.push(entry("source", &value["source"])?);
        entries.push(entry("default-routes", &value["default_routes"])?);
        entries.push(entry("counter-semantics", &value["counter_semantics"])?);
        for (name, fields) in [
            (
                "interfaces",
                &[
                    "name",
                    "flags",
                    "up",
                    "running",
                    "mtu",
                    "received_bytes",
                    "sent_bytes",
                    "received_packets",
                    "sent_packets",
                    "receive_errors",
                    "send_errors",
                    "receive_drops",
                    "counter_width_bits",
                ][..],
            ),
            ("addresses", &["interface", "family", "address"][..]),
        ] {
            // Column names are declared once; every selected native row is
            // retained. Null means the native API did not expose that field.
            entries.push(entry(format!("fields/{name}"), fields)?);
            for (index, row) in rows(&value[name])?.iter().enumerate() {
                let object = row
                    .as_object()
                    .ok_or_else(|| anyhow!("invalid macos-network-row"))?;
                let values: Vec<_> = fields
                    .iter()
                    .map(|field| object.get(*field).cloned().unwrap_or(Value::Null))
                    .collect();
                entries.push(entry(format!("{name}/{index}"), values)?);
            }
        }
    } else {
        bail!("not_enabled native-network");
    }
    Ok(entries)
}

fn columns<'a>(objects: impl Iterator<Item = &'a Value>) -> Result<Vec<String>> {
    let mut names = std::collections::BTreeSet::new();
    for object in objects {
        for key in object
            .as_object()
            .ok_or_else(|| anyhow!("invalid native-table-object"))?
            .keys()
        {
            if key.len() > 64 || names.len() > 32 {
                bail!("limit native-table-columns");
            }
            names.insert(key.clone());
        }
    }
    if names.len() > 32 {
        bail!("limit native-table-columns");
    }
    Ok(names.into_iter().collect())
}

fn values(object: &Value, fields: &[String]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|field| object.get(field).cloned().unwrap_or(Value::Null))
            .collect(),
    )
}

fn linux_network_projection(name: &str, value: &Value) -> Result<Vec<Entry>> {
    let native = rows(value)?;
    let fields: &[&str] = match name {
        "links" => &[
            "ifindex",
            "ifname",
            "flags",
            "mtu",
            "operstate",
            "address",
            "stats64",
            "stats",
        ],
        "addresses" => &["ifindex", "ifname", "addr_info"],
        "routes" => &[
            "dst", "gateway", "dev", "protocol", "scope", "prefsrc", "metric", "table", "type",
            "flags",
        ],
        _ => bail!("invalid network projection"),
    };
    let mut entries = vec![entry(format!("fields/{name}"), fields)?];
    let nested_fields = if name == "addresses" {
        let mut addresses = Vec::new();
        for row in native {
            if let Some(value) = row.get("addr_info") {
                addresses.extend(rows(value)?.iter());
            }
        }
        let fields = columns(addresses.into_iter())?;
        entries.push(entry("fields/address-info", &fields)?);
        fields
    } else if name == "links" {
        let counters = native
            .iter()
            .flat_map(|row| {
                ["stats64", "stats"]
                    .into_iter()
                    .filter_map(|field| row.get(field))
            })
            .flat_map(|stats| {
                ["rx", "tx"]
                    .into_iter()
                    .filter_map(|direction| stats.get(direction))
            });
        let fields = columns(counters)?;
        entries.push(entry("fields/link-stats", &fields)?);
        fields
    } else {
        Vec::new()
    };
    let mut route_group = Vec::new();
    let mut route_group_index = 0usize;
    for (index, row) in native.iter().enumerate() {
        if !row.is_object() {
            bail!("invalid native-network-row");
        }
        let mut selected = Vec::new();
        for field in fields {
            let mut cell = row.get(*field).cloned().unwrap_or(Value::Null);
            if *field == "addr_info" && !cell.is_null() {
                cell = Value::Array(
                    rows(&cell)?
                        .iter()
                        .map(|address| values(address, &nested_fields))
                        .collect(),
                );
            } else if ["stats64", "stats"].contains(field) && !cell.is_null() {
                cell = json!({"rx":values(&cell["rx"], &nested_fields), "tx":values(&cell["tx"], &nested_fields)});
            }
            selected.push(cell);
        }
        if name == "routes" {
            let row = Value::Array(selected);
            let candidate: Vec<_> = route_group
                .iter()
                .chain(std::iter::once(&row))
                .cloned()
                .collect();
            if candidate.len() > 8 || serde_json::to_vec(&candidate)?.len() > 768 {
                if route_group.is_empty() {
                    bail!("limit native-route-row");
                }
                entries.push(entry(format!("routes/{route_group_index}"), &route_group)?);
                route_group.clear();
                route_group_index += 1;
            }
            route_group.push(row);
            if serde_json::to_vec(&route_group)?.len() > 768 {
                bail!("limit native-route-row");
            }
        } else {
            entries.push(entry(format!("{name}/{index}"), selected)?);
        }
    }
    if !route_group.is_empty() {
        entries.push(entry(format!("routes/{route_group_index}"), route_group)?);
    }
    Ok(entries)
}

fn native_directories(path: &Path, prefix: &str) -> Result<Vec<PathBuf>> {
    let mut selected = Vec::new();
    for child in std::fs::read_dir(path)? {
        let child = child?;
        if child
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(prefix))
        {
            selected.push(child.path());
        }
        if selected.len() > MAX_ROWS {
            bail!("limit native-directories");
        }
    }
    selected.sort();
    Ok(selected)
}

fn jetson(deadline: Instant) -> Result<Vec<Entry>> {
    let board = read(Path::new("/proc/device-tree/model"), 512, deadline)?;
    if !board.contains("Jetson") {
        bail!("not_enabled jetson-board");
    }
    let mut entries = vec![
        entry("source", "device-tree-dpkg-sysfs-nvpmodel")?,
        entry("board", board)?,
    ];
    let packages = command(
        Path::new("/usr/bin/dpkg-query"),
        &[
            "-W",
            "-f=${Package}\t${Version}\n",
            "nvidia-jetpack",
            "nvidia-l4t-core",
            "cuda-toolkit-13-2",
            "nvidia-container-toolkit",
        ],
        deadline,
    )?;
    for line in std::str::from_utf8(&packages)?.lines() {
        let (name, version) = line
            .split_once('\t')
            .ok_or_else(|| anyhow!("invalid native-package"))?;
        crate::native::validate_native_id(name)?;
        entries.push(entry(format!("packages/{name}"), version)?);
    }
    for zone in native_directories(Path::new("/sys/class/thermal"), "thermal_zone")? {
        let name = zone
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("invalid thermal-name"))?;
        let kind = read(&zone.join("type"), 128, deadline)?;
        let value = match read(&zone.join("temp"), 32, deadline).and_then(|v| Ok(v.parse::<i64>()?))
        {
            Ok(value) if (-273150..=1000000).contains(&value) => {
                json!({"type":kind,"temperature_mc":value})
            }
            _ => json!({"type":kind,"state":"not_supported"}),
        };
        entries.push(entry(format!("thermal/{name}"), value)?);
    }
    for (root, prefix, file, scale) in [
        (
            "/sys/devices/system/cpu/cpufreq",
            "policy",
            "scaling_cur_freq",
            1000u64,
        ),
        ("/sys/class/devfreq", "", "cur_freq", 1u64),
    ] {
        for (index, device) in native_directories(Path::new(root), prefix)?
            .iter()
            .enumerate()
        {
            let value =
                match read(&device.join(file), 32, deadline).and_then(|v| Ok(v.parse::<u64>()?)) {
                    Ok(value) if value <= 1_000_000_000_000 / scale => {
                        json!({"source":device,"frequency_hz":value*scale})
                    }
                    _ => json!({"source":device,"state":"not_supported"}),
                };
            entries.push(entry(format!("clocks/{file}-{index}"), value)?);
        }
    }
    let power = match command(Path::new("/usr/sbin/nvpmodel"), &["-q"], deadline) {
        Ok(bytes) => json!({"source":"nvpmodel-query","observation":std::str::from_utf8(&bytes)?}),
        Err(_) => json!({"source":"nvpmodel-query","state":"not_supported"}),
    };
    entries.push(entry("power", power)?);
    Ok(entries)
}

fn nvidia(deadline: Instant) -> Result<Vec<Entry>> {
    if let Some(path) = std::env::var_os("COHESIX_GPU_EXECUTOR_CONFIG") {
        let config = gpu_bridge_host::workload::Config::load(Path::new(&path))?;
        let value = gpu_bridge_host::reference::inventory_selected(
            &config.helper,
            &config.helper_sha256,
            &config.state_root,
            0,
            config.mig.as_ref(),
        )?;
        if value["native"]["device_uuid"] != config.device_uuid {
            bail!("invalid native-device-identity");
        }
        return Ok(vec![
            entry("source", "cuda-reference-helper")?,
            entry("gpu/0", &value["native"])?,
        ]);
    }
    let bytes=command(Path::new("/usr/bin/nvidia-smi"), &["--query-gpu=uuid,pci.bus_id,driver_version,memory.total,memory.used,temperature.gpu,power.draw","--format=csv,noheader,nounits"], deadline)?;
    let mut entries = vec![entry("source", "nvidia-smi-nvml-query")?];
    for (index, line) in std::str::from_utf8(&bytes)?.lines().enumerate() {
        if index >= MAX_ROWS {
            bail!("limit nvidia-devices");
        }
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 7 || !fields[0].starts_with("GPU-") {
            bail!("invalid nvidia-row");
        }
        entries.push(entry(format!("gpu/{index}"),json!({"uuid":fields[0],"pci_bus_id":fields[1],"driver_version":fields[2],"memory_total_mib":fields[3],"memory_used_mib":fields[4],"temperature_c":fields[5],"power_draw_w":fields[6]}))?);
    }
    if entries.len() == 1 {
        bail!("not_enabled nvidia-devices");
    }
    Ok(entries)
}

fn endpoint_compliance(deadline: Instant) -> Result<Vec<Entry>> {
    if !cohesix_authority::mac_release::targets()?.iter().any(|t| {
        matches!(
            t.operation,
            cohesix_authority::mac_release::Operation::Compliance
        )
    }) {
        bail!("not_enabled endpoint-compliance-target");
    }
    Ok(vec![entry(
        "controls",
        crate::mac_release::compliance(deadline)?,
    )?])
}

fn launchd(deadline: Instant) -> Result<Vec<Entry>> {
    let targets = cohesix_authority::macos::launchd_targets()?;
    if targets.is_empty() {
        bail!("not_enabled launchd-target-map");
    }
    let mut entries = vec![entry("source", "launchctl-and-libproc")?];
    for target in targets {
        let observation = crate::launchd::observe(&target, deadline)?;
        entries.push(entry(format!("services/{}", target.id), observation)?);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_columns_preserve_native_addresses_counters_and_unknown_fields() -> Result<()> {
        let addresses = json!([{"ifindex":3,"ifname":"eth0","addr_info":[
            {"local":"192.0.2.1","prefixlen":24,"scope":"global"},
            {"local":"2001:db8::1","prefixlen":64,"scope":"global","temporary":true}
        ]}]);
        let projection = linux_network_projection("addresses", &addresses)?;
        assert_eq!(
            projection[1].value,
            r#"["local","prefixlen","scope","temporary"]"#
        );
        assert_eq!(
            projection[2].value,
            r#"[3,"eth0",[["192.0.2.1",24,"global",null],["2001:db8::1",64,"global",true]]]"#
        );
        let links = json!([{"ifindex":3,"ifname":"eth0","stats64":{"rx":{"bytes":4294967297u64,"errors":2},"tx":{"bytes":4,"errors":0}}}]);
        let projection = linux_network_projection("links", &links)?;
        assert_eq!(projection[1].value, r#"["bytes","errors"]"#);
        assert_eq!(
            projection[2].value,
            r#"[3,"eth0",null,null,null,null,{"rx":[4294967297,2],"tx":[4,0]},null]"#
        );
        let routes = json!((0..9)
            .map(|_| json!({"dst":"default","gateway":"192.0.2.1","dev":"eth0"}))
            .collect::<Vec<_>>());
        let projection = linux_network_projection("routes", &routes)?;
        assert_eq!(projection.len(), 3);
        let first: Value = serde_json::from_str(&projection[1].value)?;
        let last: Value = serde_json::from_str(&projection[2].value)?;
        assert_eq!(first.as_array().map(Vec::len), Some(8));
        assert_eq!(
            last,
            json!([[
                "default",
                "192.0.2.1",
                "eth0",
                null,
                null,
                null,
                null,
                null,
                null,
                null
            ]])
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn child_deadline_covers_closed_stdout_and_descendant_pipe_owners() {
        for script in ["exec 1>&-; sleep 5", "sleep 5 & wait"] {
            let start = Instant::now();
            assert!(command(
                Path::new("/bin/sh"),
                &["-c", script],
                start + Duration::from_millis(50)
            )
            .is_err());
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    fn selected_projection_refuses_oversize_and_keeps_native_unknowns() -> Result<()> {
        assert!(rows(&json!([{}])).is_ok());
        assert!(rows(&json!(vec![json!({}); 33])).is_err());
        assert!(rows(&json!({"items":[]})).is_err());
        let observation = entry("gpu/0", json!({"uuid":"GPU-test","power_draw_w":"[N/A]"}))?;
        assert!(observation.value.contains("[N/A]"));
        assert_eq!(
            collect("unregistered", Instant::now() + Duration::from_secs(1)),
            Err(UnavailableReason::NotSupported)
        );
        Ok(())
    }
}
