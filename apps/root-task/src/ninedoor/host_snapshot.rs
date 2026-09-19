// Author: Lukas Bower
// Purpose: Receive source-scoped host observations through existing authenticated namespace operations and withdraw expired bytes.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

extern crate alloc;

use super::{
    base64_encoded_len, generated, parse_gpu_bridge_begin, sha256_bytes, GpuBridgePending,
    NineDoorBridgeError, BASE64_STANDARD,
};
use alloc::{format, string::String, vec::Vec};
use base64::Engine;
use cohesix_authority::snapshot::{Limits, Replacement, Slot};

#[derive(Debug)]
struct Member {
    provider: &'static str,
    source_id: &'static str,
    providers: &'static [&'static str],
    slot: Slot,
    body: String,
    status: String,
    pending: Option<GpuBridgePending>,
    pending_deadline: u64,
    received_at_ms: u64,
}

/// At most sixteen generated (provider, source) pairs. Sources never overwrite
/// one another, and each upload and retained record has an absolute byte cap.
#[derive(Debug)]
pub(super) struct NativeSnapshots {
    mount: String,
    config: generated::HostSnapshotConfig,
    members: Vec<Member>,
}

impl NativeSnapshots {
    pub(super) fn new(mount: &str) -> Self {
        Self::with_config(mount, generated::host_config().snapshots)
    }

    fn with_config(mount: &str, config: generated::HostSnapshotConfig) -> Self {
        let mut members = Vec::new();
        if config.enable {
            for publisher in config.publishers {
                for provider in publisher.providers {
                    members.push(Member {
                        provider, source_id: publisher.source_id, providers: publisher.providers,
                        slot: Slot::default(), body: String::new(),
                        status: format!("schema=host-snapshot-status/v1 state=unavailable source={} provider={} sequence=0 reason=unobserved", publisher.source_id, provider),
                        pending: None, pending_deadline: 0, received_at_ms: 0,
                    });
                }
            }
        }
        Self {
            mount: format!("{mount}/snapshots"),
            config,
            members,
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.config.enable
    }

    fn member_index(&self, path: &str, leaf: &str) -> Option<usize> {
        let suffix = path.strip_prefix(&self.mount)?.strip_prefix('/')?;
        let mut parts = suffix.split('/');
        let provider = parts.next()?;
        let source = parts.next()?;
        if parts.next()? != leaf || parts.next().is_some() {
            return None;
        }
        self.members
            .iter()
            .position(|member| member.provider == provider && member.source_id == source)
    }

    pub(super) fn is_control(&self, path: &str) -> bool {
        self.member_index(path, "ctl").is_some()
    }

    /// Source and provider directories come exclusively from generated enrollment.
    pub(super) fn list(&self, path: &str) -> Option<Vec<&'static str>> {
        if !self.enabled() {
            return None;
        }
        if path == self.mount {
            let mut providers: Vec<_> = self.members.iter().map(|member| member.provider).collect();
            providers.sort_unstable();
            providers.dedup();
            return Some(providers);
        }
        let suffix = path.strip_prefix(&self.mount)?.strip_prefix('/')?;
        let parts: Vec<_> = suffix.split('/').collect();
        match parts.as_slice() {
            [provider] => {
                let mut sources: Vec<_> = self
                    .members
                    .iter()
                    .filter(|member| member.provider == *provider)
                    .map(|member| member.source_id)
                    .collect();
                if sources.is_empty() {
                    return None;
                }
                sources.sort_unstable();
                Some(sources)
            }
            [provider, source]
                if self
                    .members
                    .iter()
                    .any(|member| member.provider == *provider && member.source_id == *source) =>
            {
                Some(alloc::vec!["ctl", "snapshot", "status"])
            }
            _ => None,
        }
    }

    /// A missing or expired snapshot exposes no native data. The status leaf
    /// retains the source and last sequence so an empty read is explainable.
    pub(super) fn read(&self, path: &str, now_ms: u64) -> Option<&str> {
        if let Some(index) = self.member_index(path, "status") {
            return Some(&self.members[index].status);
        }
        let index = self.member_index(path, "snapshot")?;
        let member = &self.members[index];
        Some(if member.slot.snapshot(now_ms).is_some() {
            &member.body
        } else {
            ""
        })
    }

