// CLASSIFICATION: COMMUNITY
// Filename: trace_consensus.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-08-24

use crate::federation::keyring::Keyring;
use crate::trace::recorder;
use crate::trace::validator;
use crate::{coh_bail, new_err, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::tls::{Certificate, RootCerts, TlsConfig};
use ureq::{Agent, Error as UreqError};

use crate::utils::tiny_ed25519::TinyEd25519;
use crate::utils::tiny_rng::TinyRng;

const CONSENSUS_DIR: &str = "/srv/trace/consensus";
const FAULT_DIR: &str = "/srv/trace/fault";
const CURRENT_SNAPSHOT: &str = "/srv/trace/consensus/current.snapshot";

/// Descriptor for a peer Queen endpoint participating in trace consensus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerDescriptor {
    pub id: String,
    pub endpoint: String,
}

impl PeerDescriptor {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            endpoint: endpoint.into(),
        }
    }
}

/// Local trace segment prepared for consensus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSegment {
    pub segment_id: String,
    pub entries: Vec<String>,
}

impl TraceSegment {
    /// Construct a trace segment from a list of entries.
    pub fn new(segment_id: impl Into<String>, entries: Vec<String>) -> Self {
        let cleaned = entries
            .into_iter()
            .map(|line| line.trim_end_matches(['\r', '\n']).to_string())
            .collect();
        Self {
            segment_id: segment_id.into(),
            entries: cleaned,
        }
    }

    /// Load a segment from a log file, one entry per line.
    pub fn from_log_file(segment_id: &str, path: &str) -> Result<Self, CohError> {
        let data = fs::read_to_string(path)?;
        Ok(Self::new(
            segment_id,
            data.lines().map(|line| line.to_string()).collect(),
        ))
    }
}

/// Result of a successful consensus round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsensusResult {
    pub segment_id: String,
    pub merkle_root: String,
    pub participants: Vec<String>,
    pub required_peer_quorum: usize,
    pub achieved_peer_votes: usize,
    pub total_participants: usize,
    pub entries: Vec<String>,
}

/// Signed payload exchanged during consensus.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentEnvelope {
    pub from: String,
    pub segment_id: String,
    pub entries: Vec<String>,
    pub nonce: String,
    pub merkle_root: String,
    pub signature: String,
    pub session_pubkey: String,
    pub session_cert: String,
    pub timestamp: u64,
}

#[derive(Clone)]
struct VerifiedSegment {
    from: String,
    segment_id: String,
    merkle_root: [u8; 32],
    entries: Vec<String>,
    nonce: [u8; 32],
}

/// Builder for [`TraceConsensus`].
pub struct TraceConsensusBuilder {
    local_id: String,
    extra_roots: Vec<Vec<u8>>,
    user_agent: Option<String>,
}

impl TraceConsensusBuilder {
    pub fn new(local_id: &str) -> Self {
        Self {
            local_id: local_id.into(),
            extra_roots: Vec::new(),
            user_agent: None,
        }
    }

    /// Add an additional trusted root certificate in DER form.
    pub fn add_root(mut self, cert_der: &[u8]) -> Self {
        self.extra_roots.push(cert_der.to_vec());
        self
    }

    /// Override the default HTTP user agent string.
    pub fn user_agent(mut self, agent: &str) -> Self {
        self.user_agent = Some(agent.into());
        self
    }

    /// Build the consensus object, initialising TLS and key material.
    pub fn build(self) -> Result<TraceConsensus, CohError> {
        let keyring = Keyring::load_or_generate(&self.local_id)?;
        let agent = build_tls_agent(&self.extra_roots, self.user_agent.as_deref())?;
        Ok(TraceConsensus {
            local_id: self.local_id,
            agent,
            keyring,
        })
    }
}

/// Trace consensus orchestrator for Queen nodes.
pub struct TraceConsensus {
    local_id: String,
    agent: Agent,
    keyring: Keyring,
}

impl TraceConsensus {
    pub fn builder(local_id: &str) -> TraceConsensusBuilder {
        TraceConsensusBuilder::new(local_id)
    }

