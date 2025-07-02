// CLASSIFICATION: COMMUNITY
// Filename: scenario_runner.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-25

use crate::prelude::*;
/// Scenario runner executing compiled scenarios tick-by-tick.

use crate::agents::runtime::AgentRuntime;
use crate::trace::recorder;
use rapier3d::prelude::*;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
struct CompiledScenario {
    id: String,
    trace: Vec<String>,
}

pub struct ScenarioRunner;

impl ScenarioRunner {
    pub fn run_all(base: &Path) -> Result<()> {
        if !base.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(base)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("scenario") {
                Self::run(&path)?;
            }
        }
        Ok(())
    }

    pub fn run(path: &Path) -> Result<()> {
        let data = fs::read_to_string(path)?;
        let scn: CompiledScenario = serde_json::from_str(&data)?;
        let mut runtime = AgentRuntime::new();
        let mut physics = RigidBodySet::new();
        for line in scn.trace {
            recorder::event(&scn.id, "step", &line);
            if line.starts_with("spawn") {
                let parts: Vec<_> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let id = parts[1];
                    runtime.spawn(id, crate::cohesix_types::Role::SimulatorTest, &["/bin/true".into()])?;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::create_dir_all("/srv/scenario_result").ok();
        fs::write(format!("/srv/scenario_result/{}", scn.id), "done")?;
        Ok(())
    }
}