    fn limits(config: generated::HostSnapshotConfig, member: &Member) -> Limits<'static> {
        Limits {
            source_id: member.source_id,
            epoch: generated::AUTHORITY_POLICY.writer_epoch,
            providers: member.providers,
            max_bytes: config.max_bytes as usize,
            max_entries: config.max_entries as usize,
            max_value_bytes: config.max_value_bytes as usize,
            max_ttl_ms: u64::from(config.max_ttl_ms),
        }
    }

    /// Caller has already enforced Queen/session scope, lifecycle and policy.
    /// begin/b64/end reuse the bounded namespace upload shape; only end can
    /// replace a snapshot, after digest, source, epoch and replay validation.
    pub(super) fn append(
        &mut self,
        path: &str,
        payload: &str,
        now_ms: u64,
    ) -> Result<(), NineDoorBridgeError> {
        let index = self
            .member_index(path, "ctl")
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        if payload.contains(['\n', '\r']) {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let member = &mut self.members[index];
        let limits = Self::limits(self.config, member);
        limits
            .validate()
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        if let Some(rest) = payload.strip_prefix("begin ") {
            let (expected_bytes, expected_sha256) = parse_gpu_bridge_begin(rest)?;
            if expected_bytes == 0 || expected_bytes > limits.max_bytes {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            let deadline = now_ms
                .checked_add(u64::from(self.config.max_ttl_ms))
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            member.pending = Some(GpuBridgePending {
                expected_bytes,
                expected_sha256,
                encoded: Vec::new(),
            });
            member.pending_deadline = deadline;
            member.received_at_ms = now_ms;
            return Ok(());
        }
        if let Some(rest) = payload.strip_prefix("b64:") {
            let pending = member
                .pending
                .as_mut()
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            let maximum = base64_encoded_len(pending.expected_bytes)
                .ok_or(NineDoorBridgeError::InvalidPayload)?;
            if rest.is_empty()
                || now_ms >= member.pending_deadline
                || pending.encoded.len().saturating_add(rest.len()) > maximum
            {
                return Err(NineDoorBridgeError::InvalidPayload);
            }
            pending.encoded.extend_from_slice(rest.as_bytes());
            return Ok(());
        }
        if payload != "end" {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let pending = member
            .pending
            .take()
            .ok_or(NineDoorBridgeError::InvalidPayload)?;
        if now_ms >= member.pending_deadline {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let bytes = BASE64_STANDARD
            .decode(&pending.encoded)
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        if bytes.len() != pending.expected_bytes || sha256_bytes(&bytes) != pending.expected_sha256
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        let body = String::from_utf8(bytes).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        let result = member
            .slot
            .apply(
                body.as_bytes(),
                limits,
                member.provider,
                member.received_at_ms,
                now_ms,
            )
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        // An exact retry acknowledges the original commit without renewing its TTL.
        if result == Replacement::Applied {
            member.body = body;
        }
        if let Some(snapshot) = member.slot.snapshot(now_ms) {
            member.status = format!("schema=host-snapshot-status/v1 state={} source={} provider={} epoch={} sequence={} observed_unix_ms={} ttl_ms={} sha256={}",
                if snapshot.available { "available" } else { "unavailable" }, member.source_id, member.provider,
                snapshot.epoch, snapshot.sequence, snapshot.observed_unix_ms, snapshot.ttl_ms,
                hex::encode(pending.expected_sha256));
        } else {
            member.body.clear();
            Self::unavailable(member, "expired");
        }
        Ok(())
    }

    fn unavailable(member: &mut Member, reason: &str) {
        member.status = format!("schema=host-snapshot-status/v1 state=unavailable source={} provider={} sequence={} reason={reason}",
            member.source_id, member.provider, member.slot.sequence());
    }

    pub(super) fn withdraw_expired(&mut self, now_ms: u64) {
        for member in &mut self.members {
            if member.pending.is_some() && now_ms >= member.pending_deadline {
                member.pending = None;
            }
            match member.slot.withdraw_expired(now_ms) {
                Ok(false) => {}
                Ok(true) => {
                    member.body.clear();
                    Self::unavailable(member, "expired");
                }
                Err(_) => {
                    member.body.clear();
                    member.pending = None;
                    Self::unavailable(member, "clock_regressed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::snapshot::{Entry, Snapshot, UnavailableReason, SCHEMA};

    fn fixture() -> NativeSnapshots {
        NativeSnapshots::with_config(
            "/host",
            generated::HostSnapshotConfig {
                enable: true,
                max_bytes: 8192,
                max_entries: 16,
                max_value_bytes: 512,
                max_ttl_ms: 5000,
                publishers: &[
                    generated::HostSnapshotPublisher {
                        source_id: "linux-reference",
                        providers: &["network", "systemd"],
                    },
                    generated::HostSnapshotPublisher {
                        source_id: "mac-controller",
                        providers: &["network"],
                    },
                ],
            },
        )
    }

    fn upload(
        state: &mut NativeSnapshots,
        source: &str,
        sequence: u64,
        now: u64,
        available: bool,
    ) -> String {
        let snapshot = Snapshot {
            schema: SCHEMA.into(),
            provider: "network".into(),
            source_id: source.into(),
            epoch: generated::AUTHORITY_POLICY.writer_epoch,
            sequence,
            observed_unix_ms: 100_000 + sequence,
            ttl_ms: 100,
            available,
            reason: (!available).then_some(UnavailableReason::SourceFailed),
            entries: if available {
                alloc::vec![Entry {
                    path: "interfaces/en0".into(),
                    value: "link=up".into()
                }]
            } else {
                Vec::new()
            },
        };
        let index = state
            .members
            .iter()
            .position(|member| member.source_id == source && member.provider == "network")
            .unwrap();
        let bytes = snapshot
            .encode(NativeSnapshots::limits(state.config, &state.members[index]))
            .unwrap();
        let path = format!("/host/snapshots/network/{source}/ctl");
        state
            .append(
                &path,
                &format!(
                    "begin bytes={} sha256={}",
                    bytes.len(),
                    hex::encode(sha256_bytes(&bytes))
                ),
                now,
            )
            .unwrap();
        let encoded = BASE64_STANDARD.encode(&bytes);
        for chunk in encoded.as_bytes().chunks(80) {
            state
                .append(
                    &path,
                    &format!("b64:{}", core::str::from_utf8(chunk).unwrap()),
                    now,
                )
                .unwrap();
        }
        state.append(&path, "end", now).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn host_snapshot_upload_namespace_source_isolation_and_withdrawal() {
        let mut state = fixture();
        assert_eq!(
            state.list("/host/snapshots").unwrap(),
            alloc::vec!["network", "systemd"]
        );
        assert_eq!(
            state.list("/host/snapshots/network").unwrap(),
            alloc::vec!["linux-reference", "mac-controller"]
        );
        assert!(!state.is_control("/host/snapshots/network/unregistered/ctl"));
        let linux = upload(&mut state, "linux-reference", 1, 10, true);
        let mac = upload(&mut state, "mac-controller", 1, 20, true);
        let linux_path = "/host/snapshots/network/linux-reference/snapshot";
        let mac_path = "/host/snapshots/network/mac-controller/snapshot";
        assert_eq!(state.read(linux_path, 20).unwrap(), linux);
        assert_eq!(state.read(mac_path, 20).unwrap(), mac);
        // Digest failure cannot replace a previously valid observation.
        let ctl = "/host/snapshots/network/linux-reference/ctl";
        state
            .append(ctl, &format!("begin bytes=2 sha256={}", "0".repeat(64)), 21)
            .unwrap();
        state.append(ctl, "b64:e30=", 21).unwrap();
        assert!(state.append(ctl, "end", 21).is_err());
        assert_eq!(state.read(linux_path, 21).unwrap(), linux);
        state.withdraw_expired(110);
        assert_eq!(state.read(linux_path, 110).unwrap(), "");
        assert_eq!(state.read(mac_path, 110).unwrap(), mac);
        assert!(state
            .read("/host/snapshots/network/linux-reference/status", 110)
            .unwrap()
            .contains("reason=expired"));
        // An ambiguous retry at a later time does not refresh the original TTL.
        upload(&mut state, "mac-controller", 1, 115, true);
        assert_eq!(state.read(mac_path, 120).unwrap(), "");
        let failed = upload(&mut state, "linux-reference", 2, 120, false);
        assert_eq!(state.read(linux_path, 120).unwrap(), failed);
        assert!(state
            .read("/host/snapshots/network/linux-reference/status", 120)
            .unwrap()
            .contains("state=unavailable"));
    }
}
