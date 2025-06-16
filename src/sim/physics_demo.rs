// CLASSIFICATION: COMMUNITY
// Filename: physics_demo.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22


//! Simple Rapier physics demo used by `cohrun physics_demo`.

use rapier3d::prelude::*;
use serde::Serialize;
use std::fs;

#[derive(Serialize)]
struct BodyState {
    pos: [f32; 3],
}

/// Run a minimal gravity simulation and log to `/trace/last_sim.json`.
pub fn run_demo() {
    fs::create_dir_all("/trace").ok();

    let mut pipeline = PhysicsPipeline::new();
    let gravity = vector![0.0, -9.81, 0.0];
    let params = IntegrationParameters::default();
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut island_manager = IslandManager::new();
    let mut broad = BroadPhase::new();
    let mut narrow = NarrowPhase::new();
    let mut joints = ImpulseJointSet::new();
    let mut multi = MultibodyJointSet::new();
    let mut ccd = CCDSolver::new();

    let handle = bodies.insert(RigidBodyBuilder::dynamic().translation(vector![0.0, 2.0, 0.0]).build());
    colliders.insert_with_parent(ColliderBuilder::cuboid(0.5, 0.5, 0.5).build(), handle, &mut bodies);

    let mut query = QueryPipeline::new();
    for _ in 0..10 {
        pipeline.step(&gravity, &params, &mut island_manager, &mut broad, &mut narrow, &mut bodies, &mut colliders, &mut joints, &mut multi, &mut ccd, Some(&mut query), &(), &());
    }

    if let Some(body) = bodies.get(handle) {
        let pos = body.translation();
        let state = BodyState { pos: [pos.x, pos.y, pos.z] };
        let json = serde_json::to_string(&state).unwrap();
        fs::write("/trace/last_sim.json", json).ok();
    }
}
