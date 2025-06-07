// CLASSIFICATION: COMMUNITY
// Filename: migration.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-03

//! Agent state serialization and migration support.
//!
//! This is a best-effort implementation that snapshots basic runtime state so a
//! worker can restore an agent on a different node. Complex namespaces and
//! process state are outside the scope of this helper.

use std::collections::HashMap;
use std::fs;

use crate::runtime::ServiceRegistry;

/// Serialized representation of an agent.
#[derive(Clone, Debug)]
pub struct AgentState {
    pub env: HashMap<String, String>,
    pub trace: String,
    pub mounts: Vec<(String, String)>,
}

/// Serialize agent state from the local worker.
pub fn serialize(agent_id: &str) -> anyhow::Result<AgentState> {
    let mut env = HashMap::new();
    for (k, v) in std::env::vars() {
        env.insert(k, v);
    }
    let trace = fs::read_to_string(format!("/srv/agent_trace/{agent_id}")).unwrap_or_default();
    let mounts = fs::read_dir("/srv")?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            Some((name.clone(), e.path().to_string_lossy().into()))
        })
        .collect();
    Ok(AgentState { env, trace, mounts })
}

/// Restore a serialized agent on the current worker.
pub fn restore(agent_id: &str, state: &AgentState) -> anyhow::Result<()> {
    fs::create_dir_all(format!("/srv/agents/{agent_id}")).ok();
    fs::write(format!("/srv/agent_trace/{agent_id}"), &state.trace).ok();
    ServiceRegistry::unregister_service(agent_id);
    ServiceRegistry::register_service(agent_id, &format!("/srv/agents/{agent_id}"));
    for (k, v) in &state.env {
        std::env::set_var(k, v);
    }
    Ok(())
}

/// Migrate an agent between workers using the provided copy functions.
pub fn migrate(
    agent_id: &str,
    fetch: impl Fn(&str) -> anyhow::Result<AgentState>,
    push: impl Fn(&str, &AgentState) -> anyhow::Result<()>,
    stop: impl Fn(&str) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let state = fetch(agent_id)?;
    push(agent_id, &state)?;
    stop(agent_id)?;
    fs::remove_dir_all(format!("/srv/agents/{agent_id}")).ok();
    ServiceRegistry::unregister_service(agent_id);
    Ok(())
}

