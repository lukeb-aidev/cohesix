// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Author: Lukas Bower
// Purpose: Resolve embedded manuals locally without transport or filesystem authority.

//! Canonical operator manuals shared with the SwarmUI console.

use anyhow::{bail, Result};

const MANUAL: &str = include_str!("manual.md");

/// Render one manual or the topic index. Aliases resolve to their canonical page.
pub fn render(topic: Option<&str>) -> Result<String> {
    let Some(topic) = topic else {
        let mut index = String::from("Cohesix manuals — use man <command>:\n");
        for page in MANUAL.split("\n## ").skip(1) {
            if let Some((name, _)) = page.split_once('\n') {
                index.push_str("  ");
                index.push_str(name);
                index.push('\n');
            }
        }
        index.push_str("  login (alias for attach)\n");
        return Ok(index);
    };
    if topic.len() > 64
        || !topic
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    {
        bail!("man requires one command name; use man to list topics");
    }
    let topic = if topic == "login" { "attach" } else { topic };
    for page in MANUAL.split("\n## ").skip(1) {
        if let Some((name, body)) = page.split_once('\n') {
            if name == topic {
                return Ok(body.trim().to_owned());
            }
        }
    }
    bail!("no manual for '{topic}'; use man to list topics")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_inventory_has_manuals_and_examples() {
        // Independent public dispatch inventory, including host-only verbs.
        for command in [
            "man",
            "help",
            "attach",
            "login",
            "detach",
            "quit",
            "ls",
            "cat",
            "tail",
            "log",
            "echo",
            "spawn",
            "kill",
            "bind",
            "mount",
            "lifecycle",
            "telemetry",
            "ping",
            "bi",
            "caps",
            "smp",
            "nettest",
            "netstats",
            "reboot",
            "test",
            "tcp-diag",
            "pool",
        ] {
            let page = render(Some(command)).unwrap();
            assert!(page.contains("EXAMPLES"), "missing examples for {command}");
            assert!(page.contains("SYNOPSIS"), "missing syntax for {command}");
        }
        assert_eq!(
            render(Some("login")).unwrap(),
            render(Some("attach")).unwrap()
        );
    }

    #[test]
    fn lookup_rejects_paths_unknown_names_and_extra_tokens() {
        for input in ["../spawn", "spawn gpu", "unknown", "SPAWN", "\0", ""] {
            assert!(render(Some(input)).is_err(), "accepted {input:?}");
        }
        assert!(render(None).unwrap().contains("  spawn\n"));
    }

    #[test]
    fn documented_spawn_examples_match_the_control_contract() {
        let page = render(Some("spawn")).unwrap();
        let cases = [
            ("spawn heartbeat ticks=100 ttl_s=120 ops=500",
             serde_json::json!({"spawn":"heartbeat","ticks":100,"budget":{"ttl_s":120,"ops":500}})),
            ("spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120 priority=1 budget_ttl_s=180 budget_ops=500",
             serde_json::json!({"spawn":"gpu","lease":{"gpu_id":"GPU-0","mem_mb":4096,"streams":2,"ttl_s":120,"priority":1},"budget":{"ttl_s":180,"ops":500}})),
            ("spawn lora", serde_json::json!({"spawn":"lora"})),
        ];
        for (line, expected) in cases {
            assert!(page.lines().any(|example| example.trim() == line));
            let mut tokens = line.split_whitespace().skip(1);
            let role = tokens.next().unwrap();
            let payload = crate::build_spawn_payload(role, tokens).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
                expected
            );
        }
    }
}
