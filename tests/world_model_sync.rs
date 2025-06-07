// CLASSIFICATION: COMMUNITY
// Filename: world_model_sync.rs v0.1
// Date Modified: 2025-07-08
// Author: Cohesix Codex

use cohesix::queen::sync_daemon::QueenSyncDaemon;
use cohesix::worker::world_sync::WorkerWorldSync;
use cohesix::world_model::{Entity, WorldModelSnapshot};
use std::fs;
use tempfile::tempdir;

#[test]
fn world_model_sync_basic() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("/srv/world_model").unwrap();

    let snap = WorldModelSnapshot {
        version: 1,
        entities: vec![Entity { id: "e1".into(), position: [1.0, 2.0, 3.0], velocity: [0.0; 3], force: [0.0; 3] }],
        agent_state: "idle".into(),
        active_goals: vec!["g1".into()],
        role: "QueenPrimary".into(),
        gpu_hash: None,
    };
    snap.save("/srv/world_model/world.json").unwrap();

    let mut daemon = QueenSyncDaemon::new();
    daemon.add_worker("w1");
    daemon.push_diff(&snap);

    let path = "/srv/world_sync/w1.json";
    assert!(fs::metadata(path).is_ok());

    WorkerWorldSync::apply(path).unwrap();
    let synced = WorldModelSnapshot::load("/sim/world.json").unwrap();
    assert_eq!(synced.version, 1);
}

