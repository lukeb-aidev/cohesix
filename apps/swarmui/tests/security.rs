// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI unauthorized ticket handling and audit logs.
// Author: Lukas Bower

use anyhow::Result;
use cohsh::client::InProcessTransport;
use cohesix_ticket::Role;
use nine_door::NineDoor;
use swarmui::{SwarmUiBackend, SwarmUiConfig, SwarmUiTransportFactory};

struct InProcessFactory {
    server: NineDoor,
}

impl SwarmUiTransportFactory for InProcessFactory {
    type Transport = InProcessTransport;

    fn connect(&self) -> Result<Self::Transport, swarmui::SwarmUiError> {
        let connection = self
            .server
            .connect()
            .map_err(|err| swarmui::SwarmUiError::Transport(err.to_string()))?;
        Ok(InProcessTransport::new(connection))
    }
}

#[test]
fn unauthorized_ticket_returns_err_and_logs_audit() -> Result<()> {
    let server = NineDoor::new();
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let transcript = backend.attach(Role::WorkerHeartbeat, None);
    assert!(!transcript.ok);
    assert!(transcript
        .lines
        .iter()
        .any(|line| line.starts_with("ERR ATTACH")));
    assert!(backend
        .audit_log()
        .iter()
        .any(|line| line.contains("audit swarmui.attach outcome=err")));
    Ok(())
}
