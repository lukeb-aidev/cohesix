// CLASSIFICATION: COMMUNITY
// Filename: physics_adapter.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::prelude::*;

/// Adapter for running Rapier-based physics simulations.
use rapier3d::prelude::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::thread;
use std::time::Duration;

pub struct PhysicsAdapter {
    pipeline: PhysicsPipeline,
    bodies: RigidBodySet,
    colliders: ColliderSet,
}

impl Default for PhysicsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PhysicsAdapter {
    /// Create a new adapter with empty world.
    pub fn new() -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
        }
    }

    /// Simulate a single step with gravity.
    pub fn step(&mut self) {
        let gravity = vector![0.0, -9.81, 0.0];
        let params = IntegrationParameters::default();
        let mut island_manager = IslandManager::new();
        let mut broad = BroadPhase::new();
        let mut narrow = NarrowPhase::new();
        let mut joints = ImpulseJointSet::new();
        let mut multibody = MultibodyJointSet::new();
        let mut ccd = CCDSolver::new();
        let mut query = QueryPipeline::new();
        self.pipeline.step(
            &gravity,
            &params,
            &mut island_manager,
            &mut broad,
            &mut narrow,
            &mut self.bodies,
            &mut self.colliders,
            &mut joints,
            &mut multibody,
            &mut ccd,
            Some(&mut query),
            &(),
            &(),
        );
    }

    /// Add a BalanceBot model to the world.
    pub fn add_balance_bot(&mut self) -> RigidBodyHandle {
        let body = RigidBodyBuilder::dynamic()
            .translation(vector![0.0, 1.0, 0.0])
            .build();
        let handle = self.bodies.insert(body);
        let collider = ColliderBuilder::cuboid(0.2, 1.0, 0.2).build();
        self.colliders
            .insert_with_parent(collider, handle, &mut self.bodies);
        handle
    }

    /// Run a BalanceBot simulation and log positions.
    pub fn run_balance_bot(mut self, steps: u32) {
        fs::create_dir_all("/srv/physics").ok();
        let bot = self.add_balance_bot();
        for step in 0..steps {
            self.step();
            if let Some(rb) = self.bodies.get(bot) {
                let pos = rb.translation();
                let line = format!("{} {:.3} {:.3} {:.3}\n", step, pos.x, pos.y, pos.z);
                let _ = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/srv/physics/balancebot.log")
                    .and_then(|mut f| f.write_all(line.as_bytes()));
            }
            thread::sleep(Duration::from_millis(16));
        }
    }
}
