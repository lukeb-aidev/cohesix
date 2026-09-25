// Author: Lukas Bower
// Purpose: Check scoped MCP workflow discovery and refusal against the durable selected ledger.
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

const AUTH: &str = "m28d-mcp-workflow-auth-fixture";
const ISSUER: &str = "m28d-mcp-workflow-issuer-fixture";

struct GatewayChild(Child);

impl Drop for GatewayChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn private_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("write private fixture");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("private fixture permissions");
}

fn ticket(subject: &str, verb: TicketVerb, now_ms: u64) -> String {
    TicketIssuer::new(ISSUER)
        .issue(
            TicketClaims::new(
                Role::Queen,
                BudgetSpec::unbounded()
                    .with_ops(Some(100))
                    .with_ttl(Some(300)),
                Some(subject.into()),
                MountSpec::empty(),
                now_ms,
            )
            .with_scopes(vec![TicketScope::new("/host/tickets", verb, 100)]),
        )
        .expect("fixture ticket")
        .encode()
        .expect("encoded fixture ticket")
}

fn exchange(port: u16, ticket: &str, request: Value) -> (u16, Value) {
    let body = serde_json::to_string(&request).expect("JSON-RPC request");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("gateway connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("gateway timeout");
    write!(stream, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: 2025-11-25\r\nx-cohesix-auth: {AUTH}\r\nx-cohesix-ticket: {ticket}\r\nContent-Length: {}\r\n\r\n{body}", body.len())
        .expect("send JSON-RPC request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("gateway response");
    let (head, body) = response.split_once("\r\n\r\n").expect("HTTP framing");
    let status = head
        .lines()
        .next()
        .expect("HTTP status")
        .split_whitespace()
        .nth(1)
        .expect("HTTP code")
        .parse()
        .expect("numeric HTTP code");
    (
        status,
        serde_json::from_str(body).expect("JSON-RPC response"),
    )
}

#[test]
fn selected_discovery_and_preflight_refusal_do_not_create_an_effect() {
    let directory = tempfile::tempdir().expect("private fixture root");
    let root = directory
        .path()
        .canonicalize()
        .expect("absolute fixture root");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let now_ms = now.as_millis() as u64;
    let graph = cohesix_authority::provider::registry().unwrap()["graph_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let scope = StandingScope {
        schema: STANDING_SCOPE_SCHEMA.into(),
        id: "mcp-gpu".into(),
        subject: "m".into(),
        action: "gpu.workload.submit".into(),
        target: "/gpu/GPU-0/workload".into(),
        policy_sha256: graph.clone(),
        generation: 1,
        expires_unix_ms: now_ms + 300_000,
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
        serde_json::to_string(
            &json!({"schema":"cohesix-standing-scope-file/v1","scopes":[scope.clone()]}),
        )
        .unwrap()
        .as_bytes(),
    );
    let issuer = root.join("issuer");
    private_file(&issuer, ISSUER.as_bytes());
    let ledger_path = root.join("ledger.json");
    let manifest = include_bytes!("../../../configs/generated/root_task_resolved.json");
    let controls = StandingControls::from_resolved_manifest(manifest).unwrap();
    let ledger = StandingLedger::new(ledger_path.clone(), graph, controls, vec![scope]).unwrap();
    ledger.initialize().expect("provision selected ledger");

    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve loopback port");
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let mut child = GatewayChild(
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
            .expect("start selected gateway"),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "gateway did not start");
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut diagnostic = String::new();
            child
                .0
                .stderr
                .as_mut()
                .unwrap()
                .read_to_string(&mut diagnostic)
                .unwrap();
            panic!("gateway refused fixture ({status}): {diagnostic}");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let authorized = ticket("m", TicketVerb::ReadWrite, now_ms);
    let other = ticket("other", TicketVerb::Read, now_ms);
    let (status, list) = exchange(
        port,
        &authorized,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    );
    assert_eq!(status, 200);
    let tools = list["result"]["tools"].as_array().unwrap();
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "cohesix.preflight_selected_job"));
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "cohesix.submit_selected_job"));
    let submit = tools
        .iter()
        .find(|tool| tool["name"] == "cohesix.submit_selected_job")
        .unwrap();
    assert_eq!(
        submit["inputSchema"]["properties"]["ticket"]["properties"]["action"]["enum"],
        json!(["gpu.workload.submit"])
    );
    assert_eq!(
        submit["outputSchema"]["properties"]["record"]["type"],
        "object"
    );
    assert!(tools
        .iter()
        .any(|tool| tool["name"] == "cohesix.available_selected_jobs"));
    let (_, other_list) = exchange(
        port,
        &other,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    );
    let other_tools = other_list["result"]["tools"].as_array().unwrap();
    assert!(!other_tools
        .iter()
        .any(|tool| tool["name"] == "cohesix.submit_selected_job"));
    assert!(!other_tools
        .iter()
        .any(|tool| tool["name"] == "cohesix.preflight_selected_job"));

    let (_, refused) = exchange(
        port,
        &authorized,
        json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"cohesix.preflight_selected_job","arguments":{"scope_id":"mcp-gpu","ticket":{"receipt":"caller-authored"}}}
        }),
    );
    assert_eq!(refused["result"]["isError"], true);
    assert!(ledger.status("caller-authored").unwrap().is_none());
    let (_, resource) = exchange(
        port,
        &authorized,
        json!({
            "jsonrpc":"2.0","id":23,"method":"resources/read","params":{"uri":"cohesix://jobs/../other"}
        }),
    );
    assert_eq!(resource["id"], 23);
    assert_eq!(resource["error"]["code"], -32602);
}
