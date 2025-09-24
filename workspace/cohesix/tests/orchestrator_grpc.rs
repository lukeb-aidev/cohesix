// Author: Lukas Bower
// Date Modified: 2029-01-15

use cohesix::orchestrator::protocol::{
    AssignRoleRequest, ClusterStateRequest, GpuTelemetry, HeartbeatRequest, JoinRequest,
    ScheduleRequest, TrustUpdateRequest,
};
use cohesix::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedKey, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use serial_test::serial;
use std::{
    convert::TryInto,
    env, fs,
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use tempfile::TempDir;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tonic::Code;

#[derive(Debug)]
struct EnvGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    fn apply_owned(pairs: &[(String, String)]) -> Self {
        let mut saved = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            saved.push((key.clone(), previous));
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            if let Some(original) = value {
                env::set_var(&key, original);
            } else {
                env::remove_var(&key);
            }
        }
    }
}

struct TlsTestMaterials {
    _dir: TempDir,
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    worker_cert: PathBuf,
    worker_key: PathBuf,
    queen_cert: PathBuf,
    queen_key: PathBuf,
    rogue_cert: PathBuf,
    rogue_key: PathBuf,
}

impl TlsTestMaterials {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let ca = generate_ca();
        let ca_cert = write_pem(dir.path(), "ca.pem", &ca.cert.pem());
        let (server_cert, server_key) = issue_server_cert(dir.path(), &ca);
        let (worker_cert, worker_key) = issue_client_cert(
            dir.path(),
            &ca,
            "worker",
            "worker-alpha",
            "spiffe://cohesix/worker/DroneWorker/worker-alpha",
        );
        let (queen_cert, queen_key) = issue_client_cert(
            dir.path(),
            &ca,
            "queen",
            "queen-admin",
            "spiffe://cohesix/queen/QueenPrimary/queen-admin",
        );
        let (rogue_cert, rogue_key) = issue_client_cert(
            dir.path(),
            &ca,
            "rogue",
            "rogue-alpha",
            "spiffe://cohesix/worker/RogueRole/rogue-alpha",
        );
        Self {
            _dir: dir,
            ca_cert,
            server_cert,
            server_key,
            worker_cert,
            worker_key,
            queen_cert,
            queen_key,
            rogue_cert,
            rogue_key,
        }
    }

    fn server_env(&self) -> Vec<(String, String)> {
        vec![
            ("COHESIX_ORCH_CA_CERT".into(), self.path_str(&self.ca_cert)),
            (
                "COHESIX_ORCH_SERVER_CERT".into(),
                self.path_str(&self.server_cert),
            ),
            (
                "COHESIX_ORCH_SERVER_KEY".into(),
                self.path_str(&self.server_key),
            ),
        ]
    }

    fn worker_env(&self) -> Vec<(String, String)> {
        vec![
            ("COHESIX_ORCH_CA_CERT".into(), self.path_str(&self.ca_cert)),
            (
                "COHESIX_ORCH_CLIENT_CERT".into(),
                self.path_str(&self.worker_cert),
            ),
            (
                "COHESIX_ORCH_CLIENT_KEY".into(),
                self.path_str(&self.worker_key),
            ),
        ]
    }

    fn queen_env(&self) -> Vec<(String, String)> {
        vec![
            ("COHESIX_ORCH_CA_CERT".into(), self.path_str(&self.ca_cert)),
            (
                "COHESIX_ORCH_CLIENT_CERT".into(),
                self.path_str(&self.queen_cert),
            ),
            (
                "COHESIX_ORCH_CLIENT_KEY".into(),
                self.path_str(&self.queen_key),
            ),
        ]
    }

    fn rogue_env(&self) -> Vec<(String, String)> {
        vec![
            ("COHESIX_ORCH_CA_CERT".into(), self.path_str(&self.ca_cert)),
            (
                "COHESIX_ORCH_CLIENT_CERT".into(),
                self.path_str(&self.rogue_cert),
            ),
            (
                "COHESIX_ORCH_CLIENT_KEY".into(),
                self.path_str(&self.rogue_key),
            ),
        ]
    }

    fn path_str(&self, path: &PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }
}

