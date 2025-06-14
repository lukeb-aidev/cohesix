// CLASSIFICATION: COMMUNITY
// Filename: namespace_traversal.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

#[test]
fn namespace_traversal() {
    let ns = format!("/srv/namespaces/{}", "agent1");
    assert_eq!(ns, "/srv/namespaces/agent1");
}
