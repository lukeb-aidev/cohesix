// CLASSIFICATION: COMMUNITY
// Filename: cohtrace_cli.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-12-01

use assert_cmd::Command;
use cohesix::orchestrator::protocol::{GpuTelemetry, HeartbeatRequest, JoinRequest};
use cohesix::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
use std::fs;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

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
    let rt = Runtime::new().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let guard = rt.enter();
    let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();
    drop(guard);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let orchestrator = QueenOrchestrator::new(5, SchedulePolicy::RoundRobin);
    let server = rt.spawn(
        orchestrator
            .clone()
            .serve_with_listener(tokio_listener, async {
                let _ = shutdown_rx.await;
            }),
    );

    let endpoint = format!("http://{}", addr);
    rt.block_on(async {
        let mut client = QueenOrchestrator::connect_client(&endpoint).await.unwrap();
        client
            .join(JoinRequest {
                worker_id: "worker-x".into(),
                ip: "10.0.0.5".into(),
                role: "DroneWorker".into(),
                capabilities: vec!["cuda".into()],
                trust: "green".into(),
            })
            .await
            .unwrap();
        client
            .heartbeat(HeartbeatRequest {
                worker_id: "worker-x".into(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                status: "running".into(),
                gpu: Some(GpuTelemetry {
                    perf_watt: 10.0,
                    mem_total: 16,
                    mem_free: 8,
                    last_temp: 55,
                    gpu_capacity: 10,
                    current_load: 2,
                    latency_score: 3,
                }),
            })
            .await
            .unwrap();
    });

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("cloud")
        .env("COHESIX_ORCH_ADDR", &endpoint)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace cloud");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Queen ID"));
    assert!(stdout.contains("worker-x (DroneWorker)"));
    assert!(stdout.contains("status: running"));

    let _ = shutdown_tx.send(());
    rt.block_on(async {
        server.await.unwrap().unwrap();
    });
}
