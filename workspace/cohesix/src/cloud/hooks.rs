// CLASSIFICATION: COMMUNITY
// Filename: hooks.rs v0.3
// Author: Lukas Bower
// Date Modified: 2027-08-17

/// Cloud initialization hooks for Queen nodes.
use std::fs;
use ureq::Agent;

/// Fetch remote agent configs if `/srv/cloudinit` exists.
pub fn run_cloud_hooks() {
    if let Ok(url) = fs::read_to_string("/srv/cloudinit") {
        if let Ok(resp) = Agent::new_with_defaults().get(url.trim()).call() {
            if let Ok(body) = resp.into_string() {
                fs::create_dir_all("/srv/agents").ok();
                let _ = fs::write("/srv/agents/config.json", body);
            }
        }
    }
}
