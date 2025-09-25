// CLASSIFICATION: COMMUNITY
// Filename: reconcile.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

use crate::config::{NamespaceEntry, Secure9pConfig};
use cohesix_9p::policy::SandboxPolicy;
use log::warn;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationDomain {
    Namespace,
    Policy,
}

impl ReconciliationDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReconciliationDomain::Namespace => "namespace",
            ReconciliationDomain::Policy => "policy",
        }
    }
}

impl fmt::Display for ReconciliationDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ReconciliationEvent {
    pub trace_id: String,
    pub domain: ReconciliationDomain,
    pub agent: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedNamespace {
    pub agent: String,
    pub root: std::path::PathBuf,
    pub read_only: bool,
}

#[derive(Clone)]
pub struct ResolvedPolicy {
    pub agent: String,
    pub policy: SandboxPolicy,
}

impl fmt::Debug for ResolvedPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedPolicy")
            .field("agent", &self.agent)
            .field("read_rules", &self.policy.read.len())
            .field("write_rules", &self.policy.write.len())
            .finish()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ReconciliationOutcome {
    pub namespaces: Vec<ResolvedNamespace>,
    pub policies: Vec<ResolvedPolicy>,
    pub events: Vec<ReconciliationEvent>,
}

pub struct PolicyReconciler<'a> {
    cfg: &'a Secure9pConfig,
    trace: TraceIdGenerator,
}

impl<'a> PolicyReconciler<'a> {
    pub fn new(cfg: &'a Secure9pConfig) -> Self {
        Self {
            cfg,
            trace: TraceIdGenerator::new("secure9p"),
        }
    }

    pub fn reconcile(mut self) -> ReconciliationOutcome {
        let mut outcome = ReconciliationOutcome::default();
        let mut namespace_map: BTreeMap<String, NamespaceEntry> = BTreeMap::new();
        for (idx, entry) in self.cfg.namespace.iter().enumerate() {
            let trace_id =
                self.trace
                    .next(ReconciliationDomain::Namespace, &entry.agent, idx as u64);
            match namespace_map.get(&entry.agent) {
                None => {
                    outcome.events.push(ReconciliationEvent {
                        trace_id,
                        domain: ReconciliationDomain::Namespace,
                        agent: entry.agent.clone(),
                        message: format!(
                            "applied namespace root={} read_only={}",
                            entry.root.display(),
                            entry.read_only
                        ),
                    });
                }
                Some(previous) => {
                    if previous.root != entry.root || previous.read_only != entry.read_only {
                        outcome.events.push(ReconciliationEvent {
                            trace_id,
                            domain: ReconciliationDomain::Namespace,
                            agent: entry.agent.clone(),
                            message: format!(
                                "conflict resolved; replaced {} (read_only={}) with {} (read_only={})",
                                previous.root.display(),
                                previous.read_only,
                                entry.root.display(),
                                entry.read_only
                            ),
                        });
                    } else {
                        outcome.events.push(ReconciliationEvent {
                            trace_id,
                            domain: ReconciliationDomain::Namespace,
                            agent: entry.agent.clone(),
                            message: format!(
                                "duplicate namespace entry ignored; retaining {}",
                                entry.root.display()
                            ),
                        });
                    }
                }
            }
            namespace_map.insert(entry.agent.clone(), entry.clone());
        }

        let mut policy_map: BTreeMap<String, SandboxPolicy> = BTreeMap::new();
        for (idx, entry) in self.cfg.policy.iter().enumerate() {
            let policy = entry.to_policy();
            if policy.read.is_empty() && policy.write.is_empty() {
                let trace_id =
                    self.trace
                        .next(ReconciliationDomain::Policy, &entry.agent, idx as u64);
                outcome.events.push(ReconciliationEvent {
                    trace_id,
                    domain: ReconciliationDomain::Policy,
                    agent: entry.agent.clone(),
                    message: "skipped empty policy rules".to_string(),
                });
                warn!(
                    "secure9p reconciliation skipped empty policy for agent {}",
                    entry.agent
                );
                continue;
            }

            let trace_id = self
                .trace
                .next(ReconciliationDomain::Policy, &entry.agent, idx as u64);
            match policy_map.get(&entry.agent) {
                None => outcome.events.push(ReconciliationEvent {
                    trace_id,
                    domain: ReconciliationDomain::Policy,
                    agent: entry.agent.clone(),
                    message: format!(
                        "applied policy with {} read and {} write rules",
                        policy.read.len(),
                        policy.write.len()
                    ),
                }),
                Some(previous) => {
                    let detail = format!(
                        "conflict resolved; replaced policy (read={}, write={}) with (read={}, write={})",
                        previous.read.len(),
                        previous.write.len(),
                        policy.read.len(),
                        policy.write.len()
                    );
                    outcome.events.push(ReconciliationEvent {
                        trace_id,
                        domain: ReconciliationDomain::Policy,
                        agent: entry.agent.clone(),
                        message: detail,
                    });
                }
            }
            policy_map.insert(entry.agent.clone(), policy);
        }

        outcome.namespaces = namespace_map
            .into_iter()
            .map(|(agent, entry)| ResolvedNamespace {
                agent,
                root: entry.root,
                read_only: entry.read_only,
            })
            .collect();

        outcome.policies = policy_map
            .into_iter()
            .map(|(agent, policy)| ResolvedPolicy { agent, policy })
            .collect();

        outcome
    }
}

