// CLASSIFICATION: COMMUNITY
// Filename: secure_9p_server.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor};
use rustls::{pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer}, ServerConfig};
use rustls_pemfile::{certs, rsa_private_keys};
use serde_json::json;

use super::{auth_handler::AuthHandler, namespace_resolver::resolve, sandbox::enforce, validator_hook::ValidatorHook, cap_fid::{Capability}, policy_engine::PolicyEngine};

pub struct Secure9pServer<H: AuthHandler + Send + Sync + 'static> {
    pub port: u16,
    pub cert_path: String,
    pub key_path: String,
    pub auth_handler: H,
    pub policy: PolicyEngine,
    pub validator: Option<ValidatorHook>,
}

impl<H: AuthHandler + Send + Sync + 'static> Secure9pServer<H> {
    fn tls_acceptor(&self) -> anyhow::Result<TlsAcceptor> {
        use std::fs::File;
        use std::io::BufReader;
        let cert_file = &mut BufReader::new(File::open(&self.cert_path)?);
        let key_file = &mut BufReader::new(File::open(&self.key_path)?);
        let cert_chain = certs(cert_file)?
            .into_iter()
            .map(|c| CertificateDer::from(c).into_owned())
            .collect();
        let mut keys = rsa_private_keys(key_file)?;
        let key = PrivateKeyDer::from(PrivatePkcs1KeyDer::from(keys.remove(0)));
        let cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)?;
        Ok(TlsAcceptor::from(Arc::new(cfg)))
    }

    pub async fn run_once(&self) -> anyhow::Result<()> {
        let acceptor = self.tls_acceptor()?;
        let listener = TcpListener::bind(("127.0.0.1", self.port)).await?;
        let (stream, _) = listener.accept().await?;
        let mut stream = acceptor.accept(stream).await?;
        let mut buf = [0u8; 1];
        stream.read_exact(&mut buf).await?;
        let agent = self.auth_handler.authenticate(&buf);
        let ns = resolve(&agent);
        log_event(json!({"event":"handshake","agent":agent,"ns":ns}));
        if enforce(&ns, Capability::Read, &self.policy) {
            stream.write_all(b"1").await?;
        } else if let Some(h) = &self.validator {
            h("capability_denied", ns, crate::validator::timestamp());
        }
        Ok(())
    }
}

fn log_event(v: serde_json::Value) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/secure9p.log")
    {
        let _ = serde_json::to_writer(&mut f, &v);
        let _ = writeln!(f);
    }
}
