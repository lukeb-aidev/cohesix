// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify live SwarmUI rejects fixture credentials before opening a connection.
// Author: Lukas Bower

use cohesix_ticket::Role;
use std::net::TcpListener;
use swarmui::{SwarmUiConfig, SwarmUiConsoleBackend};

#[test]
fn swarmui_rejects_placeholders_before_connecting() -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    for token in [
        "changeme",
        "bootstrap",
        "worker",
        "worker-gpu",
        "worker-bus",
        "worker-lora",
    ] {
        let config = SwarmUiConfig::from_generated(std::env::temp_dir());
        let mut backend = SwarmUiConsoleBackend::new(config, "127.0.0.1", port, token);
        let transcript = backend.attach(Role::Queen, None);
        assert!(!transcript.ok, "placeholder must fail: {token}");
        assert!(transcript
            .lines
            .iter()
            .any(|line| line.contains("insecure placeholder")));
        assert_eq!(
            listener.accept().expect_err("no socket may open").kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    Ok(())
}
