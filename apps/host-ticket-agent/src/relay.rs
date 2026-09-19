// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Relay federated host tickets across hives with deterministic WAL-backed delivery.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, ensure, Context, Result};
use cohsh::{Session, Transport};

use crate::claim;
use crate::claim::TicketKey;
use crate::wal::{RelayWal, RelayWalState};
use crate::{HostFederationPeer, HostTicketManifest, HostTicketResult, HostTicketSpec};

/// Summary counters for one relay pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelaySummary {
    /// Specs evaluated from the source hive ticket stream.
    pub seen: usize,
    /// Specs that matched federation relay policy.
    pub candidates: usize,
    /// Specs skipped because a matching WAL key already exists.
    pub deduped: usize,
    /// Specs forwarded successfully to peer hives in this pass.
    pub forwarded: usize,
    /// Forwarding failures (missing peer, write failure, auth/transport errors).
    pub remote_write_failures: usize,
    /// Specs deferred under backpressure; the source retains their original intent.
    pub backpressure_drops: usize,
    /// Pending WAL queue depth after the pass.
    pub queue_depth: usize,
    /// Exact target terminal records returned durably to the source.
    pub terminal_returned: usize,
}

/// Delivery abstraction for relay forwarding.
pub trait RelaySender {
    /// Forward one serialized federated ticket JSON line to the target hive.
    fn forward(&mut self, peer: &HostFederationPeer, payload: &str, timeout_ms: u32) -> Result<()>;
    /// Observe a unique exact terminal result; accepted writes remain pending.
    fn terminal(
        &mut self,
        _peer: &HostFederationPeer,
        _spec: &HostTicketSpec,
        _timeout_ms: u32,
        _max_line_bytes: u32,
    ) -> Result<Option<HostTicketResult>> {
        Ok(None)
    }
}

/// Default REST-based relay sender.
#[derive(Debug, Default)]
pub struct RestRelaySender;

impl RelaySender for RestRelaySender {
    fn forward(&mut self, peer: &HostFederationPeer, payload: &str, timeout_ms: u32) -> Result<()> {
        let client = relay_client(peer, timeout_ms)?;
        client
            .echo("/host/tickets/spec", payload)
            .with_context(|| format!("forward ticket to {}", peer.rest_url))?;
        Ok(())
    }

    fn terminal(
        &mut self,
        peer: &HostFederationPeer,
        spec: &HostTicketSpec,
        timeout_ms: u32,
        max_line_bytes: u32,
    ) -> Result<Option<HostTicketResult>> {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(u64::from(timeout_ms));
        let mut lines = Vec::new();
        for path in ["/host/tickets/status", "/host/tickets/deadletter"] {
            let remaining = remaining_ms(deadline)?;
            let received = relay_client(peer, remaining)?.tail(path, 131_072)?;
            ensure!(received.len() <= 1024, "ELIMIT relay terminal observations");
            lines.extend(received);
        }
        let results = claim::parse_result_lines_from(
            &lines,
            &[
                crate::HOST_TICKET_RESULT_V1_SCHEMA.into(),
                crate::HOST_TICKET_RESULT_V2_SCHEMA.into(),
            ],
            max_line_bytes,
        )?;
        select_terminal(spec, &results)
    }
}

