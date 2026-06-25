// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Relay federated host tickets across hives with deterministic WAL-backed delivery.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use cohesix_ticket::Role;
use cohsh::{RestTransport, Session, Transport};

use crate::claim;
use crate::claim::TicketKey;
use crate::wal::RelayWal;
use crate::{HostFederationPeer, HostTicketManifest, HostTicketSpec};

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
    /// Specs dropped due to queue backpressure bounds.
    pub backpressure_drops: usize,
    /// Pending WAL queue depth after the pass.
    pub queue_depth: usize,
}

/// Delivery abstraction for relay forwarding.
pub trait RelaySender {
    /// Forward one serialized federated ticket JSON line to the target hive.
    fn forward(&mut self, peer: &HostFederationPeer, payload: &str, timeout_ms: u32) -> Result<()>;
}

/// Default REST-based relay sender.
#[derive(Debug, Default)]
pub struct RestRelaySender;

impl RelaySender for RestRelaySender {
    fn forward(
        &mut self,
        peer: &HostFederationPeer,
        payload: &str,
        _timeout_ms: u32,
    ) -> Result<()> {
        let auth = std::env::var(peer.auth_ref.as_str())
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let mut transport = RestTransport::new(peer.rest_url.as_str(), auth);
        let session = transport
            .attach(Role::Queen, None)
            .with_context(|| format!("attach relay session for {}", peer.name))?;
        let mut bytes = payload.as_bytes().to_vec();
        bytes.push(b'\n');
        let write = transport
            .write(&session, "/host/tickets/spec", bytes.as_slice())
            .with_context(|| format!("forward ticket to {}", peer.rest_url));
        let _ = transport.quit(&session);
        write
    }
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

    let specs = claim::parse_spec_lines(
        &spec_lines,
        manifest.request_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    let mut results = claim::parse_result_lines(
        &status_lines,
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    let mut deadletters = claim::parse_result_lines(
        &deadletter_lines,
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    results.append(&mut deadletters);
    let terminal = claim::terminal_keys(&results);

    let mut wal = RelayWal::load(wal_path)?;
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
        if wal.pending_count() >= manifest.federation.relay_queue_max_entries as usize
            || pending_bytes.saturating_add(payload_bytes)
                > manifest.federation.relay_queue_max_bytes as usize
        {
            summary.backpressure_drops = summary.backpressure_drops.saturating_add(1);
            continue;
        }

        wal.upsert_pending(key.as_str(), target_hive.as_str(), payload.as_str());
        pending_bytes = pending_bytes.saturating_add(payload_bytes);
    }

    for entry in wal.pending_entries() {
        let Some(peer) = peers.get(entry.target_hive.as_str()).copied() else {
            wal.mark_failed(entry.key.as_str(), "missing peer in federation inventory");
            summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            continue;
        };
        match sender.forward(
            peer,
            entry.payload.as_str(),
            manifest.federation.relay_timeout_ms,
        ) {
            Ok(()) => {
                wal.mark_delivered(entry.key.as_str());
                summary.forwarded = summary.forwarded.saturating_add(1);
            }
            Err(err) => {
                let detail = truncate_text(err.to_string().as_str(), 192);
                wal.mark_failed(entry.key.as_str(), detail.as_str());
                summary.remote_write_failures = summary.remote_write_failures.saturating_add(1);
            }
        }
    }

    wal.enforce_limits(
        manifest.federation.wal_max_entries as usize,
        manifest.federation.wal_max_bytes as usize,
    );
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

fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.len() <= max_chars {
        return input.to_owned();
    }
    input[..max_chars].to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    use cohesix_ticket::Role;

    #[derive(Debug, Default)]
    struct FakeRelaySender {
        fail_once: bool,
        calls: Vec<(String, String)>,
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
            let text = std::str::from_utf8(payload).context("fake payload utf8")?;
            let entry = self.files.entry(path.to_owned()).or_default();
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    entry.push(trimmed.to_owned());
                }
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
        }
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
}
