// CLASSIFICATION: COMMUNITY
// Filename: cohtrace_cli.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-12-01

use assert_cmd::Command;
use cohesix::orchestrator::protocol::{GpuTelemetry, HeartbeatRequest, JoinRequest};
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
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::{tempdir, TempDir};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

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
}

impl TlsTestMaterials {
    fn new() -> Self {
        let dir = TempDir::new().expect("tls tempdir");
        let ca = generate_ca();
        let ca_cert = write_pem(dir.path(), "ca.pem", &ca.cert.pem());
        let (server_cert, server_key) = issue_server_cert(dir.path(), &ca);
        let (worker_cert, worker_key) = issue_client_cert(
            dir.path(),
            &ca,
            "worker",
            "worker-x",
            "spiffe://cohesix/worker/DroneWorker/worker-x",
        );
        let (queen_cert, queen_key) = issue_client_cert(
            dir.path(),
            &ca,
            "queen",
            "queen-cli",
            "spiffe://cohesix/queen/QueenPrimary/queen-cli",
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

    fn path_str(&self, path: &PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }
}

fn generate_ca() -> CertifiedKey {
    let mut params = CertificateParams::new(vec![]).expect("ca params");
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Cohesix CLI Test CA");
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
    dn.push(DnType::CommonName, "queen.cli");
    params.distinguished_name = dn;
    params
        .subject_alt_names
        .push(SanType::DnsName("localhost".try_into().expect("dns name")));
    params.subject_alt_names.push(SanType::IpAddress(
        IpAddr::from_str("127.0.0.1").expect("loopback"),
    ));
    params.subject_alt_names.push(SanType::URI(
        "spiffe://cohesix/queen/QueenPrimary/queen-cli"
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
fn diff_command_outputs_changes() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().join("snapshots");
    fs::create_dir_all(&base).unwrap();
    let base_abs = fs::canonicalize(&base).unwrap();
    let rules_path = tmp.path().join("rules.json");
    let rules = format!("{{\"allowed_roots\":[\"{}\"]}}", base_abs.to_string_lossy());
    fs::write(&rules_path, rules).unwrap();

    let older = base.join("worker.json");
    fs::write(
        &older,
        r#"{"worker_id":"alpha","timestamp":1,"sim":{"mode":"idle"}}"#,
    )
    .unwrap();
    thread::sleep(Duration::from_millis(10));
    let newer = base.join("worker_new.json");
    fs::write(
        &newer,
        r#"{"worker_id":"alpha","timestamp":2,"sim":{"mode":"active"}}"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("diff")
        .env("SNAPSHOT_BASE", &base)
        .env("COHTRACE_RULES_PATH", &rules_path)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace diff");

    assert_eq!(output.status.code(), Some(30));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Comparing snapshots"));
    assert!(stdout.contains("sim.mode"));
}

#[test]
fn diff_command_no_drift_returns_success() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().join("snapshots");
    fs::create_dir_all(&base).unwrap();
    let base_abs = fs::canonicalize(&base).unwrap();
    let rules_path = tmp.path().join("rules.json");
    let rules = format!("{{\"allowed_roots\":[\"{}\"]}}", base_abs.to_string_lossy());
    fs::write(&rules_path, rules).unwrap();

    let snap_a = base.join("a.json");
    let snap_b = base.join("b.json");
    let payload = r#"{"worker_id":"alpha","timestamp":1,"sim":{"mode":"idle"}}"#;
    fs::write(&snap_a, payload).unwrap();
    thread::sleep(Duration::from_millis(10));
    fs::write(&snap_b, payload).unwrap();

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("diff")
        .env("SNAPSHOT_BASE", &base)
        .env("COHTRACE_RULES_PATH", &rules_path)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace diff clean");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No differences detected."));
}

#[test]
fn diff_command_missing_rules_sets_taxonomy() {
    let tmp = tempdir().unwrap();
    let base = tmp.path().join("snapshots");
    fs::create_dir_all(&base).unwrap();
    let snap_a = base.join("a.json");
    let snap_b = base.join("b.json");
    fs::write(
        &snap_a,
        r#"{"worker_id":"alpha","timestamp":1,"sim":{"mode":"idle"}}"#,
    )
    .unwrap();
    thread::sleep(Duration::from_millis(10));
    fs::write(
        &snap_b,
        r#"{"worker_id":"alpha","timestamp":2,"sim":{"mode":"active"}}"#,
    )
    .unwrap();

    let missing_rules = tmp.path().join("missing_rules.json");
    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    let output = cmd
        .arg("diff")
        .env("SNAPSHOT_BASE", &base)
        .env("COHTRACE_RULES_PATH", &missing_rules)
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace diff missing rules");

    assert_eq!(output.status.code(), Some(32));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cohtrace diff rule violation"));
}

#[test]
#[serial]
fn cloud_command_outputs_registry() {
    let materials = TlsTestMaterials::new();
    let server_env = materials.server_env();
    let _server_guard = EnvGuard::apply_owned(&server_env);

    let tmp = tempdir().unwrap();
    let rt = Runtime::new().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let guard = rt.enter();
    let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();
    drop(guard);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let orchestrator = QueenOrchestrator::new(5, SchedulePolicy::RoundRobin);
    let server = rt.spawn(
        orchestrator
            .clone()
            .serve_with_listener(tokio_listener, async {
                let _ = shutdown_rx.await;
            }),
    );

    let endpoint = format!("https://localhost:{}", addr.port());
    let worker_env = materials.worker_env();
    let worker_guard = EnvGuard::apply_owned(&worker_env);
    rt.block_on(async {
        let mut client = QueenOrchestrator::connect_client(&endpoint).await.unwrap();
        client
            .join(JoinRequest {
                worker_id: "worker-x".into(),
                ip: "10.0.0.5".into(),
                role: "DroneWorker".into(),
                capabilities: vec!["cuda".into()],
                trust: "green".into(),
            })
            .await
            .unwrap();
        client
            .heartbeat(HeartbeatRequest {
                worker_id: "worker-x".into(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                status: "running".into(),
                gpu: Some(GpuTelemetry {
                    perf_watt: 10.0,
                    mem_total: 16,
                    mem_free: 8,
                    last_temp: 55,
                    gpu_capacity: 10,
                    current_load: 2,
                    latency_score: 3,
                }),
            })
            .await
            .unwrap();
    });
    drop(worker_guard);

    let mut cmd = Command::cargo_bin("cohtrace").unwrap();
    cmd.arg("cloud").env("COHESIX_ORCH_ADDR", &endpoint);
    for (key, value) in materials.queen_env() {
        cmd.env(key, value);
    }
    let output = cmd
        .current_dir(tmp.path())
        .output()
        .expect("run cohtrace cloud");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Queen ID"));
    assert!(stdout.contains("worker-x (DroneWorker)"));
    assert!(stdout.contains("status: running"));

    let _ = shutdown_tx.send(());
    rt.block_on(async {
        server.await.unwrap().unwrap();
    });
}
