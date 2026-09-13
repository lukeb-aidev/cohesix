// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate evidence pack export behavior (including ticket redaction).
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use coh::evidence::{build_local_bounds, export_pack, EvidencePackSpec};
use coh::policy::CohPolicy;
use coh::CohAudit;
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use cohsh::client::{CohClient, InProcessTransport};
use nine_door::{
    AuditConfig, AuditLimits, HostNamespaceConfig, HostProvider, NineDoor, PolicyConfig,
    ReplayConfig,
};
use tempfile::TempDir;

const EXPECTED_RETAINED_LOG_BYTES: usize = 2048 * (256 + 1);

#[test]
fn evidence_pack_redacts_ticket_payloads() -> Result<()> {
    let audit = AuditConfig::enabled(
        AuditLimits {
            journal_max_bytes: 8192,
            decisions_max_bytes: 4096,
        },
        ReplayConfig::enabled(64, 1024, 1024),
    );
    let server = NineDoor::new_with_host_policy_audit_config(
        HostNamespaceConfig::disabled(),
        PolicyConfig::disabled(),
        audit,
    );
    server.register_ticket_secret(Role::Queen, "bootstrap");

    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let issuer = TicketIssuer::new("bootstrap");
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        Some("auditor-test".to_owned()),
        MountSpec::empty(),
        0,
    );
    let token = issuer.issue(claims).context("issue ticket")?;
    let secret_ticket = token.encode().context("encode ticket")?;
    let mut client = CohClient::connect(transport, Role::Queen, Some(secret_ticket.as_str()))?;

    // Emit at least one control write so the audit journal stores the ticket value.
    let payload = cohsh::queen::spawn("heartbeat", ["ticks=1"].iter().copied())?;
    coh::CohAccess::write_append(
        &mut client,
        cohsh::queen::queen_ctl_path(),
        payload.as_bytes(),
    )?;

    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("pack");
    let spec = EvidencePackSpec {
        out_dir: out_dir.clone(),
        with_telemetry: false,
    };
    let policy = CohPolicy::from_generated();
    let bounds = build_local_bounds();
    let mut audit = CohAudit::new();
    export_pack(&mut client, &policy, &bounds, &spec, &mut audit)?;

    let journal_path = out_dir.join("audit").join("journal");
    let journal = std::fs::read_to_string(&journal_path)
        .with_context(|| format!("read {}", journal_path.display()))?;
    assert!(
        !journal.contains(&secret_ticket),
        "evidence pack leaked raw ticket"
    );
    assert!(
        journal.contains("sha256:"),
        "expected evidence pack to hash tickets"
    );

    Ok(())
}

#[test]
fn evidence_pack_redacts_host_ticket_sensitive_fields() -> Result<()> {
    let host = HostNamespaceConfig::enabled("/host", &[HostProvider::Systemd])?;
    let server = NineDoor::new_with_host_and_policy_config(host, PolicyConfig::disabled());
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let spec_line = r#"{"schema":"host-ticket/v1","id":"ticket-1","idempotency_key":"idem-1","action":"systemd.restart","target":"/host/systemd/cohesix-agent.service/restart","source_hive":"hive-a","target_hive":"hive-b","relay_hop":1,"relay_correlation_id":"ticket-1:idem-1:hive-a:hive-b","args":{"unit":"cohesix-agent.service","auth_token":"super-secret-token","auth_ref":"COHESIX_RELAY_HIVE_B_TOKEN"}}"#;
    coh::CohAccess::write_append(
        &mut client,
        "/host/tickets/spec",
        format!("{spec_line}\n").as_bytes(),
    )?;

    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("pack");
    let spec = EvidencePackSpec {
        out_dir: out_dir.clone(),
        with_telemetry: false,
    };
    let policy = CohPolicy::from_generated();
    let bounds = build_local_bounds();
    let mut audit = CohAudit::new();
    export_pack(&mut client, &policy, &bounds, &spec, &mut audit)?;

    let captured = std::fs::read_to_string(out_dir.join("host").join("tickets").join("spec"))
        .context("read host ticket spec capture")?;
    assert!(!captured.contains("super-secret-token"));
    assert!(!captured.contains("COHESIX_RELAY_HIVE_B_TOKEN"));
    assert!(captured.contains("<redacted>"));

    Ok(())
}

#[test]
fn evidence_pack_log_capture_covers_retained_queen_log_window() -> Result<()> {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("pack");
    let spec = EvidencePackSpec {
        out_dir: out_dir.clone(),
        with_telemetry: false,
    };
    let policy = CohPolicy::from_generated();
    let bounds = build_local_bounds();
    let mut audit = CohAudit::new();
    let mut client = RecordingAccess::default();

    export_pack(&mut client, &policy, &bounds, &spec, &mut audit)?;

    assert_eq!(
        client.log_read_limit,
        Some(EXPECTED_RETAINED_LOG_BYTES),
        "evidence pack should read /log/queen.log with the full retained-window cap"
    );
    assert_eq!(
        client.log_tail_limit, None,
        "evidence pack should not use the default tail window for /log/queen.log"
    );
    let log_path = out_dir.join("log").join("queen.log");
    let captured = std::fs::metadata(&log_path)
        .with_context(|| format!("stat {}", log_path.display()))?
        .len();
    assert_eq!(captured, EXPECTED_RETAINED_LOG_BYTES as u64);

    Ok(())
}