struct TraceIdGenerator {
    prefix: &'static str,
    counter: u64,
}

impl TraceIdGenerator {
    fn new(prefix: &'static str) -> Self {
        Self { prefix, counter: 1 }
    }

    fn next(&mut self, domain: ReconciliationDomain, agent: &str, position: u64) -> String {
        let id = format!(
            "{}:{}:{}:{:08x}:{:08x}",
            self.prefix,
            domain.as_str(),
            agent,
            position,
            self.counter
        );
        self.counter = self.counter.wrapping_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PolicyEntry;
    use std::path::PathBuf;

    fn base_config() -> Secure9pConfig {
        Secure9pConfig {
            namespace: Vec::new(),
            policy: Vec::new(),
            port: 9443,
            cert: PathBuf::from("cert.pem"),
            key: PathBuf::from("key.pem"),
            ca_cert: None,
            require_client_auth: false,
        }
    }

    #[test]
    fn resolves_namespace_conflicts_deterministically() {
        let mut cfg = base_config();
        cfg.namespace = vec![
            NamespaceEntry {
                agent: "alpha".into(),
                root: PathBuf::from("/srv/a"),
                read_only: false,
            },
            NamespaceEntry {
                agent: "alpha".into(),
                root: PathBuf::from("/srv/override"),
                read_only: true,
            },
            NamespaceEntry {
                agent: "beta".into(),
                root: PathBuf::from("/srv/b"),
                read_only: false,
            },
        ];

        let outcome = PolicyReconciler::new(&cfg).reconcile();
        assert_eq!(outcome.namespaces.len(), 2);
        assert_eq!(outcome.namespaces[0].agent, "alpha");
        assert_eq!(outcome.namespaces[0].root, PathBuf::from("/srv/override"));
        assert!(outcome
            .events
            .iter()
            .any(|event| event.message.contains("conflict resolved")));
    }

    #[test]
    fn resolves_policy_conflicts_with_latest_entry() {
        let mut cfg = base_config();
        cfg.policy = vec![
            PolicyEntry {
                agent: "alpha".into(),
                allow: vec!["read:/data".into()],
            },
            PolicyEntry {
                agent: "alpha".into(),
                allow: vec!["write:/logs".into()],
            },
        ];

        let outcome = PolicyReconciler::new(&cfg).reconcile();
        assert_eq!(outcome.policies.len(), 1);
        assert_eq!(outcome.policies[0].agent, "alpha");
        assert_eq!(outcome.policies[0].policy.read.len(), 0);
        assert_eq!(outcome.policies[0].policy.write.len(), 1);
        assert!(outcome
            .events
            .iter()
            .any(|event| event.message.contains("conflict resolved")));
    }
}
