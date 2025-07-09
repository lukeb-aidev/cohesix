// CLASSIFICATION: COMMUNITY
// Filename: cloud.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-09-01

use cohesix::cloud::orchestrator::CloudOrchestrator;
use cohesix::CohError;

/// Simple launcher for the cloud orchestrator service.
fn main() -> Result<(), CohError> {
    let url = std::env::var("CLOUD_URL").unwrap_or_else(|_| "http://127.0.0.1:4070".into());
    CloudOrchestrator::start(&url)?;
    // Keep process alive to continue heartbeats and command listener
    std::thread::park();
    Ok(())
}
