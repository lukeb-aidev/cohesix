// Author: Lukas Bower
// Purpose: Refuse modified packaged reference evidence without silently replacing it.
// Copyright 2026 Lukas Bower
#[test]
fn reference_materialization_is_deterministic_and_tamper_refusing() {
    let data = tempfile::tempdir().unwrap();
    let root = swarmui::reference::materialize(data.path()).unwrap();
    assert_eq!(swarmui::reference::materialize(data.path()).unwrap(), root);
    let graph = root.join("lora/graph.json");
    assert_eq!(
        swarmui::workbench::digest(&std::fs::read(&graph).unwrap()),
        "8daf8cc168e84e59013f5cbccc79e8894508799eda10aad663252601f5b99a82"
    );
    std::fs::write(&graph, b"{}").unwrap();
    assert!(swarmui::reference::materialize(data.path())
        .unwrap_err()
        .starts_with("reference_tampered:"));
    assert_eq!(std::fs::read(&graph).unwrap(), b"{}");
}