    pub fn new(local_id: &str) -> Result<Self, CohError> {
        Self::builder(local_id).build()
    }

    /// Prepare a signed segment envelope for exchange with peers.
    pub fn prepare_segment(&self, segment: &TraceSegment) -> Result<SegmentEnvelope, CohError> {
        let merkle_root = compute_merkle_root(&segment.entries);
        let nonce = self.generate_nonce();
        let session_key = self.derive_session_key(&nonce)?;
        let merkle_hex = hex::encode(merkle_root);
        let nonce_hex = hex::encode(nonce);
        let mut cert_payload = Vec::with_capacity(nonce.len() + 32);
        cert_payload.extend_from_slice(&nonce);
        cert_payload.extend_from_slice(&session_key.public_key_bytes());
        let session_cert = self.keyring.sign(&cert_payload);
        let signature =
            session_key.sign(&message_to_sign(&segment.segment_id, &nonce, &merkle_root));
        Ok(SegmentEnvelope {
            from: self.local_id.clone(),
            segment_id: segment.segment_id.clone(),
            entries: segment.entries.clone(),
            nonce: nonce_hex,
            merkle_root: merkle_hex,
            signature: hex::encode(signature),
            session_pubkey: hex::encode(session_key.public_key_bytes()),
            session_cert: hex::encode(session_cert),
            timestamp: now_timestamp(),
        })
    }

    /// Run a consensus round using the provided peers.
    pub fn run(
        &self,
        segment: TraceSegment,
        peers: &[PeerDescriptor],
    ) -> Result<ConsensusResult, CohError> {
        fs::create_dir_all(CONSENSUS_DIR).ok();
        fs::create_dir_all(FAULT_DIR).ok();

        let expected_merkle = compute_merkle_root(&segment.entries);
        let local_envelope = self.prepare_segment(&segment)?;
        let local_vote = self.verify_envelope(&local_envelope)?;
        let mut votes = Vec::with_capacity(peers.len() + 1);
        let mut audit_votes = Vec::with_capacity(peers.len() + 1);
        let mut disqualified: Vec<(String, String)> = Vec::new();

        audit_votes.push(local_vote.clone());
        votes.push(local_vote);

        for peer in peers {
            match self.exchange_with_peer(peer, &local_envelope) {
                Ok(envelope) => match self.verify_envelope(&envelope) {
                    Ok(verified) => {
                        let mut reason = None;
                        if verified.from != peer.id {
                            reason = Some(format!(
                                "peer mismatch: expected {} got {}",
                                peer.id, verified.from
                            ));
                        } else if verified.segment_id != segment.segment_id {
                            reason = Some(format!(
                                "segment id mismatch: expected {} got {}",
                                segment.segment_id, verified.segment_id
                            ));
                        } else if verified.merkle_root != expected_merkle {
                            reason = Some(format!(
                                "merkle root mismatch: expected {} got {}",
                                hex::encode(expected_merkle),
                                hex::encode(verified.merkle_root)
                            ));
                        }
                        audit_votes.push(verified.clone());
                        if let Some(msg) = reason {
                            disqualified.push((peer.id.clone(), msg));
                        } else {
                            votes.push(verified);
                        }
                    }
                    Err(err) => disqualified.push((peer.id.clone(), err.to_string())),
                },
                Err(err) => disqualified.push((peer.id.clone(), err.to_string())),
            }
        }

        let total_participants = peers.len() + 1;
        let required_peer_quorum = if peers.is_empty() {
            0
        } else {
            ((2 * peers.len()) + 2) / 3
        };

        let outcome = select_quorum(&votes, &self.local_id, required_peer_quorum);

        if let Some(group) = outcome {
            let merkle_hex = hex::encode(group.merkle_root);
            let participants = group
                .votes
                .iter()
                .map(|v| v.from.clone())
                .collect::<Vec<_>>();
            let achieved_peer_votes = group.peer_votes;
            self.write_consensus_artifacts(
                &group.segment_id,
                &merkle_hex,
                required_peer_quorum,
                total_participants,
                achieved_peer_votes,
                &participants,
                &group.entries,
                &group.votes,
            )?;
            return Ok(ConsensusResult {
                segment_id: group.segment_id,
                merkle_root: merkle_hex,
                participants,
                required_peer_quorum,
                achieved_peer_votes,
                total_participants,
                entries: group.entries,
            });
        }

        self.write_failure_artifacts(
            &segment.segment_id,
            required_peer_quorum,
            total_participants,
            &audit_votes,
            &disqualified,
        )?;
        coh_bail!("trace consensus quorum not reached");
    }

