// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-12-01

use chrono::{TimeZone, Utc};
use clap::{Parser, Subcommand};
use cohesix::CohError;
use humantime::format_duration;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Trace inspection utilities")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected workers
    List,
    /// Push a trace file to the queen
    PushTrace { worker_id: String, path: PathBuf },
    /// Compare the two most recent snapshots under /history/snapshots
    Diff,
    /// Show queen and worker state from the active registry
    Cloud,
}

fn append_summary(entry: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("VALIDATION_SUMMARY.md")
    {
        let ts = Utc::now().to_rfc3339();
        let _ = writeln!(f, "- {ts} {entry}");
    }
}

fn cmd_list() -> Result<(), CohError> {
    let base = Path::new("/srv/workers");
    if base.exists() {
        for ent in fs::read_dir(base)? {
            let name = ent?.file_name();
            println!("worker: {}", name.to_string_lossy());
        }
    } else {
        println!("no workers directory");
    }
    append_summary("cohtrace list ok");
    Ok(())
}

fn cmd_push(worker_id: String, path: PathBuf) -> Result<(), CohError> {
    let dest = Path::new("/trace").join(&worker_id);
    fs::create_dir_all(&dest)?;
    fs::copy(&path, dest.join("sim.json"))?;
    println!("trace stored for {}", worker_id);
    append_summary(&format!(
        "cohtrace push_trace {} {}",
        worker_id,
        path.display()
    ));
    Ok(())
}

struct SnapshotInfo {
    path: PathBuf,
    worker_id: Option<String>,
    timestamp: Option<i64>,
    value: Value,
}

struct SnapshotDiff {
    newer: SnapshotInfo,
    older: SnapshotInfo,
    lines: Vec<String>,
}

#[derive(Default, Deserialize)]
struct CloudStateRecord {
    #[serde(default)]
    queen_id: Option<String>,
    #[serde(default)]
    validator: Option<bool>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    ts: Option<u64>,
    #[serde(default)]
    worker_count: Option<u64>,
}

#[derive(Default, Deserialize)]
struct ActiveWorkerRecord {
    #[serde(default)]
    worker_id: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    ip: String,
}

#[derive(Default, Deserialize)]
struct AgentTableRecord {
    #[serde(default)]
    id: String,
    #[serde(default)]
    last_heartbeat: Option<u64>,
}

struct QueenInfo {
    queen_id: Option<String>,
    role: Option<String>,
    validator_active: Option<bool>,
    last_heartbeat: Option<u64>,
    worker_count: Option<usize>,
}

struct WorkerInfo {
    id: String,
    role: String,
    status: String,
    ip: String,
    last_heartbeat: Option<u64>,
    age: Option<u64>,
    healthy: Option<bool>,
}

struct CloudReport {
    queen: Option<QueenInfo>,
    workers: Vec<WorkerInfo>,
}

fn snapshot_base() -> PathBuf {
    std::env::var("SNAPSHOT_BASE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/history/snapshots"))
}

fn srv_root() -> PathBuf {
    std::env::var("COHESIX_SRV_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/srv"))
}

fn diff_latest_snapshots_at(base: &Path) -> Result<Option<SnapshotDiff>, CohError> {
    if !base.exists() {
        return Ok(None);
    }
    let mut entries: Vec<(u128, PathBuf)> = WalkDir::new(base)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("json"))
                .unwrap_or(false)
        })
        .filter_map(|entry| {
            let ts = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|st| st.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            Some((ts, entry.into_path()))
        })
        .collect();

    if entries.len() < 2 {
        return Ok(None);
    }

    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

    let mut take = entries.into_iter().map(|(_, path)| path);
    let newer_path = take.next().unwrap();
    let older_path = take.next().unwrap();

    let newer_value: Value = serde_json::from_str(&fs::read_to_string(&newer_path)?)?;
    let older_value: Value = serde_json::from_str(&fs::read_to_string(&older_path)?)?;

    let newer = SnapshotInfo {
        worker_id: extract_worker_id(&newer_value),
        timestamp: extract_timestamp(&newer_value),
        path: newer_path,
        value: newer_value,
    };
    let older = SnapshotInfo {
        worker_id: extract_worker_id(&older_value),
        timestamp: extract_timestamp(&older_value),
        path: older_path,
        value: older_value,
    };

    let lines = diff_values(&older.value, &newer.value);
    Ok(Some(SnapshotDiff {
        newer,
        older,
        lines,
    }))
}

