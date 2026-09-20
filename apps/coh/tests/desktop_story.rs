// Author: Lukas Bower
// Purpose: Verify historical desktop journeys with canonical trust, causal failure and tamper refusal.
// Copyright 2026 Lukas Bower
use coh::operator;
use serde_json::json;
use std::path::Path;

#[test]
fn historical_journeys_keep_exact_outcomes_and_artifact_links() {
    let reference = Path::new(env!("CARGO_MANIFEST_DIR")).join("../swarmui/reference");
    for (name, outcome, ticket) in [
        ("cuda", "succeeded", "m27c-systemd-vadd"),
        ("lora", "succeeded", "m27d-import-03"),
        ("recovery", "failed", "m27d-canary-02"),
    ] {
        let root = reference.join(name);
        let story = operator::story(
            &root.join("graph.json"),
            &root.join("trust.json"),
            &root.join("cas"),
        )
        .unwrap();
        assert_eq!(story["graph"]["outcome"], outcome);
        assert_eq!(story["graph"]["binding"]["ticket_id"], ticket);
        assert_eq!(story["graph"]["records"].as_array().unwrap().len(), 9);
        assert_eq!(story["artifacts"].as_object().unwrap().len(), 6);
        if name == "recovery" {
            assert!(story["artifacts"]
                .as_object()
                .unwrap()
                .values()
                .any(|a| a["value"]["observation"]["state"] == "recovered_failure"));
        }
    }
}
#[test]
fn desktop_story_refuses_tampered_causal_record() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../swarmui/reference/lora");
    let tmp = tempfile::tempdir().unwrap();
    let mut graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("graph.json")).unwrap()).unwrap();
    graph["records"][0]["record"]["outcome"] = json!("succeeded");
    let input = tmp.path().join("graph.json");
    std::fs::write(&input, serde_json::to_vec(&graph).unwrap()).unwrap();
    assert!(operator::story(&input, &root.join("trust.json"), &root.join("cas")).is_err());
}
#[test]
fn display_redaction_keeps_identity_but_excludes_raw_model_content() {
    let mut value = json!({"operation_id":"run-7","nested":[{"prompt":"private","tool_calls":[{"command":"private"}],"model_output":"private","artifact_sha256":"abcd"}]});
    operator::redact_display_content(&mut value);
    assert_eq!(
        value,
        json!({"operation_id":"run-7","nested":[{"prompt":"<content omitted>","tool_calls":"<content omitted>","model_output":"<content omitted>","artifact_sha256":"abcd"}]})
    );
}
