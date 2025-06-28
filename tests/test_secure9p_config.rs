// CLASSIFICATION: COMMUNITY
// Filename: test_secure9p_config.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-10-28

use std::fs;

#[test]
fn secure9p_roles_match_manifest() {
    let cfg = fs::read_to_string("config/secure9p.toml").expect("config missing");
    let mut roles_in_cfg = Vec::new();
    for line in cfg.lines() {
        if line.starts_with("agent = ") {
            if let Some(agent) = line.split('=').nth(1) {
                roles_in_cfg.push(agent.trim_matches(' ').trim_matches('"'));
            }
        }
    }
    for role in cohesix::cohesix_types::VALID_ROLES {
        let name = role.to_ascii_lowercase();
        assert!(roles_in_cfg.iter().any(|r| r == &name), "{} missing", role);
    }
}
