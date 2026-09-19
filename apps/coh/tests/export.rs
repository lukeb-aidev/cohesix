// Author: Lukas Bower
// Purpose: Preserve derived exporter wire shapes and forbid receipt or build-provenance substitution.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

#[path = "../../../crates/cohesix-evidence/tests/support/mod.rs"]
mod support;

#[test]
fn derived_formats_keep_graph_identity_and_never_import_as_authority() {
    let (graph, trust) = support::fixture();
    let bytes = serde_json::to_vec(&graph).unwrap();
    let accepted = cohesix_evidence::verify(&bytes, &trust, |_| Ok(())).unwrap();
    for format in ["otel", "cloudevents", "in_toto", "siem"] {
        let exported = coh::export::render(&accepted, format).unwrap();
        assert!(exported.len() < 65_536);
        assert!(!String::from_utf8_lossy(&exported).contains("unit-invocation-unique"));
        let value: serde_json::Value = serde_json::from_slice(&exported).unwrap();
        match format {
            "otel" => {
                let span = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
                assert_eq!(span["traceId"].as_str().unwrap().len(), 32);
                assert_eq!(span["spanId"].as_str().unwrap().len(), 16);
                assert_eq!(span["startTimeUnixNano"], "1000000000");
                assert_eq!(span["attributes"][0]["value"]["boolValue"], false);
            }
            "cloudevents" => {
                assert_eq!(value["specversion"], "1.0");
                assert_eq!(value["id"], accepted.digest());
                assert_eq!(value["data"]["authoritative"], false);
            }
            "in_toto" => {
                assert_eq!(value["_type"], "https://in-toto.io/Statement/v1");
                assert_eq!(value["predicate"]["authoritative"], false);
            }
            "siem" => assert_eq!(value["authoritative"], false),
            _ => unreachable!(),
        }
        assert!(cohesix_evidence::verify(&exported, &trust, |_| Ok(())).is_err());
    }
    let metrics = String::from_utf8(coh::export::render(&accepted, "prometheus").unwrap()).unwrap();
    assert!(metrics.contains(
        "cohesix_evidence_terminal_info{action=\"systemd.restart\",outcome=\"succeeded\"} 1"
    ));
    assert!(!metrics.contains("request-1"));
    assert!(coh::export::render(&accepted, "slsa")
        .unwrap_err()
        .to_string()
        .contains("registered build provenance"));
    assert!(coh::export::render(&accepted, "raw").is_err());
}