    fn exchange_with_peer(
        &self,
        peer: &PeerDescriptor,
        payload: &SegmentEnvelope,
    ) -> Result<SegmentEnvelope, CohError> {
        ensure_https(&peer.endpoint)?;
        let body = serde_json::to_string(payload)?;
        let response = self
            .agent
            .post(&peer.endpoint)
            .content_type("application/json")
            .send(body)
            .map_err(|err| match err {
                UreqError::StatusCode(code) => {
                    new_err(format!("peer {} returned HTTP {}", peer.id, code))
                }
                other => new_err(format!("peer {} exchange failed: {other}", peer.id)),
            })?;
        let payload = response
            .into_body()
            .read_to_string()
            .map_err(|e| new_err(format!("failed to read response body: {e}")))?;
        Ok(serde_json::from_str(&payload)?)
    }

    fn verify_envelope(&self, envelope: &SegmentEnvelope) -> Result<VerifiedSegment, CohError> {
        let nonce = decode_hex_array::<32>(&envelope.nonce)?;
        let merkle_root = decode_hex_array::<32>(&envelope.merkle_root)?;
        let signature = hex::decode(&envelope.signature).map_err(|e| {
            new_err(format!(
                "invalid signature encoding for {}: {e}",
                envelope.from
            ))
        })?;
        if signature.len() != 64 {
            coh_bail!("signature length invalid for peer {}", envelope.from);
        }
        let session_pubkey = decode_hex_array::<32>(&envelope.session_pubkey)?;
        let session_cert = hex::decode(&envelope.session_cert).map_err(|e| {
            new_err(format!(
                "invalid session certificate for {}: {e}",
                envelope.from
            ))
        })?;

        let mut cert_payload = Vec::with_capacity(64);
        cert_payload.extend_from_slice(&nonce);
        cert_payload.extend_from_slice(&session_pubkey);
        if !Keyring::verify_peer(&envelope.from, &cert_payload, &session_cert)? {
            coh_bail!(
                "peer {} presented invalid session certificate",
                envelope.from
            );
        }

        let expected_root = compute_merkle_root(&envelope.entries);
        if expected_root != merkle_root {
            coh_bail!("peer {} merkle root mismatch", envelope.from);
        }

        let message = message_to_sign(&envelope.segment_id, &nonce, &merkle_root);
        if !TinyEd25519::verify(&session_pubkey, &message, &signature) {
            coh_bail!("peer {} signature verification failed", envelope.from);
        }

        Ok(VerifiedSegment {
            from: envelope.from.clone(),
            segment_id: envelope.segment_id.clone(),
            merkle_root,
            entries: envelope.entries.clone(),
            nonce,
        })
    }

    fn write_consensus_artifacts(
        &self,
        segment_id: &str,
        merkle_hex: &str,
        required_peer_quorum: usize,
        total_participants: usize,
        achieved_peer_votes: usize,
        participants: &[String],
        entries: &[String],
        votes: &[VerifiedSegment],
    ) -> Result<(), CohError> {
        let ts = now_timestamp();
        fs::create_dir_all(CONSENSUS_DIR).ok();
        let record = ConsensusRecord {
            timestamp: ts,
            segment_id: segment_id.into(),
            merkle_root: merkle_hex.into(),
            participants: participants.to_vec(),
            required_peer_quorum,
            achieved_peer_votes,
            total_participants,
            entries: entries.to_vec(),
            nonces: votes
                .iter()
                .map(|vote| PeerNonce {
                    peer: vote.from.clone(),
                    nonce: hex::encode(vote.nonce),
                })
                .collect(),
        };
        let log_path = format!("{CONSENSUS_DIR}/{ts}.log");
        fs::write(&log_path, serde_json::to_vec_pretty(&record)?)?;
        fs::write(CURRENT_SNAPSHOT, serde_json::to_vec_pretty(&record)?)?;
        log_event(&format!(
            "consensus segment={} peers={}/{}",
            segment_id, achieved_peer_votes, required_peer_quorum
        ));
        recorder::event(
            &self.local_id,
            "trace_consensus",
            &format!("segment {} merkle {}", segment_id, merkle_hex),
        );
        Ok(())
    }

