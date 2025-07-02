// CLASSIFICATION: COMMUNITY
// Filename: compiler.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-25

use serde::Deserialize;
use cohesix::CohError;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
#[allow(dead_code)]
struct Scenario {
    agents: Vec<Agent>,
    physics: Option<Physics>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Agent {
    id: String,
    role: String,
    start: u64,
    actions: Vec<Action>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Action {
    tick: u64,
    cmd: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Physics {
    objects: Vec<PhysObj>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PhysObj {
    id: String,
    mass: f32,
    pos: [f32; 2],
}

pub struct ScenarioCompiler;

impl ScenarioCompiler {
    pub fn compile(input: &Path, out_dir: &Path) -> Result<(), CohError> {
        let data = fs::read_to_string(input)?;
        let scenario: Scenario = if input.extension().and_then(|e| e.to_str()) == Some("toml") {
            toml::from_str(&data)?
        } else {
            serde_json::from_str(&data)?
        };
        fs::create_dir_all(out_dir)?;
        Self::write_trace(out_dir, &scenario)?;
        Self::write_plan9(out_dir)?;
        Self::write_rc(out_dir)?;
        Ok(())
    }

    fn write_trace(out_dir: &Path, scenario: &Scenario) -> Result<(), CohError> {
        let mut trace = Vec::new();
        for agent in &scenario.agents {
            trace.push(format!("spawn {}", agent.id));
            for act in &agent.actions {
                trace.push(format!("{} {}", act.cmd, act.tick));
            }
        }
        let path = out_dir.join("trace.trc");
        fs::write(&path, serde_json::to_string_pretty(&trace)?)?;
        Ok(())
    }

    fn write_plan9(out_dir: &Path) -> Result<(), CohError> {
        let cfg = "srv * /srv\n";
        fs::write(out_dir.join("plan9.cfg"), cfg)?;
        Ok(())
    }

    fn write_rc(out_dir: &Path) -> Result<(), CohError> {
        let rc = "echo start";
        fs::write(out_dir.join("rc.local"), rc)?;
        Ok(())
    }
}
