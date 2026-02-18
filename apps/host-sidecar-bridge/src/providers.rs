// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Normalize provider command output into bounded /host status line formats.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! Provider output normalization helpers shared by host-sidecar components.

/// Parsed systemd status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdStatus {
    /// Active state (`active`, `inactive`, ...).
    pub state: String,
    /// Sub-state (`running`, `dead`, ...).
    pub sub: String,
}

/// Parse `systemctl show --property=ActiveState,SubState` output.
#[must_use]
pub fn parse_systemd_show_output(text: &str) -> Option<SystemdStatus> {
    let mut state = None;
    let mut sub = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("ActiveState=") {
            state = Some(sanitize_value(value));
        } else if let Some(value) = line.strip_prefix("SubState=") {
            sub = Some(sanitize_value(value));
        }
    }
    Some(SystemdStatus {
        state: state?,
        sub: sub?,
    })
}

/// Render a normalized systemd status line.
#[must_use]
pub fn format_systemd_status_line(status: &SystemdStatus) -> String {
    format!(
        "state={} sub={}",
        sanitize_value(status.state.as_str()),
        sanitize_value(status.sub.as_str())
    )
}

/// Parsed Docker status snapshot from `docker info --format`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerStatus {
    /// Docker server version.
    pub version: String,
    /// Total containers.
    pub containers: String,
    /// Running containers.
    pub running: String,
    /// Paused containers.
    pub paused: String,
    /// Stopped containers.
    pub stopped: String,
}

/// Parse Docker info output in the canonical five-token format.
#[must_use]
pub fn parse_docker_info_output(text: &str) -> Option<DockerStatus> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 5 {
        return None;
    }
    Some(DockerStatus {
        version: sanitize_value(tokens[0]),
        containers: sanitize_value(tokens[1]),
        running: sanitize_value(tokens[2]),
        paused: sanitize_value(tokens[3]),
        stopped: sanitize_value(tokens[4]),
    })
}

/// Render a normalized Docker status line.
#[must_use]
pub fn format_docker_status_line(status: &DockerStatus) -> String {
    format!(
        "version={} containers={} running={} paused={} stopped={}",
        sanitize_value(status.version.as_str()),
        sanitize_value(status.containers.as_str()),
        sanitize_value(status.running.as_str()),
        sanitize_value(status.paused.as_str()),
        sanitize_value(status.stopped.as_str())
    )
}

fn sanitize_value(input: &str) -> String {
    let trimmed = input.trim();
    let mut out = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/' | ',') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_systemd_output() {
        let parsed = parse_systemd_show_output("ActiveState=active\nSubState=running\n")
            .expect("parse");
        assert_eq!(parsed.state, "active");
        assert_eq!(parsed.sub, "running");
        assert_eq!(
            format_systemd_status_line(&parsed),
            "state=active sub=running"
        );
    }

    #[test]
    fn parse_docker_output() {
        let parsed = parse_docker_info_output("25.0.0 10 6 1 3").expect("parse");
        assert_eq!(parsed.version, "25.0.0");
        assert_eq!(
            format_docker_status_line(&parsed),
            "version=25.0.0 containers=10 running=6 paused=1 stopped=3"
        );
    }
}