fn relay_client(peer: &HostFederationPeer, timeout_ms: u32) -> Result<cohesix_rest::GatewayClient> {
    let registry = cohesix_authority::provider::registry()?;
    let credentials = registry["contract"]["relay_credentials"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["peer"] == peer.name))
        .ok_or_else(|| anyhow!("not_enabled peer delegation credentials"))?;
    let auth_ref = credentials["request_auth_ref"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid peer request-auth reference"))?;
    let expected_ref = if peer.auth_ref.starts_with("env:") {
        peer.auth_ref.clone()
    } else {
        format!("env:{}", peer.auth_ref)
    };
    ensure!(
        auth_ref == expected_ref,
        "peer request-auth reference differs from generated contract"
    );
    let ticket_ref = credentials["delegated_ticket_ref"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid peer delegation reference"))?;
    let auth = cohesix_authority::secret::resolve_reference(auth_ref)?;
    let ticket = cohesix_authority::secret::resolve_reference(ticket_ref)?;
    cohesix_rest::GatewayClient::new(&peer.rest_url)
        .with_request_auth_token(auth)
        .with_delegated_ticket(ticket)
        .with_operation_deadline(std::time::Duration::from_millis(u64::from(timeout_ms)))
}

fn remaining_ms(deadline: std::time::Instant) -> Result<u32> {
    let remaining = deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_millis();
    ensure!(remaining > 0, "relay operation deadline exceeded");
    u32::try_from(remaining).context("relay deadline bound")
}

fn select_terminal(
    spec: &HostTicketSpec,
    results: &[HostTicketResult],
) -> Result<Option<HostTicketResult>> {
    let mut selected = None;
    for result in results.iter().filter(|result| {
        result.id == spec.id
            && result.idempotency_key == spec.idempotency_key
            && matches!(result.state.as_str(), "succeeded" | "failed" | "expired")
    }) {
        ensure!(
            result.action == spec.action
                && result.writer_epoch == spec.writer_epoch
                && result.admission == spec.admission
                && result.source_hive == spec.source_hive
                && result.target_hive == spec.target_hive
                && result.relay_hop == spec.relay_hop
                && result.relay_correlation_id == spec.relay_correlation_id,
            "EPERM relay terminal correlation mismatch"
        );
        ensure!(
            selected.as_ref().is_none_or(|previous| previous == result),
            "EPERM conflicting relay terminal"
        );
        selected = Some(result.clone());
    }
    Ok(selected)
}

/// Relay one deterministic pass using REST target forwarding.
pub fn relay_once(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    wal_path: &Path,
) -> Result<RelaySummary> {
    let mut sender = RestRelaySender;
    relay_once_with_sender(transport, session, manifest, wal_path, &mut sender)
}

/// Relay one deterministic pass using a caller-provided sender (tests/hooks).
pub fn relay_once_with_sender<S: RelaySender>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    wal_path: &Path,
    sender: &mut S,
) -> Result<RelaySummary> {
    let mut summary = RelaySummary::default();
    if !manifest.enabled || !manifest.federation.enabled {
        return Ok(summary);
    }

    let _fence = crate::wal::AgentFence::acquire(&wal_path.with_extension("lock"))?;
    let spec_path = manifest.spec_path();
    let status_path = manifest.status_path();
    let deadletter_path = manifest.deadletter_path();

    let spec_lines = transport
        .read(session, spec_path.as_str())
        .with_context(|| format!("read {}", spec_path))?;
    let status_lines = transport
        .read(session, status_path.as_str())
        .with_context(|| format!("read {}", status_path))?;
    let deadletter_lines = transport
        .read(session, deadletter_path.as_str())
        .with_context(|| format!("read {}", deadletter_path))?;

    let specs = claim::parse_spec_lines_from(
        &spec_lines,
        &manifest.accepted_request_schemas,
        manifest.max_line_bytes,
        claim::SpecSource::RawRequest,
    )?;
    let mut results = claim::parse_result_lines_from(
        &status_lines,
        &manifest.accepted_result_schemas,
        manifest.max_line_bytes,
    )?;
    let mut deadletters = claim::parse_result_lines_from(
        &deadletter_lines,
        &manifest.accepted_result_schemas,
        manifest.max_line_bytes,
    )?;
    results.append(&mut deadletters);
    let mut terminal = claim::terminal_keys(&results);

    let mut wal = RelayWal::load(wal_path)?;
    wal.bind_writer_epoch(wal_path, manifest.authority.writer_epoch)?;
    let peers = manifest
        .federation
        .peers
        .iter()
        .map(|peer| (peer.name.as_str(), peer))
        .collect::<BTreeMap<_, _>>();
    let relay_allowlist = manifest
        .federation
        .action_allowlist
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();

    let mut pending_bytes = wal
        .pending_entries()
        .iter()
        .map(|entry| entry.payload.len().saturating_add(1))
        .sum::<usize>();

    for spec in specs {
        summary.seen = summary.seen.saturating_add(1);
        let Some(target_hive) = spec.target_hive.as_deref().map(str::to_owned) else {
            continue;
        };
        let source_hive = spec
            .source_hive
            .as_deref()
            .unwrap_or(manifest.federation.local_hive.as_str())
            .to_owned();
        if source_hive != manifest.federation.local_hive {
            // This line was already forwarded from another hive. Never relay again.
            continue;
        }
        if target_hive == manifest.federation.local_hive {
            continue;
        }
        if !relay_allowlist.contains(spec.action.as_str()) {
            continue;
        }
        summary.candidates = summary.candidates.saturating_add(1);

        let key = federated_key(&spec, source_hive.as_str(), target_hive.as_str());
        if wal.contains_key(key.as_str())
            || terminal.contains(&TicketKey::new(&spec.id, &spec.idempotency_key))
        {
            summary.deduped = summary.deduped.saturating_add(1);
            continue;
        }

        if let Err(error) = crate::validate_writer_epoch(manifest, &spec) {
            crate::append_result(
                transport,
                session,
                manifest,
                &spec,
                "failed",
                Some(&error.to_string()),
                &manifest.deadletter_path(),
            )?;
            terminal.insert(TicketKey::new(&spec.id, &spec.idempotency_key));
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            continue;
        }
        if !peers.contains_key(target_hive.as_str()) {
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            continue;
        }

        let payload = build_relay_payload(
            spec,
            source_hive.as_str(),
            target_hive.as_str(),
            key.as_str(),
        )?;
        let payload_bytes = payload.len().saturating_add(1);
        let recovery_reservation = wal
            .pending_entries()
            .iter()
            .filter(|entry| entry.terminal_result.is_none())
            .count()
            .saturating_add(1)
            .saturating_mul(
                (manifest.max_line_bytes as usize)
                    .saturating_mul(2)
                    .saturating_add(128),
            );
        if wal.pending_count() >= manifest.federation.relay_queue_max_entries as usize
            || pending_bytes.saturating_add(payload_bytes)
                > manifest.federation.relay_queue_max_bytes as usize
            || wal
                .serialized_len()
                .saturating_add(payload_bytes.saturating_mul(2))
                .saturating_add(recovery_reservation)
                > manifest.federation.wal_max_bytes as usize
        {
            summary.backpressure_drops = summary.backpressure_drops.saturating_add(1);
            continue;
        }

        wal.upsert_pending(key.as_str(), target_hive.as_str(), payload.as_str());
        pending_bytes = pending_bytes.saturating_add(payload_bytes);
    }

    // Persist pending delivery identity before contacting a remote authority.
    wal.enforce_limits(
        manifest.federation.wal_max_entries as usize,
        manifest.federation.wal_max_bytes as usize,
    )?;
    wal.save(wal_path)?;
    for mut entry in wal.pending_entries() {
        let spec: HostTicketSpec =
            serde_json::from_str(&entry.payload).context("decode pending relay authority")?;
        if let Err(error) = crate::validate_writer_epoch(manifest, &spec) {
            wal.mark_rejected(&entry.key, &error.to_string());
            wal.save(wal_path)?;
            crate::append_result(
                transport,
                session,
                manifest,
                &spec,
                "failed",
                Some(&error.to_string()),
                &manifest.deadletter_path(),
            )?;
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            continue;
        }
        let Some(peer) = peers.get(entry.target_hive.as_str()).copied() else {
            wal.mark_failed(entry.key.as_str(), "missing peer in federation inventory");
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            continue;
        };
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(u64::from(manifest.federation.relay_timeout_ms));
        let result: Result<()> = (|| {
            if entry.state == RelayWalState::Pending {
                sender.forward(peer, &entry.payload, remaining_ms(deadline)?)?;
                wal.mark_forwarded(&entry.key);
                wal.save(wal_path)?;
                entry.state = RelayWalState::AwaitingTerminal;
                summary.forwarded = summary.forwarded.saturating_add(1);
            }
            if entry.state == RelayWalState::AwaitingTerminal {
                let Some(result) = sender.terminal(
                    peer,
                    &spec,
                    remaining_ms(deadline)?,
                    manifest.max_line_bytes,
                )?
                else {
                    return Ok(());
                };
                // Custom transports implement the same boundary as REST.
                select_terminal(&spec, std::slice::from_ref(&result))?
                    .ok_or_else(|| anyhow!("relay result is not terminal"))?;
                let line = serde_json::to_string(&result)?;
                ensure!(
                    line.len() <= manifest.max_line_bytes as usize,
                    "ELIMIT relay terminal line"
                );
                claim::parse_result_lines(
                    std::slice::from_ref(&line),
                    &manifest.result_schema,
                    manifest.max_line_bytes,
                )?;
                wal.retain_terminal(&entry.key, &line)?;
                wal.enforce_limits(
                    manifest.federation.wal_max_entries as usize,
                    manifest.federation.wal_max_bytes as usize,
                )?;
                wal.save(wal_path)?;
                entry.terminal_result = Some(line);
                entry.state = RelayWalState::TerminalRetained;
            }
            if entry.state == RelayWalState::TerminalRetained {
                let line = entry
                    .terminal_result
                    .as_deref()
                    .ok_or_else(|| anyhow!("missing retained relay terminal"))?;
                let target: HostTicketResult = serde_json::from_str(line)?;
                select_terminal(&spec, std::slice::from_ref(&target))?
                    .ok_or_else(|| anyhow!("retained relay result is not terminal"))?;
                let existing = select_terminal(&spec, &results)?;
                if let Some(existing) = existing {
                    ensure!(
                        existing == target,
                        "EPERM source terminal conflicts with target"
                    );
                } else {
                    let path = if target.state == "succeeded" {
                        &status_path
                    } else {
                        &deadletter_path
                    };
                    crate::status::append_result_line(transport, session, path, line)?;
                    results.push(target);
                }
                wal.mark_delivered(&entry.key)?;
                wal.save(wal_path)?;
                summary.terminal_returned = summary.terminal_returned.saturating_add(1);
            }
            Ok(())
        })();
        if result.is_err() {
            // Credential values, foreign response bodies and payloads stay out of WAL errors.
            wal.mark_failed(
                &entry.key,
                "relay dispatch, observation or terminal return unavailable",
            );
            wal.save(wal_path)?;
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
        }
    }

    wal.enforce_limits(
        manifest.federation.wal_max_entries as usize,
        manifest.federation.wal_max_bytes as usize,
    )?;
    wal.save(wal_path)?;
    summary.queue_depth = wal.pending_count();
    Ok(summary)
}

