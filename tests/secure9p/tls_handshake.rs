// CLASSIFICATION: COMMUNITY
// Filename: tls_handshake.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

use cohesix::p9::secure::secure_9p_server::start_secure_9p_server;
use jsonwebtoken::{encode, EncodingKey, Header};
use rcgen::generate_simple_self_signed;
use rustls::{Certificate, ClientConfig, RootCertStore, ClientConnection, StreamOwned};
use std::sync::Arc;
use tempfile::tempdir;
use std::thread;
use std::time::Duration;
use std::fs;
use std::io::Write;
use cohesix::p9::secure::validator_hook::ValidatorHook;

#[test]
fn tls_handshake() {
    if std::net::TcpListener::bind("127.0.0.1:5690").is_err() {
        eprintln!("skipping tls_handshake: port 5690 unavailable");
        return;
    }
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("config")).unwrap();
    fs::write(
        dir.path().join("config/secure9p.toml"),
        "\n[[namespace]]\nagent='tester'\nroot='/tmp'\nread_only=false\n",
    )
    .unwrap();
    std::env::set_var("COHESIX_LOG_DIR", dir.path());
    let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_path = dir.path().join("cert.pem");
    let key_path = dir.path().join("key.pem");
    fs::write(&cert_path, cert.serialize_pem().unwrap()).unwrap();
    fs::write(&key_path, cert.serialize_private_key_pem()).unwrap();
    let addr = "127.0.0.1:5690".to_string();
    let cert_clone = cert_path.clone();
    let key_clone = key_path.clone();
    let dir_clone = dir.path().to_path_buf();
    thread::spawn(move || {
        std::env::set_current_dir(&dir_clone).unwrap();
        let _ = start_secure_9p_server(&addr, &cert_clone, &key_clone);
use cohesix::secure9p::{
    auth_handler::NullAuth, cap_fid::Capability, policy_engine::PolicyEngine,
    secure_9p_server::Secure9pServer,
};
use rcgen::generate_simple_self_signed;
use rustls::{Certificate, ClientConfig, RootCertStore};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::runtime::Runtime;
use tokio_rustls::TlsConnector;

#[test]
fn tls_handshake() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let dir = tempdir().unwrap();
        std::env::set_var("COHESIX_LOG_DIR", dir.path());
        let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, cert.serialize_pem().unwrap()).unwrap();
        std::fs::write(&key_path, cert.serialize_private_key_pem()).unwrap();
        let mut policy = PolicyEngine::new();
        policy.allow("anonymous".into(), Capability::Read);
        let server = Secure9pServer {
            port: 5690,
            cert_path: cert_path.to_string_lossy().into(),
            key_path: key_path.to_string_lossy().into(),
            auth_handler: NullAuth,
            policy,
            validator: None,
        };
        let handle = tokio::spawn(async move { server.run_once().await });
        let mut roots = RootCertStore::empty();
        roots
            .add(&Certificate(cert.serialize_der().unwrap()))
            .unwrap();
        let client_config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", 5690))
            .await
            .unwrap();
        let server_name = "localhost".try_into().unwrap();
        let mut tls = connector.connect(server_name, stream).await.unwrap();
        tls.write_all(&[0]).await.unwrap();
        let mut buf = [0u8; 1];
        tls.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf[0], 1);
        handle.await.unwrap().unwrap();
        std::env::remove_var("COHESIX_LOG_DIR");
        let log = std::fs::read_to_string(dir.path().join("secure9p.log")).unwrap();
        assert!(log.contains("handshake"));
    });
    std::thread::sleep(Duration::from_millis(100));
    let mut roots = RootCertStore::empty();
    roots.add(&Certificate(cert.serialize_der().unwrap())).unwrap();
    let client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let stream = std::net::TcpStream::connect("127.0.0.1:5690").unwrap();
    let conn = ClientConnection::new(Arc::new(client_config), "localhost".try_into().unwrap()).unwrap();
    let mut tls = StreamOwned::new(conn, stream);
    let token = encode(&Header::default(), &serde_json::json!({"sub":"tester"}),
        &EncodingKey::from_secret(b"cohesix")).unwrap();
    tls.write_all(format!("JWT {}\n", token).as_bytes()).unwrap();
    tls.flush().unwrap();
    drop(tls);
    std::thread::sleep(Duration::from_millis(100));
    let hook_path = dir.path().join("hook.log");
    let hook = ValidatorHook::new(hook_path.clone());
    hook.log("tester", "read", "/tmp/x", "ok");
    let log = fs::read_to_string(hook_path).unwrap();
    assert!(log.contains("tester"));
}