#[derive(Default)]
struct RecordingAccess {
    log_read_limit: Option<usize>,
    log_tail_limit: Option<usize>,
}

impl coh::CohAccess for RecordingAccess {
    fn list_dir(&mut self, _path: &str, _max_bytes: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        match path {
            "/proc/boot" => Ok(b"boot=ok\n".to_vec()),
            "/proc/schedule/summary" => Ok(b"schedule=ok\n".to_vec()),
            "/proc/schedule/queue" => Ok(b"queue=empty\n".to_vec()),
            "/proc/lease/summary" => Ok(b"lease=ok\n".to_vec()),
            "/proc/lease/active" => Ok(b"active=none\n".to_vec()),
            "/proc/lease/preemptions" => Ok(b"preemptions=0\n".to_vec()),
            "/log/queen.log" => {
                self.log_read_limit = Some(max_bytes);
                Ok(vec![b'x'; max_bytes])
            }
            _ => Err(anyhow!("not found")),
        }
    }

    fn tail_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        if path == "/log/queen.log" {
            self.log_tail_limit = Some(max_bytes);
        }
        Err(anyhow!("not found"))
    }

    fn write_append(&mut self, _path: &str, payload: &[u8]) -> Result<usize> {
        Ok(payload.len())
    }
}

#[test]
fn failed_required_read_preserves_partial_summary_without_continuing() -> Result<()> {
    assert_failed_capture("/log/queen.log", false)
}

#[test]
fn failed_optional_read_is_not_a_successful_export() -> Result<()> {
    assert_failed_capture("/host/tickets/status", true)
}

fn assert_failed_capture(failure_path: &'static str, expect_host_reads: bool) -> Result<()> {
    struct FailingAccess {
        inner: RecordingAccess,
        failure_path: &'static str,
        paths: Vec<String>,
    }
    impl coh::CohAccess for FailingAccess {
        fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
            self.inner.list_dir(path, max_bytes)
        }
        fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
            self.paths.push(path.to_owned());
            if path == self.failure_path {
                return Err(anyhow!("ERR CAT reason=quota path={path} error=ELIMIT"));
            }
            self.inner.read_file(path, max_bytes)
        }
        fn write_append(&mut self, _path: &str, _payload: &[u8]) -> Result<usize> {
            Err(anyhow!("read-only export must not write to the target"))
        }
    }
    let temp = TempDir::new()?;
    let spec = EvidencePackSpec {
        out_dir: temp.path().join("pack"),
        with_telemetry: false,
    };
    let mut client = FailingAccess {
        inner: RecordingAccess::default(),
        failure_path,
        paths: Vec::new(),
    };
    let result = export_pack(
        &mut client,
        &CohPolicy::from_generated(),
        &build_local_bounds(),
        &spec,
        &mut CohAudit::new(),
    );
    assert!(result.is_err(), "a capture refusal must fail the export");
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(spec.out_dir.join("summary.json"))?)?;
    assert_eq!(summary["errors"], 1);
    let items = summary["items"].as_array().context("summary items")?;
    let error = items
        .iter()
        .find(|item| item["path"] == failure_path)
        .context("failed path must be retained")?;
    assert_eq!(error["status"], "error");
    assert!(error["detail"]
        .as_str()
        .context("error detail")?
        .contains("ELIMIT"));
    assert_eq!(std::fs::read(spec.out_dir.join("proc/boot"))?, b"boot=ok\n");
    assert_eq!(
        client.paths.iter().any(|path| path.starts_with("/host/")),
        expect_host_reads
    );
    assert!(!spec
        .out_dir
        .join(failure_path.trim_start_matches('/'))
        .exists());
    Ok(())
}

#[test]
fn malformed_audit_payload_is_not_saved_or_reported_as_captured() -> Result<()> {
    struct MalformedAudit(RecordingAccess);
    impl coh::CohAccess for MalformedAudit {
        fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
            self.0.list_dir(path, max_bytes)
        }
        fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
            match path {
                "/audit/export" => Ok(br#"{"journal_base":0,"journal_next":16,"decisions_base":0,"decisions_next":0}"#.to_vec()),
                "/audit/journal" => Ok(b"not-json-secret".to_vec()),
                _ => self.0.read_file(path, max_bytes),
            }
        }
        fn write_append(&mut self, _path: &str, _payload: &[u8]) -> Result<usize> {
            Err(anyhow!("read-only export must not write to the target"))
        }
    }
    let temp = TempDir::new()?;
    let spec = EvidencePackSpec {
        out_dir: temp.path().join("pack"),
        with_telemetry: false,
    };
    let result = export_pack(
        &mut MalformedAudit(RecordingAccess::default()),
        &CohPolicy::from_generated(),
        &build_local_bounds(),
        &spec,
        &mut CohAudit::new(),
    );
    assert!(result.is_err());
    let bytes = std::fs::read(spec.out_dir.join("summary.json"))?;
    let summary: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(summary["errors"], 1);
    let row = summary["items"]
        .as_array()
        .context("summary items")?
        .iter()
        .find(|row| row["path"] == "/audit/journal")
        .context("journal row")?;
    assert_eq!(row["status"], "error");
    assert_eq!(row["bytes"], serde_json::Value::Null);
    assert!(!spec.out_dir.join("audit/journal").exists());
    assert!(!String::from_utf8(bytes)?.contains("not-json-secret"));
    Ok(())
}