fn generate_ca() -> CertifiedKey {
    let mut params = CertificateParams::new(vec![]).expect("ca params");
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Cohesix Test CA");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let key_pair = KeyPair::generate().expect("ca key");
    let cert = params.self_signed(&key_pair).expect("ca certificate");
    CertifiedKey { cert, key_pair }
}

fn issue_server_cert(dir: &Path, ca: &CertifiedKey) -> (PathBuf, PathBuf) {
    let mut params = CertificateParams::new(vec!["localhost".into()]).expect("server params");
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "queen.local");
    params.distinguished_name = dn;
    params
        .subject_alt_names
        .push(SanType::DnsName("localhost".try_into().expect("dns name")));
    params.subject_alt_names.push(SanType::IpAddress(
        IpAddr::from_str("127.0.0.1").expect("loopback"),
    ));
    params.subject_alt_names.push(SanType::URI(
        "spiffe://cohesix/queen/QueenPrimary/queen-alpha"
            .try_into()
            .expect("queen uri"),
    ));
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let key_pair = KeyPair::generate().expect("server key");
    let cert = params
        .signed_by(&key_pair, &ca.cert, &ca.key_pair)
        .expect("server certificate");
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let cert_path = write_pem(dir, "server_cert.pem", &cert_pem);
    let key_path = write_pem(dir, "server_key.pem", &key_pem);
    (cert_path, key_path)
}

fn issue_client_cert(
    dir: &Path,
    ca: &CertifiedKey,
    prefix: &str,
    common_name: &str,
    role_uri: &str,
) -> (PathBuf, PathBuf) {
    let mut params = CertificateParams::new(vec!["localhost".into()]).expect("client params");
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    params
        .subject_alt_names
        .push(SanType::URI(role_uri.try_into().expect("role uri")));
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let key_pair = KeyPair::generate().expect("client key");
    let cert = params
        .signed_by(&key_pair, &ca.cert, &ca.key_pair)
        .expect("client certificate");
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let cert_path = write_pem(dir, &format!("{prefix}_cert.pem"), &cert_pem);
    let key_path = write_pem(dir, &format!("{prefix}_key.pem"), &key_pem);
    (cert_path, key_path)
}

fn write_pem(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write pem");
    path
}

#[test]
#[serial]
fn orchestrator_grpc_flows() {
    let materials = TlsTestMaterials::new();
    let server_env = materials.server_env();
    let _server_guard = EnvGuard::apply_owned(&server_env);

    let rt = Runtime::new().expect("runtime");
    rt.block_on(async {
        let orchestrator = QueenOrchestrator::new(1, SchedulePolicy::GpuPriority);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(orchestrator.clone().serve_with_listener(listener, async {
            let _ = shutdown_rx.await;
        }));

        let endpoint = format!("https://localhost:{}", addr.port());

        let worker_env = materials.worker_env();
        let worker_guard = EnvGuard::apply_owned(&worker_env);
        let mut worker_client = QueenOrchestrator::connect_client(&endpoint)
            .await
            .expect("worker client");
        worker_client
            .join(JoinRequest {
                worker_id: "worker-alpha".into(),
                ip: "10.1.1.10".into(),
                role: "DroneWorker".into(),
                capabilities: vec!["cuda".into()],
                trust: "green".into(),
            })
            .await
            .expect("join");
        worker_client
            .heartbeat(HeartbeatRequest {
                worker_id: "worker-alpha".into(),
                timestamp: 0,
                status: "running".into(),
                gpu: Some(GpuTelemetry {
                    perf_watt: 12.5,
                    mem_total: 24,
                    mem_free: 12,
                    last_temp: 60,
                    gpu_capacity: 12,
                    current_load: 3,
                    latency_score: 5,
                }),
            })
            .await
            .expect("heartbeat");

        let schedule = worker_client
            .request_schedule(ScheduleRequest {
                agent_id: "job-1".into(),
                require_gpu: true,
            })
            .await
            .expect("schedule")
            .into_inner();
        assert!(schedule.assigned);
        assert_eq!(schedule.worker_id, "worker-alpha");

        let state_err = worker_client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .expect_err("worker should not fetch cluster state");
        assert_eq!(state_err.code(), Code::PermissionDenied);
        drop(worker_guard);
        drop(worker_client);

        let queen_env = materials.queen_env();
        let queen_guard = EnvGuard::apply_owned(&queen_env);
        let mut queen_client = QueenOrchestrator::connect_client(&endpoint)
            .await
            .expect("queen client");

        queen_client
            .assign_role(AssignRoleRequest {
                worker_id: "worker-alpha".into(),
                role: "ComputeNode".into(),
            })
            .await
            .expect("assign");

        queen_client
            .update_trust(TrustUpdateRequest {
                worker_id: "worker-alpha".into(),
                level: "yellow".into(),
            })
            .await
            .expect("trust");

        let state = queen_client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .expect("state")
            .into_inner();
        assert_eq!(state.workers.len(), 1);
        assert_eq!(state.workers[0].trust, "yellow");
        assert_eq!(state.workers[0].role, "ComputeNode");

        tokio::time::sleep(Duration::from_secs(2)).await;
        let state_after = queen_client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .expect("state2")
            .into_inner();
        assert_eq!(state_after.workers[0].status, "restarting");

        drop(queen_guard);
        drop(queen_client);

        let _ = shutdown_tx.send(());
        server.await.expect("server").expect("shutdown");
    });
}

