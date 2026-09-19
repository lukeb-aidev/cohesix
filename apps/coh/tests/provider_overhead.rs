// Author: Lukas Bower
// Purpose: Measure selected host provider and exporter overhead using explicitly non-live signed fixtures.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

#[path = "../../../crates/cohesix-evidence/tests/support/mod.rs"]
mod support;

use cohesix_authority::provider;
use cohesix_identity::{Policy, Rule, Scope};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

fn measure(name: &str, mut operation: impl FnMut()) -> Value {
    for _ in 0..10 {
        operation();
    }
    let mut samples = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed().as_nanos());
    }
    let mut sorted = samples.clone();
    sorted.sort_unstable();
    json!({"name":name,"samples_ns":samples,"sample_count":100,"warmup_count":10,
        "p50_ns":sorted[49],"p95_ns":sorted[94],"maximum_ns":sorted[99]})
}

#[test]
#[ignore = "explicit provider-conformance --perf-only microbenchmark"]
fn bounded_provider_exporter_overhead() {
    let (graph, trust) = support::fixture();
    let graph_bytes = serde_json::to_vec(&graph).expect("fixture graph");
    let accepted =
        cohesix_evidence::verify(&graph_bytes, &trust, |_| Ok(())).expect("fixture verified");
    let registry = provider::registry().expect("compiled registry");
    let graph_hash = registry["graph_sha256"]
        .as_str()
        .expect("generated graph hash");
    let actions = BTreeSet::from(["systemd.status-check".to_owned()]);
    let policy = Policy {
        id: "local-performance-fixture".into(),
        kind: "local".into(),
        enabled: true,
        issuer: "local:performance".into(),
        audience: "host-contract-benchmark".into(),
        algorithm: "os-euid".into(),
        keyset_sha256: vec![],
        maximum_ttl_s: 60,
        rules: vec![Rule {
            subject: rustix::process::geteuid().as_raw().to_string(),
            required_groups: vec![],
            normalized_subject: "performance-fixture".into(),
            role: "queen".into(),
            scopes: vec![Scope {
                path: "/host/tickets/spec".into(),
                verb: "write".into(),
            }],
            provider_actions: vec!["systemd.status-check".into()],
            maximum_operations: 1,
        }],
    };
    let mut rows = vec![
        measure("registry_lookup", || {
            black_box(provider::action(black_box("systemd.status-check"))).expect("known action");
        }),
        measure("provider_validation", || {
            provider::validate_target(
                black_box("systemd.status-check"),
                Some(black_box("ssh.service")),
            )
            .expect("unit grammar");
        }),
        measure("provider_refusal", || {
            assert_eq!(
                provider::validate_target(
                    black_box("systemd.status-check"),
                    Some(black_box("../../etc/shadow"))
                ),
                Err(provider::ProviderError::InvalidTarget)
            );
        }),
        measure("local_identity_mapping", || {
            black_box(cohesix_identity::map_local(
                &policy, 1000, graph_hash, &actions,
            ))
            .expect("local subject fixture");
        }),
        measure("signed_graph_verification", || {
            black_box(cohesix_evidence::verify(
                black_box(&graph_bytes),
                &trust,
                |_| Ok(()),
            ))
            .expect("fixed signed graph");
        }),
        measure("receipt_rendering", || {
            black_box(serde_json::to_vec(&accepted)).expect("verified projection");
        }),
    ];
    let mut sizes = serde_json::Map::new();
    for format in ["prometheus", "otel", "cloudevents", "in_toto", "siem"] {
        let output = coh::export::render(&accepted, format).expect("derived projection");
        assert!(output.len() <= 65_536);
        sizes.insert(format.into(), json!(output.len()));
        rows.push(measure(&format!("export_{format}"), || {
            black_box(coh::export::render(&accepted, black_box(format)))
                .expect("derived projection");
        }));
    }
    println!(
        "COHESIX_PROVIDER_PERF_JSON={}",
        json!({
            "schema":"cohesix-provider-overhead/v1","claiming":false,"production_proven":false,
            "mode":"host-contract-fixture","target":"host","profile":"release",
            "architecture":std::env::consts::ARCH,"os":std::env::consts::OS,
            "provider_graph_sha256":graph_hash,"fixture_graph_sha256":cohesix_evidence::digest(&graph_bytes),
            "graph_bytes":graph_bytes.len(),"graph_nodes":graph.records.len(),
            "registry_bytes":provider::registry_json().len(),"export_bytes":sizes,"measurements":rows,
            "proof_limits":["No native operation, TLS delivery, device or Worker execution.",
                "In-memory fixture CAS; file I/O and network latency excluded.",
                "Warm host process; no Pi/QEMU throughput or latency equivalence claim."]
        })
    );
}
