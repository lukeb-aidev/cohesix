// CLASSIFICATION: COMMUNITY
// Filename: rapier_bridge.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Wrapper around the Rapier physics engine exposing a simple Plan9-style interface.

use rapier3d::prelude::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::mpsc::Sender as StdSender;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Commands that can be sent to the simulation loop.
pub enum SimCommand {
    AddSphere { radius: f32, position: Vector<Real> },
    ApplyForce { id: RigidBodyHandle, force: Vector<Real> },
}

/// Bridge structure holding the command channel sender.
pub struct SimBridge {
    tx: Sender<SimCommand>,
    telemetry: StdSender<String>,
}

impl SimBridge {
    /// Start the simulation loop in a background thread.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let (ttx, _) = mpsc::channel();
        let ttx_thread = ttx.clone();
        thread::spawn(move || simulation_loop(rx, ttx_thread));
        Self { tx, telemetry: ttx }
    }

    /// Send a command to the simulation thread.
    pub fn send(&self, cmd: SimCommand) {
        let _ = self.tx.send(cmd);
    }

    pub fn telemetry_sender(&self) -> StdSender<String> {
        self.telemetry.clone()
    }
}

fn simulation_loop(rx: Receiver<SimCommand>, telemetry: StdSender<String>) {
    let mut pipeline = PhysicsPipeline::new();
    let gravity = vector![0.0, -9.81, 0.0];
    let mut integration_parameters = IntegrationParameters::default();
    let mut broad_phase = BroadPhase::new();
    let mut narrow_phase = NarrowPhase::new();
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut island_manager = IslandManager::new();
    let mut impulse_joints = ImpulseJointSet::new();
    let mut multibody_joints = MultibodyJointSet::new();
    let mut ccd_solver = CCDSolver::new();

    fs::create_dir_all("sim").ok();
    let mut step = 0u64;
    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                SimCommand::AddSphere { radius, position } => {
                    let body = RigidBodyBuilder::dynamic().translation(position).build();
                    let handle = bodies.insert(body);
                    let collider = ColliderBuilder::ball(radius).build();
                    colliders.insert_with_parent(collider, handle, &mut bodies);
                    append_trace(format!("added sphere {:?}\n", handle));
                }
                SimCommand::ApplyForce { id, force } => {
                    if let Some(body) = bodies.get_mut(id) {
                        body.add_force(force, true);
                        append_trace(format!("force {:?} -> {:?}\n", force, id));
                    }
                }
            }
        }

        let mut query_pipeline = QueryPipeline::new();
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

        let state = write_state(&bodies, step);
        let _ = telemetry.send(state.clone());
        step += 1;
        thread::sleep(Duration::from_millis(16));
    }
}

fn write_state(bodies: &RigidBodySet, step: u64) -> String {
    let mut out = String::new();
    for (handle, body) in bodies.iter() {
        let pos = body.translation();
        out.push_str(&format!("{:?}: [{}, {}, {}]\n", handle, pos.x, pos.y, pos.z));
    }
    let _ = fs::write("sim/state", &out);
    append_trace(format!("step {}\n", step));
    out
}

fn append_trace(line: String) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("sim/trace") {
        let _ = f.write_all(line.as_bytes());
    }
}
