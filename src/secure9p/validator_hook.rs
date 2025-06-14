// CLASSIFICATION: COMMUNITY
// Filename: validator_hook.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! Structured logging for Secure9P requests.

#[cfg(feature = "secure9p")]
use serde::Serialize;
#[cfg(feature = "secure9p")]
use std::{fs::OpenOptions, io::Write, path::PathBuf};

#[cfg(feature = "secure9p")]
#[derive(Serialize)]
struct Record<'a> {
    agent: &'a str,
    op: &'a str,
    path: &'a str,
    result: &'a str,
    ts: i64,
}

#[cfg(feature = "secure9p")]
#[derive(Clone)]
pub struct ValidatorHook { path: PathBuf }

#[cfg(feature = "secure9p")]
impl ValidatorHook {
    pub fn new(path: PathBuf) -> Self { Self { path } }

    pub fn log(&self, agent: &str, op: &str, path: &str, result: &str) {
        let rec = Record {
            agent,
            op,
            path,
            result,
            ts: chrono::Utc::now().timestamp(),
        };
        if let Ok(line) = serde_json::to_string(&rec) {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
                let _ = writeln!(f, "{}", line);
            }
        }
    }
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_log() {
        let dir = std::env::temp_dir();
        let path = dir.join("v.log");
        let hook = ValidatorHook::new(path.clone());
        hook.log("a", "read", "/x", "ok");
        let txt = fs::read_to_string(path).unwrap();
        assert!(txt.contains("\"agent\":"));
    }
}