fn federated_key(spec: &HostTicketSpec, source_hive: &str, target_hive: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        spec.id, spec.idempotency_key, source_hive, target_hive
    )
}

fn build_relay_payload(
    mut spec: HostTicketSpec,
    source_hive: &str,
    target_hive: &str,
    correlation_id: &str,
) -> Result<String> {
    spec.source_hive = Some(source_hive.to_owned());
    spec.target_hive = Some(target_hive.to_owned());
    let next_hop = spec.relay_hop.unwrap_or(0).saturating_add(1);
    spec.relay_hop = Some(next_hop);
    spec.relay_correlation_id = Some(correlation_id.to_owned());
    serde_json::to_string(&spec).context("serialize relay payload")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[cfg(test)]
    use cohesix_ticket::Role;

    #[derive(Debug, Default)]
    struct FakeRelaySender {
        fail_once: bool,
        calls: Vec<(String, String)>,
        pending_terminal: bool,
    }

    impl RelaySender for FakeRelaySender {
        fn forward(
            &mut self,
            peer: &HostFederationPeer,
            payload: &str,
            _timeout_ms: u32,
        ) -> Result<()> {
            self.calls.push((peer.name.clone(), payload.to_owned()));
            if self.fail_once {
                self.fail_once = false;
                return Err(anyhow::anyhow!("simulated remote failure"));
            }
            Ok(())
        }

        fn terminal(
            &mut self,
            _peer: &HostFederationPeer,
            spec: &HostTicketSpec,
            _timeout_ms: u32,
            _max_line_bytes: u32,
        ) -> Result<Option<HostTicketResult>> {
            if self.pending_terminal {
                return Ok(None);
            }
            let value = serde_json::json!({"schema":"host-ticket-result/v1", "id":spec.id,
                "idempotency_key":spec.idempotency_key,"action":spec.action,"state":"succeeded",
                "writer_epoch":spec.writer_epoch,"source_hive":spec.source_hive,"target_hive":spec.target_hive,
                "relay_hop":spec.relay_hop,"relay_correlation_id":spec.relay_correlation_id});
            Ok(Some(serde_json::from_value(value)?))
        }
    }

    #[derive(Debug)]
    struct FakeTransport {
        files: BTreeMap<String, Vec<String>>,
    }

    impl Transport for FakeTransport {
        fn attach(&mut self, _role: Role, _ticket: Option<&str>) -> Result<Session> {
            Ok(Session::new(1.into(), Role::Queen))
        }

        fn ping(&mut self, _session: &Session) -> Result<String> {
            Ok("pong".to_owned())
        }

        fn tail(
            &mut self,
            session: &Session,
            path: &str,
            _lines: Option<u16>,
        ) -> Result<Vec<String>> {
            self.read(session, path)
        }

        fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
            if self.files.remove("fail-next-write").is_some() {
                return Err(anyhow!("injected source disconnect"));
            }
            let lose_ack = self.files.remove("lose-next-ack").is_some();
            let text = std::str::from_utf8(payload).context("fake payload utf8")?;
            let entry = self.files.entry(path.to_owned()).or_default();
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    entry.push(trimmed.to_owned());
                }
            }
            if lose_ack {
                return Err(anyhow!("injected lost source acknowledgement"));
            }
            Ok(())
        }
    }

    fn sample_manifest() -> HostTicketManifest {
        HostTicketManifest {
            enabled: true,
            mount_path: "/host".to_owned(),
            request_schema: "host-ticket/v1".to_owned(),
            result_schema: "host-ticket-result/v1".to_owned(),
            max_line_bytes: 2048,
            action_allowlist: vec!["systemd.restart".to_owned()],
            lifecycle: vec![
                "queued".to_owned(),
                "claimed".to_owned(),
                "running".to_owned(),
                "succeeded".to_owned(),
                "failed".to_owned(),
                "expired".to_owned(),
            ],
            federation: crate::HostFederationManifest {
                enabled: true,
                local_hive: "hive-a".to_owned(),
                peers: vec![HostFederationPeer {
                    name: "hive-b".to_owned(),
                    rest_url: "http://127.0.0.1:8081".to_owned(),
                    auth_ref: "COHESIX_RELAY_HIVE_B_TOKEN".to_owned(),
                }],
                action_allowlist: vec!["systemd.restart".to_owned()],
                relay_queue_max_entries: 64,
                relay_queue_max_bytes: 16 * 1024,
                wal_max_entries: 256,
                wal_max_bytes: 256 * 1024,
                relay_timeout_ms: 1500,
            },
            ..HostTicketManifest::default()
        }
    }

    #[test]
    fn relay_rejects_stale_epoch_without_remote_dispatch_and_persists_owner_floor() {
        let temp = tempfile::TempDir::new().expect("temp");
        let wal = temp.path().join("relay.json");
        let mut manifest = sample_manifest();
        manifest.authority.writer_epoch = 3;
        manifest.authority.writer_epoch_required = true;
        let payload = r#"{"schema":"host-ticket/v1","id":"old","idempotency_key":"once","writer_epoch":2,"action":"systemd.restart","args":{"unit":"cohesix.service"},"source_hive":"hive-a","target_hive":"hive-b"}"#;
        let files = BTreeMap::from([
            (manifest.spec_path(), vec![payload.into()]),
            (manifest.status_path(), vec![]),
            (manifest.deadletter_path(), vec![]),
        ]);
        let mut transport = FakeTransport { files };
        let mut sender = FakeRelaySender::default();
        let session = Session::new(1.into(), Role::Queen);
        let summary =
            relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
                .expect("stale record refused");
        assert_eq!(summary.remote_write_failures, 1);
        assert!(sender.calls.is_empty());
        assert!(transport.files[&manifest.deadletter_path()][0].contains("stale-writer"));
        let replay = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .expect("repeated stale record reuses terminal refusal");
        assert_eq!(replay.deduped, 1);
        assert_eq!(transport.files[&manifest.deadletter_path()].len(), 1);
        manifest.authority.writer_epoch = 2;
        let error = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .expect_err("configuration rollback refused");
        assert!(error.to_string().contains("stale-writer-configuration"));
        assert!(sender.calls.is_empty());
    }

    #[test]
    fn relay_pass_is_deduplicated_by_wal() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let wal = temp.path().join("relay.json");
        let manifest = sample_manifest();

        let mut files = BTreeMap::<String, Vec<String>>::new();
        files.insert(
            manifest.spec_path(),
            vec!["{\"schema\":\"host-ticket/v1\",\"id\":\"ticket-1\",\"idempotency_key\":\"idem-1\",\"action\":\"systemd.restart\",\"target\":\"/host/systemd/cohesix-agent.service/restart\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\"}".to_owned()],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());

        let mut transport = FakeTransport { files };
        let session = Session::new(1.into(), Role::Queen);
        let mut sender = FakeRelaySender::default();

        let first = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .unwrap_or_else(|err| unreachable!("first relay pass: {err}"));
        assert_eq!(first.forwarded, 1);
        assert_eq!(first.queue_depth, 0);
        assert_eq!(sender.calls.len(), 1);

        let second = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .unwrap_or_else(|err| unreachable!("second relay pass: {err}"));
        assert_eq!(second.forwarded, 0);
        assert_eq!(second.deduped, 1);
        assert_eq!(sender.calls.len(), 1);
    }

    #[test]
    fn relay_resumes_failed_wal_entries() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let wal = temp.path().join("relay.json");
        let manifest = sample_manifest();

        let mut files = BTreeMap::<String, Vec<String>>::new();
        files.insert(
            manifest.spec_path(),
            vec!["{\"schema\":\"host-ticket/v1\",\"id\":\"ticket-2\",\"idempotency_key\":\"idem-2\",\"action\":\"systemd.restart\",\"target\":\"/host/systemd/cohesix-agent.service/restart\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\"}".to_owned()],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());

        let mut transport = FakeTransport { files };
        let session = Session::new(1.into(), Role::Queen);
        let mut sender = FakeRelaySender {
            fail_once: true,
            calls: Vec::new(),
            ..FakeRelaySender::default()
        };

        let first = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .unwrap_or_else(|err| unreachable!("first pass: {err}"));
        assert_eq!(first.forwarded, 0);
        assert_eq!(first.remote_write_failures, 1);
        assert_eq!(first.queue_depth, 1);

        let second = relay_once_with_sender(&mut transport, &session, &manifest, &wal, &mut sender)
            .unwrap_or_else(|err| unreachable!("second pass: {err}"));
        assert_eq!(second.forwarded, 1);
        assert_eq!(second.queue_depth, 0);
    }

    #[test]
    fn relay_payload_omits_null_optional_fields() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "fed-ticket-1".to_owned(),
            idempotency_key: "idem-1".to_owned(),
            action: "systemd.stop".to_owned(),
            target: None,
            args: serde_json::Value::Null,
            expires_unix_ms: None,
            source_hive: Some("hive-a".to_owned()),
            target_hive: Some("hive-b".to_owned()),
            relay_hop: Some(1),
            relay_correlation_id: Some("fed-ticket-1:idem-1:hive-a:hive-b".to_owned()),
            ..HostTicketSpec::default()
        };

        let payload = build_relay_payload(
            spec,
            "hive-a",
            "hive-b",
            "fed-ticket-1:idem-1:hive-a:hive-b",
        )
        .unwrap_or_else(|err| unreachable!("payload build: {err}"));

        assert!(!payload.contains("\"target\":null"));
        assert!(!payload.contains("\"args\":null"));
        assert!(!payload.contains("\"expires_unix_ms\":null"));
        assert!(payload.len() <= 224);
    }

    #[test]
    fn relay_ack_and_source_disconnect_recover_without_replaying_execution() {
        let temp = tempfile::tempdir().expect("tempdir");
        let wal = temp.path().join("relay.json");
        let manifest = sample_manifest();
        let request = r#"{"schema":"host-ticket/v1","id":"recover","idempotency_key":"once","action":"systemd.restart","args":{"unit":"cohesix.service"},"source_hive":"hive-a","target_hive":"hive-b"}"#;
        let local = r#"{"schema":"host-ticket/v2","id":"local","idempotency_key":"local","action":"peft.export","args":{},"receipt_mode":"worker","operation_id":"export-1","subject_ref":"job-1","receipt_worker_role":"worker-lora","receipt_worker_id":"lora-worker-1","receipt_supervisor_generation":1,"receipt_cap_generation":1}"#;
        let mut source = FakeTransport {
            files: BTreeMap::from([(manifest.spec_path(), vec![request.into(), local.into()])]),
        };
        let session = Session::new(1.into(), Role::Queen);
        let mut sender = FakeRelaySender {
            pending_terminal: true,
            ..FakeRelaySender::default()
        };
        let first = relay_once_with_sender(&mut source, &session, &manifest, &wal, &mut sender)
            .expect("ACK only");
        assert_eq!(
            (first.forwarded, first.terminal_returned, first.queue_depth),
            (1, 0, 1)
        );
        assert_eq!(
            RelayWal::load(&wal)
                .expect("durable wait")
                .pending_entries()[0]
                .state,
            RelayWalState::AwaitingTerminal
        );
        sender.pending_terminal = false;
        source.files.insert("fail-next-write".into(), vec![]);
        let failed = relay_once_with_sender(&mut source, &session, &manifest, &wal, &mut sender)
            .expect("retain terminal across disconnect");
        assert_eq!((failed.terminal_returned, failed.queue_depth), (0, 1));
        assert_eq!(
            RelayWal::load(&wal)
                .expect("durable terminal")
                .pending_entries()[0]
                .state,
            RelayWalState::TerminalRetained
        );
        source.files.insert("lose-next-ack".into(), vec![]);
        let lost_ack = relay_once_with_sender(&mut source, &session, &manifest, &wal, &mut sender)
            .expect("lost source ACK");
        assert_eq!((lost_ack.terminal_returned, lost_ack.queue_depth), (0, 1));
        assert_eq!(source.files[&manifest.status_path()].len(), 1);
        let recovered = relay_once_with_sender(&mut source, &session, &manifest, &wal, &mut sender)
            .expect("restart return");
        assert_eq!((recovered.terminal_returned, recovered.queue_depth), (1, 0));
        assert_eq!(sender.calls.len(), 1);
        assert_eq!(source.files[&manifest.status_path()].len(), 1);
        let repeated = relay_once_with_sender(&mut source, &session, &manifest, &wal, &mut sender)
            .expect("duplicate");
        assert_eq!(repeated.terminal_returned, 0);
        assert_eq!(source.files[&manifest.status_path()].len(), 1);
        let mut wrong: HostTicketResult =
            serde_json::from_str(&source.files[&manifest.status_path()][0]).expect("result");
        let forwarded: HostTicketSpec =
            serde_json::from_str(&sender.calls[0].1).expect("forwarded identity");
        wrong.target_hive = Some("different-hive".into());
        assert!(select_terminal(&forwarded, &[wrong]).is_err());
    }
}
