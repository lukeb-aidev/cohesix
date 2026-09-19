// Author: Lukas Bower
// Purpose: Require complete staged workflows, explicit node ownership and separately admitted recovery in the integration graph.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub schema: String,
    pub topology: Vec<Node>,
    pub execute_actions: Vec<String>,
    pub recovery_actions: Vec<String>,
    pub external_dependencies: Vec<Dependency>,
    pub stages: Vec<Stage>,
    pub worker_tier: String,
    pub evidence_schema: String,
    pub governance_mode: String,
    pub authority_custodian: String,
    pub bypass_posture: String,
    pub package_profile: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub owner: String,
    pub host_class: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub id: String,
    pub owner: String,
    pub kind: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub id: String,
    pub after: Vec<String>,
    pub owner: String,
    pub postcondition: String,
    pub when: String,
}

impl Workflow {
    pub fn validate(&self, actions: &BTreeSet<String>, packages: &BTreeSet<String>) -> Result<()> {
        ensure!(
            self.schema == "cohesix-workflow/v1"
                && self.evidence_schema == "cohesix-causal-evidence/v1"
                && self.governance_mode == "observe"
                && self.authority_custodian == "root-admitted-ticket"
                && !self.bypass_posture.is_empty()
                && self.bypass_posture.len() <= 512
                && matches!(
                    self.worker_tier.as_str(),
                    "none" | "executable" | "receipt_only"
                )
                && packages.contains(&self.package_profile),
            "invalid workflow authority/package"
        );
        ensure!(
            (3..=8).contains(&self.topology.len()),
            "workflow topology bound"
        );
        let mut nodes = BTreeSet::new();
        for node in &self.topology {
            ensure!(
                cohesix_authority::validate_id(&node.id).is_ok()
                    && nodes.insert(node.id.as_str())
                    && !node.owner.is_empty()
                    && node.owner.len() <= 128
                    && matches!(
                        node.host_class.as_str(),
                        "macos" | "linux" | "jetson" | "sel4" | "remote-cuda" | "fleet"
                    ),
                "invalid workflow node"
            );
        }
        ensure!(
            ["controller", "target-hive", "provider-host"]
                .iter()
                .all(|n| nodes.contains(n)),
            "missing workflow topology owner"
        );
        for list in [&self.execute_actions, &self.recovery_actions] {
            ensure!(
                !list.is_empty() && list.len() <= 16 && list.iter().all(|a| actions.contains(a)),
                "workflow unregistered action"
            );
        }
        let mut dependencies = BTreeSet::new();
        ensure!(
            self.external_dependencies.len() <= 16,
            "workflow dependency bound"
        );
        for d in &self.external_dependencies {
            ensure!(
                cohesix_authority::validate_id(&d.id).is_ok()
                    && dependencies.insert(&d.id)
                    && !d.owner.is_empty()
                    && d.owner.len() <= 128
                    && matches!(d.kind.as_str(), "signed_application" | "accepted_milestone"),
                "workflow external dependency"
            );
        }
        let stages = [
            "preflight",
            "admit",
            "execute",
            "observe",
            "verify",
            "recover",
        ];
        ensure!(self.stages.len() == stages.len(), "workflow missing stage");
        for (index, stage) in self.stages.iter().enumerate() {
            ensure!(
                stage.id == stages[index]
                    && nodes.contains(stage.owner.as_str())
                    && !stage.postcondition.is_empty()
                    && stage.postcondition.len() <= 512
                    && stage.when
                        == if index == 5 {
                            "explicit_recovery"
                        } else {
                            "normal"
                        }
                    && stage.after
                        == if index == 0 {
                            vec![]
                        } else {
                            vec![stages[index - 1].to_owned()]
                        },
                "invalid workflow stage order/owner"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_workflows_require_every_stage_owner_and_registered_action() {
        let source: toml::Value = toml::from_str(include_str!(
            "../../../configs/host_integration_acceptance.toml"
        ))
        .unwrap();
        let actions = source["providers"]["families"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.get("actions").and_then(toml::Value::as_array))
            .flatten()
            .map(|a| a["id"].as_str().unwrap().to_owned())
            .collect();
        let packages = source["providers"]["deployment_profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect();
        let rows = source["playbooks"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let flow: Workflow = row["workflow"].clone().try_into().unwrap();
            flow.validate(&actions, &packages).unwrap();
            let mut missing = flow.clone();
            missing.stages.remove(3);
            assert!(missing.validate(&actions, &packages).is_err());
            let mut bypass = flow.clone();
            bypass.stages[5].when = "normal".into();
            assert!(bypass.validate(&actions, &packages).is_err());
            let mut unknown = flow.clone();
            unknown.execute_actions[0] = "shell.exec".into();
            assert!(unknown.validate(&actions, &packages).is_err());
            let mut orphan = flow;
            orphan.topology[2].id = "unowned".into();
            assert!(orphan.validate(&actions, &packages).is_err());
        }
    }
}
