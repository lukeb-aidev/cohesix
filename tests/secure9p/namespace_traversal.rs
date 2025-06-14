// CLASSIFICATION: COMMUNITY
// Filename: namespace_traversal.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use cohesix::secure9p::namespace_resolver::resolve;

#[test]
fn namespace_traversal() {
    let ns = resolve("agent1");
    assert_eq!(ns, "/srv/namespaces/agent1");
}