fn extract_worker_id(value: &Value) -> Option<String> {
    value
        .get("worker_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_timestamp(value: &Value) -> Option<i64> {
    if let Some(ts) = value.get("timestamp").and_then(|v| v.as_i64()) {
        Some(ts)
    } else {
        value
            .get("timestamp")
            .and_then(|v| v.as_u64())
            .and_then(|u| i64::try_from(u).ok())
    }
}

fn diff_values(old: &Value, new: &Value) -> Vec<String> {
    let mut old_map = BTreeMap::new();
    let mut new_map = BTreeMap::new();
    flatten_value(old, "", &mut old_map);
    flatten_value(new, "", &mut new_map);

    let mut keys: BTreeSet<String> = old_map.keys().cloned().collect();
    keys.extend(new_map.keys().cloned());

    let mut lines = Vec::new();
    for key in keys {
        match (old_map.get(&key), new_map.get(&key)) {
            (Some(o), Some(n)) if o == n => {}
            (Some(o), Some(n)) => {
                lines.push(format!("~ {key}: {o} -> {n}"));
            }
            (None, Some(n)) => {
                lines.push(format!("+ {key}: {n}"));
            }
            (Some(o), None) => {
                lines.push(format!("- {key}: {o}"));
            }
            (None, None) => {}
        }
    }
    lines
}

fn flatten_value(value: &Value, prefix: &str, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                out.insert(prefix_or_root(prefix), "{}".into());
            } else {
                for (key, val) in map {
                    let next = key_name(prefix, key);
                    flatten_value(val, &next, out);
                }
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                out.insert(prefix_or_root(prefix), "[]".into());
            } else {
                for (idx, val) in arr.iter().enumerate() {
                    let next = key_name(prefix, &format!("[{}]", idx));
                    flatten_value(val, &next, out);
                }
            }
        }
        _ => {
            let key = prefix_or_root(prefix);
            out.insert(key, simple_value(value));
        }
    }
}

fn prefix_or_root(prefix: &str) -> String {
    if prefix.is_empty() {
        "<root>".to_string()
    } else {
        prefix.to_string()
    }
}

fn key_name(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else if segment.starts_with('[') {
        format!("{prefix}{segment}")
    } else {
        format!("{prefix}.{segment}")
    }
}

fn simple_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<complex>".into()),
    }
}

fn cmd_diff() -> Result<(), CohError> {
    let base = snapshot_base();
    match diff_latest_snapshots_at(&base)? {
        None => {
            println!("no snapshots available under {}", base.display());
        }
        Some(diff) => {
            println!("Comparing snapshots:");
            println!(
                "  Newer: {}{}{}",
                diff.newer.path.display(),
                diff.newer
                    .worker_id
                    .as_ref()
                    .map(|w| format!(" (worker {w})"))
                    .unwrap_or_default(),
                diff.newer
                    .timestamp
                    .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                    .map(|dt| format!(" @ {}", dt.to_rfc3339()))
                    .unwrap_or_default()
            );
            println!(
                "  Older: {}{}{}",
                diff.older.path.display(),
                diff.older
                    .worker_id
                    .as_ref()
                    .map(|w| format!(" (worker {w})"))
                    .unwrap_or_default(),
                diff.older
                    .timestamp
                    .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                    .map(|dt| format!(" @ {}", dt.to_rfc3339()))
                    .unwrap_or_default()
            );
            if diff.lines.is_empty() {
                println!("No differences detected.");
            } else {
                println!("Changes:");
                for line in &diff.lines {
                    println!("  {line}");
                }
            }
            append_summary(&format!(
                "cohtrace diff {} {}",
                diff.newer.path.display(),
                diff.older.path.display()
            ));
        }
    }
    Ok(())
}

