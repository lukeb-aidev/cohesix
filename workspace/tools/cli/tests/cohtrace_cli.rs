// CLASSIFICATION: COMMUNITY
// Filename: cohtrace_cli.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-12-01

use assert_cmd::Command;
use std::fs;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

#[test]
fn diff_command_outputs_changes() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().join("snapshots");
    fs::create_dir_all(&base).unwrap();

    let older = base.join("worker.json");
    fs::write(
        &older,
        r#"{"worker_id":"alpha","timestamp":1,"sim":{"mode":"idle"}}"#,
    )
    .unwrap();
    thread::sleep(Duration::from_millis(10));
    let newer = base.join("worker_new.json");
    fs::write(
        &newer,
        r#"{"worker_id":"alpha","timestamp":2,"sim":{"mode":"active"}}"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("diff")
        .env("SNAPSHOT_BASE", &base)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace diff");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Comparing snapshots"));
    assert!(stdout.contains("sim.mode"));
}

#[test]
fn cloud_command_outputs_registry() {
    let tmp = tempdir().unwrap();
    let srv_root = tmp.path().join("srv");
    fs::create_dir_all(srv_root.join("cloud")).unwrap();
    fs::create_dir_all(srv_root.join("agents")).unwrap();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cloud_state = format!(
        "{{\"queen_id\":\"queen-1\",\"validator\":true,\"role\":\"QueenPrimary\",\"ts\":{now},\"worker_count\":1}}"
    );
    fs::write(srv_root.join("cloud/state.json"), cloud_state).unwrap();
    fs::write(
        srv_root.join("agents/active.json"),
        r#"[{"worker_id":"worker-x","role":"DroneWorker","status":"running","ip":"10.0.0.5"}]"#,
    )
    .unwrap();
    fs::write(
        srv_root.join("agents/agent_table.json"),
        format!("[{{\"id\":\"worker-x\",\"last_heartbeat\":{}}}]", now),
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("cloud")
        .env("COHESIX_SRV_ROOT", &srv_root)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace cloud");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Queen ID: queen-1"));
    assert!(stdout.contains("worker-x (DroneWorker)"));
    assert!(stdout.contains("healthy"));
}
