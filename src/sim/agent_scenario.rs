// CLASSIFICATION: COMMUNITY
// Filename: agent_scenario.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-09-22

use crate::prelude::*;
use crate::{coh_error, CohError};
/// Scenario engine for automated agent tests.
//
/// Reads a scenario configuration from `/boot/scenario.json`, spawns agents
/// using `AgentRuntime`, applies mock environmental inputs and writes a score
/// file under `/srv/scenario_result/<id>`.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::agents::runtime::AgentRuntime;
use crate::cohesix_types::Role;
use crate::physical::sensors;
#[cfg(feature = "joystick")]
use crate::physical::joystick;
use crate::trace::recorder;

#[derive(Deserialize)]
struct ScenarioConfig {
    id: String,
    agents: Vec<AgentSpec>,
}

#[derive(Deserialize)]
struct AgentSpec {
    id: String,
    role: String,
    cmd: String,
}

pub struct ScenarioEngine;

impl ScenarioEngine {
    pub fn run(path: &Path) -> Result<(), CohError> {
        let iso = if Path::new("out/cohesix_grub.iso").exists() {
            Path::new("out/cohesix_grub.iso")
        } else {
            Path::new("out/cohesix.iso")
        };
        if !iso.exists() {
            return Err(coh_error!(
                "boot ISO not found; expected {} or out/cohesix.iso",
                iso.display()
            ));
        }
        let data = fs::read_to_string(path)?;
        let cfg: ScenarioConfig = serde_json::from_str(&data)?;
        recorder::event("scenario", "start", &cfg.id);
        fs::create_dir_all("/srv/scenario_result").ok();
        let mut runtime = AgentRuntime::new();
        for agent in &cfg.agents {
            let args = vec![agent.cmd.clone()];
            let role = match agent.role.as_str() {
                "QueenPrimary" => Role::QueenPrimary,
                "DroneWorker" => Role::DroneWorker,
                "KioskInteractive" => Role::KioskInteractive,
                "GlassesAgent" => Role::GlassesAgent,
                "SensorRelay" => Role::SensorRelay,
                "SimulatorTest" => Role::SimulatorTest,
                _ => Role::Other(agent.role.clone()),
            };
            runtime.spawn(&agent.id, role, &args)?;
            let _ = sensors::read_temperature(&agent.id);
            #[cfg(feature = "joystick")]
            let _ = joystick::read_state(&agent.id);
        }
        fs::write(
            format!("/srv/scenario_result/{}", cfg.id),
            "ok",
        )?;
        recorder::event("scenario", "end", &cfg.id);
        Ok(())
    }
}
