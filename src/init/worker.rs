// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.6
// Author: Lukas Bower
// Date Modified: 2026-02-21
// Uses rand for trace IDs; disabled for UEFI builds where getrandom isn't available.

//! DroneWorker role initialisation.
use std::fs::{self, OpenOptions};
use std::io::Write;
use crate::plan9::namespace::NamespaceLoader;
use cohesix_9p::fs::InMemoryFs;
use crate::utils::tiny_rng::TinyRng;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/srv/devlog") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

fn init_physics() {
    log("[worker] init physics engine");
    #[cfg(feature = "rapier")]
    {
        use rapier3d::prelude::*;
        fs::create_dir_all("/sim").ok();
        let mut pipeline = PhysicsPipeline::new();
        let gravity = vector![0.0, -9.81, 0.0];
        let params = IntegrationParameters::default();
        let mut islands = IslandManager::new();
        let mut broad = BroadPhase::new();
        let mut narrow = NarrowPhase::new();
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        let mut joints = ImpulseJointSet::new();
        let mut multi = MultibodyJointSet::new();
        let mut ccd = CCDSolver::new();
        let mut query = QueryPipeline::new();
        let body = RigidBodyBuilder::dynamic()
            .translation(vector![0.0, 1.0, 0.0])
            .build();
        let handle = bodies.insert(body);
        colliders.insert_with_parent(ColliderBuilder::ball(1.0).build(), handle, &mut bodies);
        pipeline.step(
            &gravity,
            &params,
            &mut islands,
            &mut broad,
            &mut narrow,
            &mut bodies,
            &mut colliders,
            &mut joints,
            &mut multi,
            &mut ccd,
            Some(&mut query),
            &(),
            &(),
        );
        log("[worker] physics engine initialized");
    }
}

fn run_cuda_demo() {
    fs::create_dir_all("/srv/logs").ok();
    let log_path = "/srv/logs/cuda_demo.log";
    if std::env::var("COH_GPU").unwrap_or_default() == "0" {
        return;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
        let demo = "/srv/cuda/cuda_infer";
        if std::path::Path::new(demo).is_file() {
            match std::process::Command::new(demo).output() {
                Ok(out) => {
                    let _ = f.write_all(&out.stdout);
                    let _ = f.write_all(&out.stderr);
                }
                Err(e) => {
                    let _ = writeln!(f, "error: {e}");
                }
            }
        } else {
            let _ = writeln!(f, "demo binary not found");
        }
    }
}

/// Entry point for the DroneWorker role.
pub fn start() {
    init_physics();

    let mut ns = NamespaceLoader::load().unwrap_or_default();
    let _ = NamespaceLoader::apply(&mut ns);

    let fs = InMemoryFs::new();
    fs.mount("/srv/cuda");
    fs.mount("/srv/shell");
    fs.mount("/srv/diag");

    fs::create_dir_all("/srv").ok();
    for p in ["/srv/cuda", "/srv/sim", "/srv/shell", "/srv/telemetry"] {
        let _ = fs::write(p, "ready");
    }

    run_cuda_demo();

    // expose runtime metadata to other agents
    let role = std::env::var("COH_ROLE").unwrap_or_else(|_| "unknown".into());
    fs::create_dir_all("/srv/agent_meta").ok();
    fs::write("/srv/agent_meta/role.txt", &role).ok();
    fs::write("/srv/agent_meta/uptime.txt", "0").ok();
    fs::write("/srv/agent_meta/last_goal.json", "null").ok();
    let mut rng = TinyRng::new(0xFACEFEED);
    let trace_id = format!("{:08x}", rng.next_u32());
    fs::write("/srv/agent_meta/trace_id.txt", trace_id).ok();

    log("[worker] services ready");
}
