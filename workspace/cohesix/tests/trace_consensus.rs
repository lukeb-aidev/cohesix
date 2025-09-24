// CLASSIFICATION: COMMUNITY
// Filename: trace_consensus.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-24

use cohesix::trace::trace_consensus::{PeerDescriptor, TraceConsensus, TraceSegment};
use cohesix::{new_err, CohError};
use rcgen::generate_simple_self_signed;
use serde_json::Value;
use serial_test::serial;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Response, Server, SslConfig};

struct MockPeerServer {
    server: Arc<Server>,
    join: Option<thread::JoinHandle<()>>,
}

impl MockPeerServer {
    fn start(
        peer_id: &str,
        body: String,
        config: SslConfig,
    ) -> Result<(Self, PeerDescriptor), CohError> {
        let server = Arc::new(
            Server::https("127.0.0.1:0", config)
                .map_err(|e| new_err(format!("failed to start TLS server: {e}")))?,
        );
        let port = server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .ok_or_else(|| new_err("unsupported server address"))?;
        let response_body = Arc::new(body);
        let server_clone = server.clone();
        let body_clone = response_body.clone();
        let join = thread::spawn(move || {
            for request in server_clone.incoming_requests() {
                let response = Response::from_string((*body_clone).clone())
                    .with_header(Header::from_bytes(b"Content-Type", b"application/json").unwrap());
                let _ = request.respond(response);
                break;
            }
        });
        let endpoint = format!("https://localhost:{port}/segment");
        Ok((
            Self {
                server,
                join: Some(join),
            },
            PeerDescriptor::new(peer_id.to_string(), endpoint),
        ))
    }
}

impl Drop for MockPeerServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn cleanup_paths() {
    let _ = fs::remove_dir_all("/srv/trace");
    let _ = fs::remove_dir_all("/srv/fault");
    let _ = fs::remove_dir_all("/srv/federation");
}

fn spawn_peer(
    peer_id: &str,
    segment_id: &str,
    entries: Vec<String>,
) -> Result<(MockPeerServer, PeerDescriptor, Vec<u8>), CohError> {
    let peer_consensus = TraceConsensus::new(peer_id)?;
    let envelope = peer_consensus.prepare_segment(&TraceSegment::new(segment_id, entries))?;
    let body = serde_json::to_string(&envelope)?;
    let certificate = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = certificate.cert.der().to_vec();
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();
    let ssl_config = SslConfig {
        certificate: cert_pem.into_bytes(),
        private_key: key_pem.into_bytes(),
    };
    let (server, descriptor) = MockPeerServer::start(peer_id, body, ssl_config)?;
    Ok((server, descriptor, cert_der))
}

#[test]
#[serial]
fn consensus_success_tls() -> Result<(), CohError> {
    cleanup_paths();
    let segment_id = "seg-42";
    let entries = vec!["evt-a".to_string(), "evt-b".to_string()];
    let (server_a, desc_a, cert_a) = spawn_peer("peer-a", segment_id, entries.clone())?;
    let (server_b, desc_b, cert_b) = spawn_peer("peer-b", segment_id, entries.clone())?;

    let _guards = (server_a, server_b);

    let builder = TraceConsensus::builder("queen-test")
        .add_root(&cert_a)
        .add_root(&cert_b);
    let consensus = builder.build()?;
    let result = consensus.run(
        TraceSegment::new(segment_id, entries.clone()),
        &[desc_a.clone(), desc_b.clone()],
    )?;

    assert_eq!(result.segment_id, segment_id);
    assert_eq!(result.required_peer_quorum, 2);
    assert_eq!(result.achieved_peer_votes, 2);
    assert_eq!(result.total_participants, 3);
    assert!(result.participants.contains(&"queen-test".to_string()));
    assert!(result.participants.contains(&"peer-a".to_string()));
    assert!(result.participants.contains(&"peer-b".to_string()));

    let snapshot_path = Path::new("/srv/trace/consensus/current.snapshot");
    assert!(snapshot_path.exists());
    let snapshot: Value = serde_json::from_str(&fs::read_to_string(snapshot_path)?)?;
    assert_eq!(snapshot["segment_id"], segment_id);
    assert_eq!(snapshot["merkle_root"], result.merkle_root);
    Ok(())
}

#[test]
#[serial]
fn consensus_failure_logs_fault() -> Result<(), CohError> {
    cleanup_paths();
    let segment_id = "seg-diverge";
    let entries_good = vec!["evt-1".to_string(), "evt-2".to_string()];
    let entries_bad = vec!["evt-1".to_string(), "evt-x".to_string()];
    let (server_a, desc_a, cert_a) = spawn_peer("peer-a", segment_id, entries_good.clone())?;
    let (server_b, desc_b, cert_b) = spawn_peer("peer-b", segment_id, entries_bad.clone())?;
    let _guards = (server_a, server_b);

    let builder = TraceConsensus::builder("queen-test")
        .add_root(&cert_a)
        .add_root(&cert_b);
    let consensus = builder.build()?;
    let peers = [desc_a.clone(), desc_b.clone()];
    let err = consensus
        .run(TraceSegment::new(segment_id, entries_good.clone()), &peers)
        .unwrap_err();
    assert!(format!("{err}").contains("quorum"));

    let fault_dir = Path::new("/srv/trace/fault");
    assert!(fault_dir.exists());
    let files = fs::read_dir(fault_dir)?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let fault: Value = serde_json::from_str(&fs::read_to_string(files[0].path())?)?;
    assert_eq!(fault["segment_id"], segment_id);
    assert!(fault["policy_hash"].as_str().unwrap().len() > 0);
    assert_eq!(fault["required_peer_quorum"], 2);
    assert!(fault["disqualified"].as_array().unwrap().len() >= 1);
    Ok(())
}

#[test]
#[serial]
fn rejects_non_tls_endpoints() -> Result<(), CohError> {
    cleanup_paths();
    let consensus = TraceConsensus::new("queen-test")?;
    let peer = PeerDescriptor::new("peer-http", "http://localhost:12345/segment");
    let result = consensus.run(
        TraceSegment::new("seg-http", vec!["evt".to_string()]),
        &[peer],
    );
    assert!(result.is_err());
    let fault_dir = Path::new("/srv/trace/fault");
    assert!(fault_dir.exists());
    Ok(())
}
