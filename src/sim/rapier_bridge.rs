// CLASSIFICATION: COMMUNITY
// Filename: rapier_bridge.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-06-25

//! Rapier physics engine bridge exposing a simple command interface.
//!
//! Commands are written to `/sim/commands` and state snapshots are
//! updated to `/sim/state`. All transitions are logged to
//! `/srv/trace/sim.log`.

use crate::runtime::ServiceRegistry;
use rapier3d::prelude::*;
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

/// Commands sent to the simulation loop.
pub enum SimCommand {
    AddSphere { radius: f32, position: Vector<Real> },
    ApplyForce { id: RigidBodyHandle, force: Vector<Real> },
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
        ServiceRegistry::register_service("sim", "/sim");
        Self { tx }
    }

    /// Send a command to the simulation thread.
    pub fn send(&self, cmd: SimCommand) {
        let _ = self.tx.send(cmd);
    }
}

fn simulation_loop(rx: Receiver<SimCommand>) {
    fs::create_dir_all("/srv/trace").ok();
    fs::create_dir_all("sim").ok();
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
    let mut step = 0u64;

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
            &(),
            &(),
        );

        write_state(&bodies, step);
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
            handle,
            pos.x,
            pos.y,
            pos.z,
            vel.x,
            vel.y,
            vel.z,
            rot.i,
            rot.j,
            rot.k,
            rot.w
        ));
    }
    let _ = fs::write("sim/state", &out);
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
    let state = fs::read_to_string("sim/state").unwrap_or_default();
    let line = state.lines().next().unwrap_or("");
    let parts: Vec<&str> = line.split_whitespace().collect();
    let id = parts.get(0).cloned().unwrap_or("");
    SimObject {
        id: RigidBodyHandle::from_raw_parts(id.trim_matches(|c| c == '(' || c == ')').parse().unwrap_or(0), 0),
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