    fn write_failure_artifacts(
        &self,
        segment_id: &str,
        required_peer_quorum: usize,
        total_participants: usize,
        votes: &[VerifiedSegment],
        disqualified: &[(String, String)],
    ) -> Result<(), CohError> {
        let ts = now_timestamp();
        fs::create_dir_all(FAULT_DIR).ok();
        let policy_hash = validator::security_policy_digest().ok();
        let record = FaultRecord {
            timestamp: ts,
            segment_id: segment_id.into(),
            reason: "quorum not reached".into(),
            required_peer_quorum,
            total_participants,
            policy_hash,
            votes: votes
                .iter()
                .map(|v| FaultVote {
                    peer: v.from.clone(),
                    segment_id: v.segment_id.clone(),
                    merkle_root: hex::encode(v.merkle_root),
                    nonce: hex::encode(v.nonce),
                })
                .collect(),
            disqualified: disqualified
                .iter()
                .map(|(peer, error)| FaultPeer {
                    peer: peer.clone(),
                    error: error.clone(),
                })
                .collect(),
        };
        let fault_path = format!("{FAULT_DIR}/{ts}.error");
        fs::write(&fault_path, serde_json::to_vec_pretty(&record)?)?;
        log_event(&format!(
            "consensus fault segment={} required={} total={}",
            segment_id, required_peer_quorum, total_participants
        ));
        recorder::event(
            &self.local_id,
            "trace_consensus_fault",
            &format!("segment {} quorum failure", segment_id),
        );
        Ok(())
    }

    fn derive_session_key(&self, nonce: &[u8; 32]) -> Result<TinyEd25519, CohError> {
        let signature = self.keyring.sign(nonce);
        let digest = Sha256::digest(&signature);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&digest[..32]);
        Ok(TinyEd25519::from_seed(&seed))
    }

    fn generate_nonce(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.local_id.as_bytes());
        hasher.update(now_timestamp().to_le_bytes());
        let digest = hasher.finalize();
        let mut seed_bytes = [0u8; 8];
        seed_bytes.copy_from_slice(&digest[..8]);
        let mut rng = TinyRng::new(u64::from_le_bytes(seed_bytes));
        let mut nonce = [0u8; 32];
        rng.fill_bytes(&mut nonce);
        nonce
    }
}

struct GroupSelection {
    segment_id: String,
    merkle_root: [u8; 32],
    entries: Vec<String>,
    votes: Vec<VerifiedSegment>,
    peer_votes: usize,
}

fn select_quorum(
    votes: &[VerifiedSegment],
    local_id: &str,
    required_peer_quorum: usize,
) -> Option<GroupSelection> {
    let mut groups: HashMap<String, GroupSelection> = HashMap::new();
    for vote in votes {
        let key = format!("{}:{}", vote.segment_id, hex::encode(vote.merkle_root));
        let entry = groups.entry(key).or_insert_with(|| GroupSelection {
            segment_id: vote.segment_id.clone(),
            merkle_root: vote.merkle_root,
            entries: vote.entries.clone(),
            votes: Vec::new(),
            peer_votes: 0,
        });
        if vote.from != local_id {
            entry.peer_votes += 1;
        }
        entry.votes.push(vote.clone());
    }

    let mut best: Option<GroupSelection> = None;
    for group in groups.into_values() {
        let has_local = group.votes.iter().any(|v| v.from == local_id);
        if !has_local {
            continue;
        }
        if group.peer_votes < required_peer_quorum {
            continue;
        }
        match &best {
            Some(existing) if existing.peer_votes >= group.peer_votes => {}
            _ => best = Some(group),
        }
    }
    best
}

