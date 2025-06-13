// CLASSIFICATION: COMMUNITY
// Filename: ensemble_agent.rs v0.2
// Date Modified: 2025-07-22
// Author: Cohesix Codex

use cohesix::agents::ensemble::{Arbitration, DecisionAgent, EnsembleAgent, SharedMemory};
use serde_json::Value;
use std::fs;
use std::io::Write;

struct FixedAgent { action: &'static str, score: f32 }
impl DecisionAgent for FixedAgent {
    fn propose(&mut self, _m: &SharedMemory) -> (String, f32) { (self.action.into(), self.score) }
}

#[test]
fn ensemble_weighted_selects_best() {
    // Load ensemble configuration for test
    let cfg_data = std::fs::read_to_string("tests/data/ensemble_config.json")
        .expect("Missing ensemble_config.json for test input");
    let cfg: Value = serde_json::from_str(&cfg_data).expect("invalid json");
    println!("{:?}", cfg);

    // Pre-create goals log expected by the agent
    fs::create_dir_all("/ensemble/e1").expect("failed to create ensemble dir");
    let mut f = fs::File::create("/ensemble/e1/goals.json")
        .expect("failed to create goals log");
    writeln!(f, "{{\"goal\": \"balance\", \"score\": 1.0}}")
        .expect("failed to write mock goals");

    let mut ens = EnsembleAgent::new("e1", Arbitration::Weighted);
    ens.add_agent(Box::new(FixedAgent { action: "A", score: 0.2 }));
    ens.add_agent(Box::new(FixedAgent { action: "B", score: 0.8 }));
    let act = ens.tick();
    assert_eq!(act, "B");
    let data = std::fs::read_to_string("/ensemble/e1/goals.json")
        .expect("missing goals log");
    assert!(data.contains("B"));

    // Clean up mock files
    let _ = fs::remove_file("/ensemble/e1/goals.json");
    let _ = fs::remove_dir_all("/ensemble/e1");
}
