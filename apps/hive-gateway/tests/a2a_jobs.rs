// Author: Lukas Bower
// Purpose: Verify A2A scoped discovery and refusal against the existing durable job ledger.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cohesix_authority::standing::{StandingControls, StandingScope, STANDING_SCOPE_SCHEMA};
use cohesix_authority::standing_ledger::StandingLedger;
use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketScope, TicketVerb,
};
use serde_json::{json, Value};

const AUTH: &str = "m28e-a2a-jobs-fixture";
const ISSUER: &str = "m28e-a2a-issuer-fixture";

struct Gateway(Child);

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

fn ticket(subject: &str, verb: TicketVerb, now: u64) -> String {
    TicketIssuer::new(ISSUER)
        .issue(
            TicketClaims::new(
                Role::Queen,
                BudgetSpec::unbounded()
                    .with_ops(Some(100))
                    .with_ttl(Some(300)),
                Some(subject.into()),
                MountSpec::empty(),
                now,
            )
            .with_scopes(vec![TicketScope::new("/host/tickets", verb, 100)]),
        )
        .unwrap()
        .encode()
        .unwrap()
}

fn exchange(
    port: u16,
    method: &str,
    path: &str,
    ticket: &str,
    request: Option<Value>,
) -> (u16, Value) {
    let body = request
        .map(|request| request.to_string())
        .unwrap_or_default();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nx-cohesix-auth: {AUTH}\r\nx-cohesix-ticket: {ticket}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let code = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (code, serde_json::from_str(body).unwrap())
}

#[test]
fn card_is_scoped_and_bad_task_data_cannot_reserve_a_job() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let graph = cohesix_authority::provider::registry().unwrap()["graph_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let scope = StandingScope {
        schema: STANDING_SCOPE_SCHEMA.into(),
        id: "a2a-gpu".into(),
        subject: "operator".into(),
        action: "gpu.workload.submit".into(),
        target: "/gpu/GPU-0/workload".into(),
        policy_sha256: graph.clone(),
        generation: 1,
        expires_unix_ms: now + 300_000,
        max_job_units: 1,
        max_total_units: 2,
        max_concurrent: 1,
        max_retries: 1,
        cooldown_ms: 0,
        fact_max_age_ms: 5_000,
        decision_ttl_ms: 5_000,
    };
    let scopes = root.join("scopes.json");
    private_file(
        &scopes,
        serde_json::to_string(&json!({
        "schema":"cohesix-standing-scope-file/v1","scopes":[scope.clone()] }))
        .unwrap()
        .as_bytes(),
    );
    let issuer = root.join("issuer");
    private_file(&issuer, ISSUER.as_bytes());
    let ledger_path = root.join("ledger.json");
    let controls = StandingControls::from_resolved_manifest(include_bytes!(
        "../../../configs/generated/root_task_resolved.json"
    ))
    .unwrap();
    let ledger = StandingLedger::new(ledger_path.clone(), graph, controls, vec![scope]).unwrap();
    ledger.initialize().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut child = Gateway(
        Command::new(env!("CARGO_BIN_EXE_hive-gateway"))
            .args([
                "--mock",
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--request-auth-token",
                AUTH,
                "--delegation-key-ref",
                &format!("file:{}", issuer.display()),
                "--standing-ledger",
                ledger_path.to_str().unwrap(),
                "--standing-scopes",
                scopes.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline);
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut reason = String::new();
            child
                .0
                .stderr
                .as_mut()
                .unwrap()
                .read_to_string(&mut reason)
                .unwrap();
            panic!("gateway exited {status}: {reason}");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let operator = ticket("operator", TicketVerb::ReadWrite, now);
    let other = ticket("other", TicketVerb::Read, now);
    let (status, card) = exchange(port, "GET", "/.well-known/agent-card.json", &operator, None);
    assert_eq!(status, 200);
    assert_eq!(card["protocolVersion"], "0.3.0");
    assert_eq!(card["skills"].as_array().unwrap().len(), 1);
    assert_eq!(card["skills"][0]["id"], "gpu.workload.submit");
    let (status, card) = exchange(port, "GET", "/.well-known/agent-card.json", &other, None);
    assert_eq!(status, 200);
    assert!(card["skills"].as_array().unwrap().is_empty());
    let (_, refused) = exchange(
        port,
        "POST",
        "/a2a",
        &operator,
        Some(json!({
            "jsonrpc":"2.0","id":1,"method":"message/send",
            "params":{"message":{"messageId":"request-1","role":"user", "parts":[
                {"kind":"data","data":{"skillId":"gpu.workload.submit",
                    "binding":{"admission_id":"forged"},"ticket":{"receipt":"caller"}}}]}}
        })),
    );
    assert_eq!(refused["error"]["code"], -32602);
    assert!(ledger.status("forged").unwrap().is_none());
    let (_, hidden) = exchange(
        port,
        "POST",
        "/a2a",
        &other,
        Some(json!({
        "jsonrpc":"2.0","id":2,"method":"tasks/get","params":{"id":"forged"}})),
    );
    assert_eq!(hidden["error"]["code"], -32001);
}
