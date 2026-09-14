// Author: Lukas Bower
// Purpose: Preserve regression ticket claims while binding fixture signatures to the selected test issuer.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, ensure, Context, Result};
use clap::Parser;
use coh_rtc::ir::{self, Role, TicketSpec};
use cohesix_ticket::{TicketIssuer, TicketKey, TicketToken};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_SCRIPT_BYTES: usize = 65_536;

#[derive(Parser)]
struct Args {
    /// Canonical development manifest that signed the checked-in fixtures.
    #[arg(long)]
    fixture_manifest: PathBuf,
    /// Selected target manifest with the test issuer's secret or reference.
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    script: PathBuf,
    /// New retained script; existing evidence is never overwritten.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Serialize)]
struct Binding {
    line: usize,
    claims_sha256: String,
    fixture_ticket_sha256: String,
    selected_ticket_sha256: String,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn role_name(role: cohesix_ticket::Role) -> Role {
    match role {
        cohesix_ticket::Role::Queen => Role::Queen,
        cohesix_ticket::Role::WorkerHeartbeat => Role::WorkerHeartbeat,
        cohesix_ticket::Role::WorkerGpu => Role::WorkerGpu,
        cohesix_ticket::Role::WorkerBus => Role::WorkerBus,
        cohesix_ticket::Role::WorkerLora => Role::WorkerLora,
    }
}

fn secret(tickets: &[TicketSpec], role: Role) -> Result<String> {
    let matches: Vec<_> = tickets.iter().filter(|item| item.role == role).collect();
    ensure!(
        matches.len() == 1,
        "fixture role must have exactly one issuer"
    );
    let ticket = matches[0];
    if let Some(reference) = &ticket.secret_ref {
        ensure!(ticket.secret.is_empty(), "ambiguous fixture issuer source");
        return cohesix_authority::secret::resolve_reference(reference)
            .map_err(|error| anyhow::anyhow!("fixture issuer reference: {error}"));
    }
    // Published development keys are legitimate only in this offline fixture
    // compiler. Live authentication retains its separate placeholder refusal.
    ensure!(
        !ticket.secret.is_empty() && ticket.secret.len() <= 4096,
        "invalid fixture issuer"
    );
    Ok(ticket.secret.clone())
}

fn materialize(
    source: &str,
    fixtures: &[TicketSpec],
    selected: &[TicketSpec],
) -> Result<(String, Vec<Binding>)> {
    ensure!(
        source.len() <= MAX_SCRIPT_BYTES,
        "regression script exceeds byte bound"
    );
    let mut output = String::with_capacity(source.len());
    let mut bindings = Vec::new();
    for (index, line) in source.split_inclusive('\n').enumerate() {
        if line.trim_start().starts_with('#') || !line.contains("cohesix-ticket-") {
            output.push_str(line);
            continue;
        }
        let words: Vec<_> = line.split_whitespace().collect();
        ensure!(
            words.len() == 3
                && words[0].eq_ignore_ascii_case("attach")
                && words[2].starts_with("cohesix-ticket-"),
            "unsupported literal fixture ticket syntax"
        );
        let token = words[2];
        let role = role_name(TicketToken::decode_unverified(token)?.role);
        ensure!(
            words[1] == role.as_str() || (words[1] == "worker" && role == Role::WorkerHeartbeat),
            "fixture attach role differs from signed role"
        );
        let verified =
            TicketToken::decode(token, &TicketKey::from_secret(&secret(fixtures, role)?))
                .context("fixture signature does not match the canonical development issuer")?;
        let replacement = TicketIssuer::new(&secret(selected, role)?)
            .issue(verified.claims().clone())?
            .encode()?;
        let (original_payload, _) = token.split_once('.').context("fixture MAC separator")?;
        let (replacement_payload, _) = replacement
            .split_once('.')
            .context("issued MAC separator")?;
        // This is the key invariant: expiry, mount, scopes, quotas and every
        // claim byte remain the independent checked-in fixture's exact bytes.
        ensure!(
            original_payload == replacement_payload,
            "fixture claim bytes changed"
        );
        bindings.push(Binding {
            line: index + 1,
            claims_sha256: digest(
                &hex::decode(
                    original_payload
                        .strip_prefix("cohesix-ticket-")
                        .context("fixture ticket prefix")?,
                )
                .map_err(|error| anyhow::anyhow!("fixture payload hex: {error}"))?,
            ),
            fixture_ticket_sha256: digest(token.as_bytes()),
            selected_ticket_sha256: digest(replacement.as_bytes()),
        });
        output.push_str(&line.replacen(token, &replacement, 1));
    }
    Ok((output, bindings))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let fixture = ir::load_manifest(&args.fixture_manifest)?;
    let selected = ir::load_manifest(&args.manifest)?;
    let mut bytes = Vec::new();
    fs::File::open(&args.script)?
        .take((MAX_SCRIPT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SCRIPT_BYTES {
        bail!("regression script exceeds byte bound");
    }
    let source = std::str::from_utf8(&bytes)?;
    let (output, bindings) = materialize(source, &fixture.tickets, &selected.tickets)?;
    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent)?;
    }
    let record = serde_json::json!({
        "schema": "cohesix-regression-ticket-binding/v1",
        "script_sha256": digest(&bytes),
        "selected_script_sha256": digest(output.as_bytes()),
        "fixture_manifest_sha256": digest(&fs::read(&args.fixture_manifest)?),
        "selected_manifest_sha256": digest(&fs::read(&args.manifest)?),
        "claim_bytes_unchanged": true,
        "tickets": bindings,
    });
    write_new(&args.out, output.as_bytes())?;
    write_new(
        &args.out.with_extension("tickets.json"),
        &serde_json::to_vec_pretty(&record)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuers(secret: &str) -> Vec<TicketSpec> {
        vec![TicketSpec {
            role: Role::Queen,
            secret: secret.into(),
            secret_ref: None,
        }]
    }

    #[test]
    fn canonical_expiry_scope_and_quota_claims_are_byte_exact() {
        let source = include_str!("../../../../scripts/cohsh/telemetry_ring.coh");
        let (output, bindings) = materialize(
            source,
            &issuers("bootstrap"),
            &issuers("selected-test-issuer"),
        )
        .unwrap();
        assert_eq!(bindings.len(), 5);
        for (old, new) in source.lines().zip(output.lines()) {
            if !old.starts_with("attach queen cohesix-ticket-") {
                assert_eq!(old, new);
                continue;
            }
            let old = old.split_whitespace().nth(2).unwrap();
            let new = new.split_whitespace().nth(2).unwrap();
            assert_eq!(
                old.split_once('.').unwrap().0,
                new.split_once('.').unwrap().0
            );
            assert_ne!(old, new);
            assert!(
                TicketToken::decode(new, &TicketKey::from_secret("selected-test-issuer")).is_ok()
            );
            assert!(TicketToken::decode(new, &TicketKey::from_secret("bootstrap")).is_err());
        }
    }

    #[test]
    fn unknown_signature_missing_issuer_and_changed_role_are_refused() {
        let source = include_str!("../../../../scripts/cohsh/telemetry_ring.coh");
        assert!(materialize(
            source,
            &issuers("wrong-fixture-issuer"),
            &issuers("selected-test-issuer")
        )
        .is_err());
        assert!(materialize(source, &issuers("bootstrap"), &[]).is_err());
        assert!(materialize(
            &source.replacen("attach queen", "attach worker", 1),
            &issuers("bootstrap"),
            &issuers("selected-test-issuer")
        )
        .is_err());
    }

    #[test]
    fn sidecar_subject_and_scopes_survive_issuer_rotation() {
        let source = include_str!("../../../../scripts/cohsh/sidecar_integration.coh");
        let mut fixture = issuers("worker-bus");
        fixture[0].role = Role::WorkerBus;
        let mut selected = issuers("selected-bus-test-issuer");
        selected[0].role = Role::WorkerBus;
        let (output, bindings) = materialize(source, &fixture, &selected).unwrap();
        assert_eq!(bindings.len(), 1);
        let old = source
            .lines()
            .nth(3)
            .unwrap()
            .split_whitespace()
            .nth(2)
            .unwrap();
        let new = output
            .lines()
            .nth(3)
            .unwrap()
            .split_whitespace()
            .nth(2)
            .unwrap();
        let old = TicketToken::decode(old, &TicketKey::from_secret("worker-bus")).unwrap();
        let new =
            TicketToken::decode(new, &TicketKey::from_secret("selected-bus-test-issuer")).unwrap();
        assert_eq!(old.claims(), new.claims());
        assert_eq!(
            output.lines().skip(4).collect::<Vec<_>>(),
            source.lines().skip(4).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ordinary_commands_and_comments_are_unchanged_and_input_is_bounded() {
        let source = "# cohesix-ticket-example\nattach queen\nEXPECT OK\nquit\n";
        let (output, bindings) = materialize(source, &[], &[]).unwrap();
        assert_eq!(output, source);
        assert!(bindings.is_empty());
        assert!(materialize(&"x".repeat(MAX_SCRIPT_BYTES + 1), &[], &[]).is_err());
    }

    #[test]
    fn evidence_is_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.coh");
        write_new(&path, b"first").unwrap();
        assert!(write_new(&path, b"second").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"first");
    }
}
