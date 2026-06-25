// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI queen log dump projection behavior.
// Author: Lukas Bower

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::Result;
use cohesix_ticket::Role;
use cohsh::{Session as CohshSession, Transport as CohshTransport};
use secure9p_codec::SessionId;
use swarmui::{SwarmUiConfig, SwarmUiConsoleBackend};

struct TestTransport {
    reads: Arc<AtomicUsize>,
}

impl TestTransport {
    fn new(reads: Arc<AtomicUsize>) -> Self {
        Self { reads }
    }
}

impl CohshTransport for TestTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<CohshSession> {
        Ok(CohshSession::new(SessionId::BOOTSTRAP, role))
    }

    fn kind(&self) -> &'static str {
        "test"
    }

    fn ping(&mut self, _session: &CohshSession) -> Result<String> {
        Ok("pong".to_owned())
    }

    fn tail(&mut self, _session: &CohshSession, _path: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn read(&mut self, _session: &CohshSession, path: &str) -> Result<Vec<String>> {
        assert_eq!(path, "/log/queen.log");
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(vec![
            "boot marker".to_owned(),
            "benchmark phase=end".to_owned(),
        ])
    }

    fn list(&mut self, _session: &CohshSession, _path: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn write(&mut self, _session: &CohshSession, _path: &str, _payload: &[u8]) -> Result<()> {
        Ok(())
    }
}

#[test]
fn console_dump_queen_log_uses_active_session_read() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config = SwarmUiConfig::from_generated(temp_dir.path().to_path_buf());
    let reads = Arc::new(AtomicUsize::new(0));
    let transport = TestTransport::new(reads.clone());
    let mut backend = SwarmUiConsoleBackend::with_transport(config, transport);

    let attach = backend.attach(Role::Queen, None);
    assert!(attach.ok);
    let dump = backend.dump_queen_log().expect("log dump");

    assert_eq!(dump.filename, "queen.log.txt");
    assert_eq!(dump.lines, 2);
    assert_eq!(dump.bytes, "boot marker\nbenchmark phase=end\n".len());
    assert_eq!(dump.text, "boot marker\nbenchmark phase=end\n");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}
