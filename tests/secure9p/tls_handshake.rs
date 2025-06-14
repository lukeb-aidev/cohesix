// CLASSIFICATION: COMMUNITY
// Filename: tls_handshake.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

use cohesix::p9::secure::{
    auth_handler::NullAuth, cap_fid::Cap, policy_engine::PolicyEngine,
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
        policy.allow("anonymous".into(), Cap::READ);
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
}