fn compute_merkle_root(entries: &[String]) -> [u8; 32] {
    if entries.is_empty() {
        return [0u8; 32];
    }
    let mut hashes: Vec<[u8; 32]> = entries
        .iter()
        .map(|entry| {
            let mut hasher = Sha256::new();
            hasher.update(entry.as_bytes());
            let mut out = [0u8; 32];
            out.copy_from_slice(&hasher.finalize());
            out
        })
        .collect();
    while hashes.len() > 1 {
        let mut next = Vec::with_capacity((hashes.len() + 1) / 2);
        for chunk in hashes.chunks(2) {
            if chunk.len() == 1 {
                next.push(chunk[0]);
            } else {
                let mut hasher = Sha256::new();
                hasher.update(chunk[0]);
                hasher.update(chunk[1]);
                let mut out = [0u8; 32];
                out.copy_from_slice(&hasher.finalize());
                next.push(out);
            }
        }
        hashes = next;
    }
    hashes[0]
}

fn message_to_sign(segment_id: &str, nonce: &[u8; 32], merkle_root: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(segment_id.len() + nonce.len() + merkle_root.len());
    msg.extend_from_slice(segment_id.as_bytes());
    msg.extend_from_slice(nonce);
    msg.extend_from_slice(merkle_root);
    msg
}

fn decode_hex_array<const N: usize>(value: &str) -> Result<[u8; N], CohError> {
    let bytes = hex::decode(value).map_err(|e| new_err(format!("invalid hex value: {e}")))?;
    if bytes.len() != N {
        coh_bail!("expected {N} bytes but found {}", bytes.len());
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ensure_https(endpoint: &str) -> Result<(), CohError> {
    if endpoint.to_ascii_lowercase().starts_with("https://") {
        Ok(())
    } else {
        Err(new_err(format!("endpoint {endpoint} must use https")))
    }
}

fn build_tls_agent(extra_roots: &[Vec<u8>], user_agent: Option<&str>) -> Result<Agent, CohError> {
    let mut config_builder = Agent::config_builder();
    config_builder = config_builder.proxy(None);
    if !extra_roots.is_empty() {
        let certs: Vec<Certificate<'static>> = extra_roots
            .iter()
            .map(|der| Certificate::from_der(der).to_owned())
            .collect();
        let tls_config = TlsConfig::builder()
            .root_certs(RootCerts::from(certs))
            .build();
        config_builder = config_builder.tls_config(tls_config);
    }
    if let Some(agent_name) = user_agent {
        config_builder = config_builder.user_agent(agent_name);
    }
    Ok(config_builder.build().new_agent())
}

fn log_event(msg: &str) {
    if fs::create_dir_all(CONSENSUS_DIR).is_err() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(CONSENSUS_DIR).join("consensus.log"))
    {
        let _ = writeln!(f, "{}", msg);
    }
}

#[derive(Serialize)]
struct ConsensusRecord {
    timestamp: u64,
    segment_id: String,
    merkle_root: String,
    participants: Vec<String>,
    required_peer_quorum: usize,
    achieved_peer_votes: usize,
    total_participants: usize,
    entries: Vec<String>,
    nonces: Vec<PeerNonce>,
}

#[derive(Serialize)]
struct FaultRecord {
    timestamp: u64,
    segment_id: String,
    reason: String,
    required_peer_quorum: usize,
    total_participants: usize,
    policy_hash: Option<String>,
    votes: Vec<FaultVote>,
    disqualified: Vec<FaultPeer>,
}

#[derive(Serialize)]
struct FaultVote {
    peer: String,
    segment_id: String,
    merkle_root: String,
    nonce: String,
}

#[derive(Serialize)]
struct FaultPeer {
    peer: String,
    error: String,
}

#[derive(Serialize)]
struct PeerNonce {
    peer: String,
    nonce: String,
}
