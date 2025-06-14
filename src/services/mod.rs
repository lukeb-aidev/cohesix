// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.2
// Date Modified: 2025-07-23
// Author: Lukas Bower

//! Services Module
//!
//! Defines basic runtime services for Cohesix including telemetry reporting,
//! sandbox enforcement, health monitoring and IPC via the 9P protocol.

pub mod devd;
pub mod gpuinfo;
pub mod health;
pub mod ipc;
pub mod nswatch;
pub mod sandbox;
pub mod telemetry;
pub mod webcam;

/// Generic interface implemented by all runtime services.
pub trait Service {
    /// Return the service name used for logging.
    fn name(&self) -> &'static str;
    /// Initialize the service. Called during system startup.
    fn init(&mut self);
    /// Shut down the service gracefully.
    fn shutdown(&mut self);
}

/// Initialize all registered services under the `/srv/` namespace.
pub fn initialize_services() {
    println!("[services] initializing telemetry, sandbox, health, IPC, webcam, GPU info, devd and nswatch ...");
    let mut services: Vec<Box<dyn Service>> = vec![
        Box::new(telemetry::TelemetryService::default()),
        Box::new(sandbox::SandboxService::default()),
        Box::new(health::HealthService::default()),
        Box::new(ipc::IpcService::default()),
        Box::new(webcam::WebcamService::default()),
        Box::new(gpuinfo::GpuInfoService::default()),
        Box::new(devd::DevdService::default()),
        Box::new(nswatch::NsWatchService::default()),
    ];
    for svc in services.iter_mut() {
        println!("[services] starting {}", svc.name());
        svc.init();
    }
}
