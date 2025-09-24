// CLASSIFICATION: COMMUNITY
// Filename: protocol.rs v1.0
// Author: Lukas Bower
// Date Modified: 2029-01-15
#![cfg(feature = "std")]

/// Generated gRPC bindings for the orchestrator service.
#[allow(clippy::all)]
pub mod generated {
    tonic::include_proto!("cohesix.orchestrator");
}

pub use generated::orchestrator_service_client::OrchestratorServiceClient;
pub use generated::orchestrator_service_server::OrchestratorService;
pub use generated::orchestrator_service_server::OrchestratorServiceServer;
pub use generated::*;

/// Default endpoint used when no explicit orchestrator address is provided.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:50051";
