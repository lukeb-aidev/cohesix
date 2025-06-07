// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-04

//! Agent runtime management.
//!
//! Spawns, traces and terminates agents in sandboxed namespaces. Each agent is
//! registered under `/srv/agents/<id>` and a trace log is kept in
//! `/srv/agent_trace/<id>`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::ServiceRegistry;
use crate::cohesix_types::Role;
use crate::trace::recorder;
use crate::agent::directory::{AgentDirectory, AgentRecord};

/// Runtime responsible for managing spawned agents.
pub struct AgentRuntime {
    procs: HashMap<String, Child>,
}

impl AgentRuntime {
    /// Create a new agent runtime manager.
    pub fn new() -> Self {
        Self { procs: HashMap::new() }
    }

    /// Spawn a new agent process with the given role and arguments.
    pub fn spawn(&mut self, agent_id: &str, role: Role, args: &[String]) -> anyhow::Result<()> {
        match role {
            Role::Other(_) => return Err(anyhow::anyhow!("invalid role")),
            _ => {}
        }
        fs::create_dir_all("/srv/agents")?;
        let path = format!("/srv/agents/{agent_id}");
        fs::create_dir_all(&path)?;
        ServiceRegistry::register_service(agent_id, &path);

        fs::create_dir_all("/srv/agent_trace")?;
        let mut trace = OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("/srv/agent_trace/{agent_id}"))?;
        writeln!(trace, "spawn {} {:?}", timestamp(), args)?;
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let _ = recorder::spawn(agent_id, &args[0], &arg_refs);

        let mut cmd = Command::new(&args[0]);
        if args.len() > 1 {
            cmd.args(&args[1..]);
        }
        let child = cmd.spawn()?;
        self.procs.insert(agent_id.to_string(), child);
        AgentDirectory::update(AgentRecord {
            id: agent_id.into(),
            location: path,
            role: format!("{:?}", role),
            status: "running".into(),
            last_heartbeat: timestamp(),
        });
        Ok(())
    }

    /// Terminate an existing agent and remove its record.
    pub fn terminate(&mut self, agent_id: &str) -> anyhow::Result<()> {
        if let Some(mut child) = self.procs.remove(agent_id) {
            let _ = child.kill();
            let _ = child.wait();
            let mut trace = OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("/srv/agent_trace/{agent_id}"))?;
            writeln!(trace, "terminate {}", timestamp())?;
            recorder::event(agent_id, "terminate", "");
        }
        std::fs::remove_dir_all(format!("/srv/agents/{agent_id}")).ok();
        AgentDirectory::remove(agent_id);
        Ok(())
    }

    /// Return the trace file path for an agent.
    pub fn trace(&self, agent_id: &str) -> PathBuf {
        PathBuf::from(format!("/srv/agent_trace/{agent_id}"))
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