#[test]
#[serial]
fn orchestrator_rejects_untrusted_roles() {
    let materials = TlsTestMaterials::new();
    let server_env = materials.server_env();
    let _server_guard = EnvGuard::apply_owned(&server_env);

    let rt = Runtime::new().expect("runtime");
    rt.block_on(async {
        let orchestrator = QueenOrchestrator::new(1, SchedulePolicy::RoundRobin);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(orchestrator.clone().serve_with_listener(listener, async {
            let _ = shutdown_rx.await;
        }));

        let endpoint = format!("https://localhost:{}", addr.port());
        let rogue_env = materials.rogue_env();
        let _rogue_guard = EnvGuard::apply_owned(&rogue_env);
        let mut rogue_client = QueenOrchestrator::connect_client(&endpoint)
            .await
            .expect("rogue client");

        let err = rogue_client
            .join(JoinRequest {
                worker_id: "rogue-alpha".into(),
                ip: "10.9.0.10".into(),
                role: "RogueRole".into(),
                capabilities: vec![],
                trust: "red".into(),
            })
            .await
            .expect_err("join should be rejected");
        assert_eq!(err.code(), Code::PermissionDenied);

        let _ = shutdown_tx.send(());
        server.await.expect("server").expect("shutdown");
    });
}

#[test]
#[serial]
fn orchestrator_rejects_mismatched_worker_identity() {
    let materials = TlsTestMaterials::new();
    let server_env = materials.server_env();
    let _server_guard = EnvGuard::apply_owned(&server_env);

    let rt = Runtime::new().expect("runtime");
    rt.block_on(async {
        let orchestrator = QueenOrchestrator::new(1, SchedulePolicy::RoundRobin);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(orchestrator.clone().serve_with_listener(listener, async {
            let _ = shutdown_rx.await;
        }));

        let endpoint = format!("https://localhost:{}", addr.port());
        let worker_env = materials.worker_env();
        let _worker_guard = EnvGuard::apply_owned(&worker_env);
        let mut client = QueenOrchestrator::connect_client(&endpoint)
            .await
            .expect("client");

        let err = client
            .join(JoinRequest {
                worker_id: "worker-beta".into(),
                ip: "10.1.1.20".into(),
                role: "DroneWorker".into(),
                capabilities: vec!["cuda".into()],
                trust: "green".into(),
            })
            .await
            .expect_err("mismatched worker id should be rejected");
        assert_eq!(err.code(), Code::PermissionDenied);

        let _ = shutdown_tx.send(());
        server.await.expect("server").expect("shutdown");
    });
}
