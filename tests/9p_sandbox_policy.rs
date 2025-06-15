// CLASSIFICATION: COMMUNITY
// Filename: 9p_sandbox_policy.rs v0.1
// Date Modified: 2025-07-23
// Author: Cohesix Codex

use cohesix_9p::{policy::SandboxPolicy, FsConfig, FsServer};
use ninep::client::TcpClient;
use serial_test::serial;
use tempfile::tempdir;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

#[test]
#[serial]
fn sandbox_violation_logged() {
    if std::net::TcpListener::bind("127.0.0.1:5670").is_err() {
        eprintln!("skipping sandbox_violation_logged: port busy");
        return;
    }
    let dir = tempdir().unwrap();
    let log_dir = dir.path().join("log");
    fs::create_dir_all(&log_dir).unwrap();
    unsafe {
        std::env::set_var("COHESIX_LOG_DIR", &log_dir);
    }
    let mut srv = FsServer::new(FsConfig {
        port: 5670,
        ..Default::default()
    });
    srv.set_validator_hook(Arc::new(|ty, file, agent, time| {
        cohesix::validator::log_violation(cohesix::validator::RuleViolation {
            type_: ty,
            file,
            agent,
            time,
        });
    }));
    let mut policy = SandboxPolicy::default();
    policy.read.push("/".into());
    policy.write.push("/allowed".into());
    srv.set_policy("tester".into(), policy);
    srv.start().unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let mut cli = TcpClient::new_tcp("tester".to_string(), "127.0.0.1:5670", "").unwrap();
    let res = cli.write("/secret/data", 0, b"x");
    assert!(res.is_err());

    let log = fs::read_to_string(log_dir.join("validator_runtime.log")).unwrap();
    assert!(log.contains("/secret/data"));
}
