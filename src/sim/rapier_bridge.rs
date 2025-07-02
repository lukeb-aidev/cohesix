// CLASSIFICATION: COMMUNITY
// Filename: rapier_bridge.rs v0.6
// Author: Lukas Bower
// Date Modified: 2025-08-17
// Relies on rand and Rapier; omitted from UEFI builds.

use crate::prelude::*;
/// Rapier physics engine bridge exposing a simple command interface.
//
/// Commands are written to `/sim/commands` and state snapshots are
/// updated to `/sim/state`. All transitions are logged to
/// `/srv/trace/sim.log`.
use crate::runtime::ServiceRegistry;
use crate::utils::tiny_rng::TinyRng;
use rapier3d::na::UnitQuaternion;
use rapier3d::pipeline::QueryPipeline;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Simulation object state.
#[derive(Clone, Debug)]
pub struct SimObject {
    pub id: RigidBodyHandle,
    pub pos: Vector<Real>,
    pub vel: Vector<Real>,
    pub rot: UnitQuaternion<Real>,
}

/// Serialized body state for snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BodyState {
    pub index: u32,
    pub generation: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub rotation: [f32; 4],
}

/// Serialized snapshot of the entire simulation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimSnapshot {
    pub step: u64,
    pub bodies: Vec<BodyState>,
}

/// Commands sent to the simulation loop.
pub enum SimCommand {
    AddSphere {
        radius: f32,
        position: Vector<Real>,
    },
    ApplyForce {
        id: RigidBodyHandle,
        force: Vector<Real>,
    },
}

/// Simulation bridge handle.
pub struct SimBridge {
    tx: Sender<SimCommand>,
}

impl SimBridge {
    /// Start the simulation loop and register the `/sim` service.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || simulation_loop(rx));
        let _ = ServiceRegistry::register_service("sim", "/sim");
        Self { tx }
    }

    /// Send a command to the simulation thread.
    pub fn send(&self, cmd: SimCommand) {
        let _ = self.tx.send(cmd);
    }
}

fn simulation_loop(rx: Receiver<SimCommand>) {
    fs::create_dir_all("/srv/trace").ok();
    fs::create_dir_all("/sim").ok();
    let mut pipeline = PhysicsPipeline::new();
    let gravity = vector![0.0, -9.81, 0.0];
    let integration_parameters = IntegrationParameters::default();
    let mut broad_phase = BroadPhase::new();
    let mut narrow_phase = NarrowPhase::new();
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut island_manager = IslandManager::new();
    let mut impulse_joints = ImpulseJointSet::new();
    let mut multibody_joints = MultibodyJointSet::new();
    let mut ccd_solver = CCDSolver::new();
    let mut query_pipeline = QueryPipeline::new();
    let mut step = 0u64;
    if let Ok(data) = fs::read("/sim/world.json") {
        if let Ok(snap) = serde_json::from_slice::<SimSnapshot>(&data) {
            for b in snap.bodies {
                let body = RigidBodyBuilder::dynamic()
                    .translation(vector![b.position[0], b.position[1], b.position[2]])
                    .linvel(vector![b.velocity[0], b.velocity[1], b.velocity[2]])
                    .build();
                let handle = bodies.insert(body);
                let collider = ColliderBuilder::ball(1.0).build();
                colliders.insert_with_parent(collider, handle, &mut bodies);
            }
            step = snap.step;
        }
    }

    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                SimCommand::AddSphere { radius, position } => {
                    let body = RigidBodyBuilder::dynamic().translation(position).build();
                    let handle = bodies.insert(body);
                    let collider = ColliderBuilder::ball(radius).build();
                    colliders.insert_with_parent(collider, handle, &mut bodies);
                    log_transition(format!("added {:?}\n", handle));
                }
                SimCommand::ApplyForce { id, force } => {
                    if let Some(body) = bodies.get_mut(id) {
                        body.add_force(force, true);
                        log_transition(format!("force {:?} -> {:?}\n", force, id));
                    }
                }
            }
        }

        pipeline.step(
            &gravity,
            &integration_parameters,
            &mut island_manager,
            &mut broad_phase,
            &mut narrow_phase,
            &mut bodies,
            &mut colliders,
            &mut impulse_joints,
            &mut multibody_joints,
            &mut ccd_solver,
            Some(&mut query_pipeline),
            &(),
            &(),
        );

        write_state(&bodies, step);
        save_snapshot(&bodies, step);
        step += 1;
        thread::sleep(Duration::from_millis(16));
    }
}