fn read_cloud_state(root: &Path) -> Option<QueenInfo> {
    let state_path = root.join("cloud/state.json");
    if let Ok(data) = fs::read_to_string(&state_path) {
        if let Ok(record) = serde_json::from_str::<CloudStateRecord>(&data) {
            return Some(QueenInfo {
                queen_id: record.queen_id.and_then(non_empty_string),
                role: record.role.and_then(non_empty_string),
                validator_active: record.validator,
                last_heartbeat: record.ts,
                worker_count: record.worker_count.map(|c| c as usize),
            });
        }
    }

    let queen_id = fs::read_to_string(root.join("cloud/queen_id"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let last_heartbeat = fs::read_to_string(root.join("cloud/last_heartbeat"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());

    if queen_id.is_some() || last_heartbeat.is_some() {
        Some(QueenInfo {
            queen_id,
            role: None,
            validator_active: None,
            last_heartbeat,
            worker_count: None,
        })
    } else {
        None
    }
}

fn non_empty_string(input: String) -> Option<String> {
    if input.trim().is_empty() {
        None
    } else {
        Some(input)
    }
}

fn read_agent_table(root: &Path) -> BTreeMap<String, AgentTableRecord> {
    let mut map = BTreeMap::new();
    let path = root.join("agents/agent_table.json");
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(entries) = serde_json::from_str::<Vec<AgentTableRecord>>(&data) {
            for entry in entries {
                if !entry.id.is_empty() {
                    map.insert(entry.id.clone(), entry);
                }
            }
        }
    }
    map
}

fn read_active_workers(root: &Path) -> Vec<ActiveWorkerRecord> {
    let path = root.join("agents/active.json");
    if let Ok(data) = fs::read_to_string(path) {
        if let Ok(entries) = serde_json::from_str::<Vec<ActiveWorkerRecord>>(&data) {
            return entries;
        }
    }
    Vec::new()
}

fn heartbeat_from_file(path: &Path) -> Option<u64> {
    if let Ok(contents) = fs::read_to_string(path) {
        contents.trim().parse::<u64>().ok()
    } else {
        fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    }
}

fn cloud_report(root: &Path) -> CloudReport {
    let queen = read_cloud_state(root);
    let agent_table = read_agent_table(root);
    let active_workers = read_active_workers(root);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut workers = Vec::new();
    for entry in active_workers {
        let id = if entry.worker_id.is_empty() {
            "unknown".to_string()
        } else {
            entry.worker_id.clone()
        };
        let agent_rec = agent_table.get(&id);
        let heartbeat = agent_rec
            .and_then(|rec| rec.last_heartbeat)
            .or_else(|| heartbeat_from_file(&root.join(format!("agents/{id}/heartbeat"))));
        let age = heartbeat.map(|hb| now.saturating_sub(hb));
        let healthy = age.map(|secs| secs <= 120);
        workers.push(WorkerInfo {
            id,
            role: entry.role,
            status: entry.status,
            ip: entry.ip,
            last_heartbeat: heartbeat,
            age,
            healthy,
        });
    }

    workers.sort_by(|a, b| a.id.cmp(&b.id));

    CloudReport { queen, workers }
}

fn cmd_cloud() -> Result<(), CohError> {
    let root = srv_root();
    let report = cloud_report(&root);

    if let Some(queen) = report.queen {
        println!("Queen status:");
        if let Some(id) = queen.queen_id {
            println!("  Queen ID: {id}");
        } else {
            println!("  Queen ID: unknown");
        }
        if let Some(role) = queen.role {
            println!("  Role: {role}");
        }
        if let Some(active) = queen.validator_active {
            println!(
                "  Validator: {}",
                if active { "active" } else { "inactive" }
            );
        }
        if let Some(ts) = queen.last_heartbeat {
            let human = Utc
                .timestamp_opt(ts as i64, 0)
                .single()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| ts.to_string());
            let ago = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|now| now.as_secs().saturating_sub(ts))
                .map(Duration::from_secs)
                .map(|d| format_duration(d).to_string())
                .unwrap_or_else(|| "unknown".into());
            println!("  Last heartbeat: {human} ({ago} ago)");
        }
        if let Some(count) = queen.worker_count {
            println!("  Reported workers: {count}");
        }
    } else {
        println!(
            "No queen heartbeat data under {}",
            root.join("cloud/state.json").display()
        );
    }

    if report.workers.is_empty() {
        println!("No active workers registered.");
    } else {
        println!("Workers:");
        for worker in &report.workers {
            let hb_line = if let Some(ts) = worker.last_heartbeat {
                let human = Utc
                    .timestamp_opt(ts as i64, 0)
                    .single()
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| ts.to_string());
                let ago = worker
                    .age
                    .map(Duration::from_secs)
                    .map(|d| format_duration(d).to_string())
                    .unwrap_or_else(|| "unknown".into());
                let health = worker
                    .healthy
                    .map(|ok| if ok { "healthy" } else { "stale" })
                    .unwrap_or("unknown");
                format!("last heartbeat: {human} ({ago} ago, {health})")
            } else {
                "last heartbeat: unknown".into()
            };
            let ip = if worker.ip.is_empty() {
                "unknown"
            } else {
                &worker.ip
            };
            println!(
                "  {} ({}) - status: {}, ip: {}, {}",
                worker.id, worker.role, worker.status, ip, hb_line
            );
        }
    }

    append_summary(&format!("cohtrace cloud {} workers", report.workers.len()));
    Ok(())
}

