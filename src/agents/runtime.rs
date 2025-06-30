// CLASSIFICATION: COMMUNITY
// Filename: runtime.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-12-30

//! Agent runtime management.
//!
//! Spawns, traces and terminates agents in sandboxed namespaces. Each agent is
//! registered under `/srv/agents/<id>` and a trace log is kept in
//! `/srv/agent_trace/<id>`.

use anyhow::Context;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::directory::{AgentDirectory, AgentRecord};
use crate::cohesix_types::Role;
use crate::runtime::ServiceRegistry;
use crate::trace::recorder;

/// Runtime responsible for managing spawned agents.
pub struct AgentRuntime {
    pub procs: HashMap<String, Child>,
}

use crate::agent_migration::{Migrateable, MigrationStatus};
use crate::agent_transport::AgentTransport;

fn agents_dir() -> String {
    std::env::var("COHESIX_AGENTS_DIR").unwrap_or_else(|_| "/srv/agents".into())
}

fn agent_trace_dir() -> String {
    std::env::var("COHESIX_AGENT_TRACE_DIR").unwrap_or_else(|_| "/srv/agent_trace".into())
}

impl Migrateable for AgentRuntime {
    fn migrate<T: AgentTransport>(
        &self,
        peer: &str,
        transport: &T,
    ) -> anyhow::Result<MigrationStatus> {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/srv".to_string());
        let path = format!("{}/runtime_state.json", tmpdir);
        std::fs::write(&path, "runtime")?;
        transport.send_state("runtime", peer, &path)?;
        Ok(MigrationStatus::Completed)
    }
}

impl AgentRuntime {
    /// Create a new agent runtime manager.
    pub fn new() -> Self {
        Self {
            procs: HashMap::new(),
        }
    }

    /// Spawn a new agent process with the given role and arguments.
    pub fn spawn(&mut self, agent_id: &str, role: Role, args: &[String]) -> anyhow::Result<()> {
        if let Role::Other(_) = role {
            return Err(anyhow::anyhow!("invalid role"));
        }
        let agents_dir = agents_dir();
        fs::create_dir_all(&agents_dir)
            .with_context(|| format!("failed to create agents dir {agents_dir}"))?;
        let path = format!("{}/{}", agents_dir, agent_id);
        fs::create_dir_all(&path).with_context(|| format!("failed to create agent path {path}"))?;
        ServiceRegistry::register_service(agent_id, &path).context("service registry failed")?;

        let trace_dir = agent_trace_dir();
        fs::create_dir_all(&trace_dir)
            .with_context(|| format!("failed to create trace dir {trace_dir}"))?;
        let trace_path = format!("{}/{}", trace_dir, agent_id);
        let mut trace = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&trace_path)
            .with_context(|| format!("open trace file {trace_path}"))?;
        writeln!(trace, "spawn {} {:?}", timestamp(), args)?;
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let _ = recorder::spawn(agent_id, &args[0], &arg_refs);

        let mut cmd = Command::new(&args[0]);
        if args.len() > 1 {
            cmd.args(&args[1..]);
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn agent {} command {}", agent_id, args[0]))?;
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

    /// Pause a running agent process.
    pub fn pause(&mut self, agent_id: &str) -> anyhow::Result<()> {
        if let Some(child) = self.procs.get_mut(agent_id) {
            // Removed nix::sys::signal dependency for UEFI
            let _ = child.kill();
        }
        Ok(())
    }

    /// Terminate an existing agent and remove its record.
    pub fn terminate(&mut self, agent_id: &str) -> anyhow::Result<()> {
        if let Some(mut child) = self.procs.remove(agent_id) {
            let _ = child.kill();
            let _ = child.wait();
            let trace_dir = agent_trace_dir();
            let mut trace = OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("{}/{}", trace_dir, agent_id))?;
            writeln!(trace, "terminate {}", timestamp())?;
            recorder::event(agent_id, "terminate", "");
        }
        let agents_dir = agents_dir();
        std::fs::remove_dir_all(format!("{}/{}", agents_dir, agent_id)).ok();
        AgentDirectory::remove(agent_id);
        Ok(())
    }

    /// Return the trace file path for an agent.
    pub fn trace(&self, agent_id: &str) -> PathBuf {
        PathBuf::from(format!("{}/{}", agent_trace_dir(), agent_id))
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}
