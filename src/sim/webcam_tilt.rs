// CLASSIFICATION: COMMUNITY
// Filename: webcam_tilt.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use crate::prelude::*;
/// Map webcam input to a beam balance simulation.
use rapier3d::prelude::*;
use serde::Serialize;
use std::fs;

#[derive(Serialize)]
struct TiltTrace {
    offset: f32,
    angle: f32,
}

/// Run the webcam tilt simulation using an optional image path.
pub fn run(image: Option<&str>) {
    let path = image.unwrap_or("/srv/webcam/frame.jpg");
    let img = image::open(path).unwrap_or_else(|_| image::DynamicImage::new_luma8(1, 1));
    let offset = compute_offset(&img);

    let mut pipeline = PhysicsPipeline::new();
    let gravity = vector![0.0, 0.0, 0.0];
    let params = IntegrationParameters::default();
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut island = IslandManager::new();
    let mut broad = BroadPhase::new();
    let mut narrow = NarrowPhase::new();
    let mut joints = ImpulseJointSet::new();
    let mut multi = MultibodyJointSet::new();
    let mut ccd = CCDSolver::new();
    let mut query = QueryPipeline::new();

    let ground = RigidBodyBuilder::fixed().build();
    let ground_handle = bodies.insert(ground);

    let beam = RigidBodyBuilder::dynamic().build();
    let beam_handle = bodies.insert(beam);
    colliders.insert_with_parent(
        ColliderBuilder::cuboid(1.0, 0.1, 0.1).build(),
        beam_handle,
        &mut bodies,
    );
    joints.insert(
        beam_handle,
        ground_handle,
        RevoluteJoint::new(UnitVector::new_normalize(vector![0.0, 0.0, 1.0])),
        true,
    );

    for _ in 0..20 {
        bodies[beam_handle].add_force(vector![offset * 5.0, 0.0, 0.0], true);
        pipeline.step(
            &gravity,
            &params,
            &mut island,
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
    }

    let angle = bodies[beam_handle].rotation().euler_angles().0;
    fs::create_dir_all("/trace").ok();
    let trace = TiltTrace { offset, angle };
    let json = serde_json::to_string(&trace).unwrap();
    fs::write("/trace/last_sim.json", json).ok();
}

fn compute_offset(img: &image::DynamicImage) -> f32 {
    let gray = img.to_luma8();
    let w = gray.width() as f32;
    let mut sum = 0.0;
    let mut total = 0.0;
    for (x, _, p) in gray.enumerate_pixels() {
        let v = p[0] as f32;
        sum += x as f32 * v;
        total += v;
    }
    if total == 0.0 {
        0.0
    } else {
        (sum / total) / w - 0.5
    }
}
