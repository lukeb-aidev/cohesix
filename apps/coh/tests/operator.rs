// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Protect read-only source boundaries, exact diffs, redaction and pack confinement.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Result};
use coh::operator::{self, Availability};
use coh::{CohAccess, CohAudit};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

struct ReadOnly(BTreeMap<String, Vec<u8>>);

impl CohAccess for ReadOnly {
    fn list_dir(&mut self, path: &str, _: usize) -> Result<Vec<String>> {
        if path == "/proc/root" {
            Ok(vec!["reachable".to_owned(), "cut_reason".to_owned()])
        } else {
            Err(anyhow!("not found"))
        }
    }
    fn read_file(&mut self, path: &str, max: usize) -> Result<Vec<u8>> {
        let bytes = self.0.get(path).ok_or_else(|| anyhow!("not found"))?;
        anyhow::ensure!(bytes.len() <= max, "read bound");
        Ok(bytes.clone())
    }
    fn write_append(&mut self, _: &str, _: &[u8]) -> Result<usize> {
        panic!("operator operation attempted a write")
    }
}

fn source() -> ReadOnly {
    ReadOnly(BTreeMap::from([
        (
            "/proc/boot".to_owned(),
            b"manifest_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
                .to_vec(),
        ),
        (
            "/proc/root/reachable".to_owned(),
            b"reachable=yes\n".to_vec(),
        ),
        (
            "/proc/root/cut_reason".to_owned(),
            b"cut_reason=none\n".to_vec(),
        ),
    ]))
}

#[test]
fn canonical_cross_language_redaction_fixture() -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/operator/contracts.json"
    ))?;
    let sanitized = operator::sanitize(&serde_json::to_vec(&value["redaction"]["input"])?)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&sanitized)?,
        value["redaction"]["expected"]
    );
    Ok(())
}

#[test]
fn readonly_inspection_retains_unknown_capabilities_without_health_inference() -> Result<()> {
    let snapshot = operator::inspect_live(&mut source(), "live-console")?;
    assert!(snapshot.violations.is_empty());
    assert!(snapshot
        .observations
        .iter()
        .any(|o| o.path == "/proc/spool/status" && o.status == Availability::Missing));
    assert_eq!(operator::diff(&snapshot, &snapshot)?, vec![]);
    assert_eq!(
        snapshot.render()?,
        operator::inspect_live(&mut source(), "live-console")?.render()?
    );
    assert!(!snapshot.render()?.contains("healthy"));
    Ok(())
}

#[test]
fn canonical_pack_inspect_diff_roundtrip_is_readonly() -> Result<()> {
    let temp = TempDir::new()?;
    let mut client = source();
    coh::evidence::export_pack(
        &mut client,
        &coh::policy::CohPolicy::from_generated(),
        &coh::evidence::build_local_bounds(),
        &coh::evidence::EvidencePackSpec {
            out_dir: temp.path().to_owned(),
            with_telemetry: false,
        },
        &mut CohAudit::new(),
    )?;
    let original = operator::inspect_pack(temp.path())?;
    assert!(original.violations.is_empty());
    fs::write(temp.path().join("proc/root/reachable"), b"reachable=no\n")?;
    let changed = operator::inspect_pack(temp.path())?;
    let diff = operator::diff(&original, &changed)?;
    assert_eq!(
        serde_json::to_value(diff)?,
        json!([{"field":"/proc/root/reachable/@content","before":"reachable=yes\n","after":"reachable=no\n"}])
    );
    assert_eq!(original.source_class, "offline-pack");
    Ok(())
}

#[test]
fn embedded_secrets_and_peer_errors_never_enter_output() -> Result<()> {
    let bytes = br#"{"payload":"{\"args\":{\"auth_token\":\"CANARY1\"},\"ticket\":\"CANARY2\"}","password":"CANARY3","future":{"credential":"CANARY4"}}"#;
    let text = operator::sanitize(bytes)?;
    for canary in ["CANARY1", "CANARY2", "CANARY3", "CANARY4"] {
        assert!(!text.contains(canary));
    }
    assert_eq!(operator::sanitize(b"error token=CANARY\n")?, "<redacted>\n");
    Ok(())
}

#[test]
fn captured_missing_file_and_inconsistent_inventory_are_violations() -> Result<()> {
    let temp = TempDir::new()?;
    fs::write(temp.path().join("summary.json"), json!({"schema":"cohesix-evidence-pack/summary-v1","captured":2,"missing":0,"errors":0,"items":[{"path":"/proc/boot","saved_as":"proc/boot","status":"captured"}]}).to_string())?;
    assert_eq!(
        operator::inspect_pack(temp.path())?.violations,
        [
            "captured-file-unreadable:proc/boot",
            "inventory-count:captured"
        ]
    );
    Ok(())
}

#[test]
fn traversal_links_and_oversize_inputs_fail_before_reading() -> Result<()> {
    let temp = TempDir::new()?;
    for path in ["../secret", "/secret", "proc/../../secret", "a\nsecret"] {
        assert!(operator::confined_path(temp.path(), path).is_err());
    }
    let file = temp.path().join("large");
    fs::File::create(&file)?.set_len(operator::MAX_BYTES as u64 + 1)?;
    assert!(operator::read_bounded(&file, operator::MAX_BYTES).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path(), temp.path().join("link"))?;
        assert!(operator::confined_path(temp.path(), "link/large").is_err());
    }
    Ok(())
}

#[test]
fn bundle_attachments_bind_sanitized_bytes_and_reject_corruption() -> Result<()> {
    let temp = TempDir::new()?;
    let source = TempDir::new()?;
    coh::evidence::export_pack(
        &mut self::source(),
        &coh::policy::CohPolicy::from_generated(),
        &coh::evidence::build_local_bounds(),
        &coh::evidence::EvidencePackSpec {
            out_dir: temp.path().to_owned(),
            with_telemetry: false,
        },
        &mut CohAudit::new(),
    )?;
    let manifest = source.path().join("manifest.toml");
    fs::write(&manifest, "[tickets]\nsecret = 'ATTACHMENT_CANARY'\n")?;
    let serial = source.path().join("serial.log");
    fs::write(
        &serial,
        "AUTH SERIAL_CANARY\nOK CAT path=/proc/boot\ntoken=SERIAL_CANARY\nEND\n",
    )?;
    coh::evidence::attach_artifacts(
        temp.path(),
        Some(&manifest),
        None,
        Some(&serial),
        None,
        None,
    )?;
    let snapshot = operator::inspect_pack(temp.path())?;
    assert!(snapshot.violations.is_empty());
    assert!(!snapshot.render()?.contains("CANARY"));
    let refs: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.path().join("artifact_refs.json"))?)?;
    let retained = fs::read(temp.path().join("attachments/manifest.json"))?;
    assert_eq!(
        refs["items"]["attachments/manifest.json"]["sha256"],
        operator::digest(&retained)
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("pack.sha256"))?.trim(),
        operator::digest(&fs::read(temp.path().join("checksums.json"))?)
    );
    fs::write(temp.path().join("attachments/manifest.json"), b"{}")?;
    assert!(operator::inspect_pack(temp.path())?
        .violations
        .contains(&"attachment-identity:attachments/manifest.json".to_owned()));
    Ok(())
}
