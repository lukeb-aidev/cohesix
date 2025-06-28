// CLASSIFICATION: COMMUNITY
// Filename: test_secure9p_config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

use std::fs;

#[test]
fn secure9p_roles_match_manifest() {
    let cfg = fs::read_to_string("config/secure9p.toml").expect("config missing");
    let mut roles_in_cfg = Vec::new();
    for line in cfg.lines() {
        if line.starts_with("agent = ") {
            if let Some(agent) = line.split('=').nth(1) {
                let cleaned = agent
                    .trim_matches(' ')
                    .trim_matches('"')
                    .to_string();
                roles_in_cfg.push(cleaned);
            }
        }
    }
    for role in cohesix::cohesix_types::VALID_ROLES {
        let mut name = String::new();
        for (i, ch) in role.chars().enumerate() {
            if ch.is_uppercase() {
                if i != 0 {
                    name.push('_');
                }
                name.push(ch.to_ascii_lowercase());
            } else {
                name.push(ch);
            }
        }
        name = name.to_ascii_lowercase();
        assert!(roles_in_cfg.iter().any(|r| r == &name), "{} missing", role);
    }
}
