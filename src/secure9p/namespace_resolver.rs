// CLASSIFICATION: COMMUNITY
// Filename: namespace_resolver.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

pub fn resolve(agent: &str) -> String {
    format!("/srv/namespaces/{}", agent)
}