fn main() -> Result<(), CohError> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => cmd_list()?,
        Cmd::PushTrace { worker_id, path } => cmd_push(worker_id, path)?,
        Cmd::Diff => cmd_diff()?,
        Cmd::Cloud => cmd_cloud()?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;

    #[test]
    fn diff_detects_changes() {
        let tmp = tempdir().unwrap();
        let snaps = tmp.path().join("history/snapshots");
        fs::create_dir_all(&snaps).unwrap();
        let older = snaps.join("worker1.json");
        let newer = snaps.join("worker1_latest.json");
        fs::write(
            &older,
            r#"{"worker_id":"w1","timestamp":10,"value":1,"sim":{"mode":"idle"}}"#,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        fs::write(
            &newer,
            r#"{"worker_id":"w1","timestamp":11,"value":2,"sim":{"mode":"active"}}"#,
        )
        .unwrap();

        let diff = diff_latest_snapshots_at(&snaps).unwrap().unwrap();
        assert!(diff.lines.iter().any(|l| l.contains("sim.mode")));
        assert!(diff.lines.iter().any(|l| l.contains("timestamp")));
        assert_eq!(
            diff.newer.worker_id.as_deref(),
            Some("w1"),
            "newer worker id detected"
        );
    }

    #[test]
    fn cloud_reports_workers() {
        let tmp = tempdir().unwrap();
        let srv = tmp.path().join("srv");
        fs::create_dir_all(srv.join("cloud")).unwrap();
        fs::create_dir_all(srv.join("agents")).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cloud_state = format!(
            "{{\"queen_id\":\"queen-alpha\",\"validator\":true,\"role\":\"QueenPrimary\",\"ts\":{},\"worker_count\":2}}",
            now
        );
        fs::write(srv.join("cloud/state.json"), cloud_state).unwrap();
        let active = r#"[
            {"worker_id":"worker-a","role":"DroneWorker","status":"running","ip":"10.0.0.2"},
            {"worker_id":"worker-b","role":"SensorRelay","status":"stale","ip":"10.0.0.3"}
        ]"#;
        fs::write(srv.join("agents/active.json"), active).unwrap();
        let table = format!(
            "[{{\"id\":\"worker-a\",\"last_heartbeat\":{}}},{{\"id\":\"worker-b\",\"last_heartbeat\":{}}}]",
            now,
            now.saturating_sub(500)
        );
        fs::write(srv.join("agents/agent_table.json"), table).unwrap();

        let report = cloud_report(&srv);
        assert!(report.queen.is_some());
        assert_eq!(report.workers.len(), 2);
        let first = &report.workers[0];
        assert_eq!(first.id, "worker-a");
        assert_eq!(first.role, "DroneWorker");
        assert!(first.healthy.unwrap_or(false));
        let second = &report.workers[1];
        assert_eq!(second.id, "worker-b");
        assert!(second.age.unwrap() >= 500);
        assert_eq!(second.healthy, Some(false));
    }
}