fn write_state(bodies: &RigidBodySet, step: u64) {
    let mut out = String::new();
    for (handle, body) in bodies.iter() {
        let pos = body.translation();
        let vel = body.linvel();
        let rot = body.rotation();
        out.push_str(&format!(
            "{:?} [{:.2},{:.2},{:.2}] v[{:.2},{:.2},{:.2}] r[{:.2},{:.2},{:.2},{:.2}]\n",
            handle, pos.x, pos.y, pos.z, vel.x, vel.y, vel.z, rot.i, rot.j, rot.k, rot.w
        ));
    }
    let _ = fs::write("/sim/state", &out);
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/trace/sim.log")
        .and_then(|mut f| f.write_all(out.as_bytes()));
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/trace/sim.log")
        .and_then(|mut f| writeln!(f, "step {}", step));
}

fn save_snapshot(bodies: &RigidBodySet, step: u64) {
    let snap = collect_snapshot(bodies, step);
    if let Ok(data) = serde_json::to_vec_pretty(&snap) {
        let _ = fs::write("/sim/world.json", data);
    }
}

fn log_transition(line: String) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/trace/sim.log")
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Example gravity drop: adds a sphere and lets it fall for 3 steps.
pub fn example_gravity_drop() -> SimObject {
    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere {
        radius: 1.0,
        position: vector![0.0, 5.0, 0.0],
    });
    thread::sleep(Duration::from_millis(50));
    let state = fs::read_to_string("/sim/state").unwrap_or_default();
    let line = state.lines().next().unwrap_or("");
    let parts: Vec<&str> = line.split_whitespace().collect();
    let id = parts.first().cloned().unwrap_or("");
    SimObject {
        id: RigidBodyHandle::from_raw_parts(
            id.trim_matches(|c| c == '(' || c == ')')
                .parse()
                .unwrap_or(0),
            0,
        ),
        pos: vector![0.0, 0.0, 0.0],
        vel: vector![0.0, 0.0, 0.0],
        rot: UnitQuaternion::identity(),
    }
}

/// Example lateral push.
pub fn example_lateral_push() {
    let bridge = SimBridge::start();
    bridge.send(SimCommand::AddSphere {
        radius: 1.0,
        position: vector![0.0, 0.0, 0.0],
    });
    thread::sleep(Duration::from_millis(20));
    bridge.send(SimCommand::ApplyForce {
        id: RigidBodyHandle::from_raw_parts(0, 0),
        force: vector![10.0, 0.0, 0.0],
    });
}

/// Deterministic simulation harness used by tests.
pub fn deterministic_harness(seed: u64, steps: u32) -> Vec<SimSnapshot> {
    fs::create_dir_all("/srv/trace").ok();
    fs::create_dir_all("/sim").ok();

    let mut rng = TinyRng::new(seed);
    let mut pipeline = PhysicsPipeline::new();
    let gravity = vector![0.0, -9.81, 0.0];
    let params = IntegrationParameters::default();
    let mut broad = BroadPhase::new();
    let mut narrow = NarrowPhase::new();
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut islands = IslandManager::new();
    let mut joints = ImpulseJointSet::new();
    let mut mb = MultibodyJointSet::new();
    let mut ccd = CCDSolver::new();
    let mut query = QueryPipeline::new();

    let body = RigidBodyBuilder::dynamic()
        .translation(vector![0.0, 1.0, 0.0])
        .build();
    let handle = bodies.insert(body);
    let collider = ColliderBuilder::ball(1.0).build();
    colliders.insert_with_parent(collider, handle, &mut bodies);

    let mut out = Vec::new();

    for step in 0..steps {
        if let Some(b) = bodies.get_mut(handle) {
            let force = vector![
                rng.gen_range(-1.0, 1.0),
                rng.gen_range(-1.0, 1.0),
                rng.gen_range(-1.0, 1.0),
            ];
            b.add_force(force, true);
            log_transition(format!("force {:?} -> {:?}\n", force, handle));
        }

        pipeline.step(
            &gravity,
            &params,
            &mut islands,
            &mut broad,
            &mut narrow,
            &mut bodies,
            &mut colliders,
            &mut joints,
            &mut mb,
            &mut ccd,
            Some(&mut query),
            &(),
            &(),
        );
        write_state(&bodies, step as u64);
        save_snapshot(&bodies, step as u64);
        out.push(collect_snapshot(&bodies, step as u64));
    }

    out
}

fn collect_snapshot(bodies: &RigidBodySet, step: u64) -> SimSnapshot {
    let mut bodies_out = Vec::new();
    for (handle, body) in bodies.iter() {
        let (id, r#gen) = handle.into_raw_parts();
        bodies_out.push(BodyState {
            index: id,
            generation: r#gen,
            position: [
                body.translation().x,
                body.translation().y,
                body.translation().z,
            ],
            velocity: [body.linvel().x, body.linvel().y, body.linvel().z],
            rotation: [
                body.rotation().i,
                body.rotation().j,
                body.rotation().k,
                body.rotation().w,
            ],
        });
    }
    SimSnapshot {
        step,
        bodies: bodies_out,
    }
}
