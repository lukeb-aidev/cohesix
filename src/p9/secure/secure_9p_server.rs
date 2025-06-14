// CLASSIFICATION: COMMUNITY
// Filename: secure_9p_server.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! TLS-wrapped 9P server with policy enforcement.

#[cfg(feature = "secure9p")]
use crate::p9::secure::{
    auth_handler,
    cap_fid::Cap,
    namespace_resolver::{self, MountNamespace},
    policy_engine::PolicyEngine,
    sandbox,
    validator_hook::ValidatorHook,
};
#[cfg(feature = "secure9p")]
use cohesix_9p::{policy::SandboxPolicy, FsConfig, FsServer};
#[cfg(feature = "secure9p")]
use rustls::{server::ServerConfig, Certificate, PrivateKey, ServerConnection, StreamOwned};
#[cfg(feature = "secure9p")]
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
#[cfg(feature = "secure9p")]
use serde_json::json;
#[cfg(feature = "secure9p")]
use std::{
    fs::File,
    io::{BufReader, Read, Write},
    net::TcpListener,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

#[cfg(feature = "secure9p")]
fn load_certs(path: &Path) -> anyhow::Result<Vec<Certificate>> {
    let mut rd = BufReader::new(File::open(path)?);
    Ok(certs(&mut rd)?.into_iter().map(Certificate).collect())
}

#[cfg(feature = "secure9p")]
fn load_key(path: &Path) -> anyhow::Result<PrivateKey> {
    let mut rd = BufReader::new(File::open(path)?);
    let keys = pkcs8_private_keys(&mut rd)?;
    Ok(PrivateKey(
        keys.first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no key"))?,
    ))
}

#[cfg(feature = "secure9p")]
fn policy_for(engine: &PolicyEngine, agent: &str) -> SandboxPolicy {
    let mut pol = SandboxPolicy::default();
    for (verb, path) in engine.policy_for(agent) {
        match verb.as_str() {
            "read" => pol.read.push(path),
            "write" => pol.write.push(path),
            _ => {}
        }
    }
    pol
}

/// Start a TLS-wrapped 9P server listening on `addr` using `cert` and `key`.
#[cfg(feature = "secure9p")]
pub fn start_secure_9p_server(addr: &str, cert: &Path, key: &Path) -> anyhow::Result<()> {
    let certs = load_certs(cert)?;
    let key = load_key(key)?;
    let tls_cfg = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    let listener = TcpListener::bind(addr)?;
    let engine = PolicyEngine::load(Path::new("config/secure9p.toml"))?;
    let log_dir = std::env::var("COHESIX_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let hook = ValidatorHook::new(log_dir.join("secure9p.log"));
    for stream in listener.incoming() {
        let tcp = stream?;
        let cfg = Arc::new(tls_cfg.clone());
        let engine = engine.clone();
        let hook = hook.clone();
        thread::spawn(move || {
            let mut conn = ServerConnection::new(cfg).unwrap();
            let mut tls = StreamOwned::new(conn, tcp);
            if tls.complete_io().is_err() {
                return;
            }
            let mut buf = Vec::new();
            let _ = tls.read_to_end(&mut buf);
            let mut cursor = std::io::Cursor::new(buf);
            let id = auth_handler::extract_identity(tls.conn_mut(), &mut cursor)
                .unwrap_or_else(|_| "unknown".into());
            let MountNamespace { root, readonly } = match namespace_resolver::resolve_namespace(&id)
            {
                Ok(ns) => ns,
                Err(_) => return,
            };
            let socket = std::env::temp_dir().join(format!("secure9p_{id}.sock"));
            let mut fs = FsServer::new(FsConfig {
                root,
                port: 0,
                readonly,
            });
            fs.set_policy(id.clone(), policy_for(&engine, &id));
            fs.set_validator_hook(Arc::new(move |ty, f, agent, _| {
                hook.log(&agent, ty, &f, "")
            }));
            fs.start_socket(&socket).ok();
            if let Ok(mut inner) = UnixStream::connect(&socket) {
                let mut writer = inner.try_clone().unwrap();
                let _ = std::io::copy(&mut cursor, &mut writer);
            }
        });
    }
    Ok(())
}

#[cfg(feature = "secure9p")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "secure9p")]
use tokio::net::TcpListener;
#[cfg(feature = "secure9p")]
use tokio_rustls::TlsAcceptor;

#[cfg(feature = "secure9p")]
pub struct Secure9pServer<H: auth_handler::AuthHandler + Send + Sync + 'static> {
    pub port: u16,
    pub cert_path: String,
    pub key_path: String,
    pub auth_handler: H,
    pub policy: PolicyEngine,
    pub validator: Option<ValidatorHook>,
}

#[cfg(feature = "secure9p")]
impl<H: auth_handler::AuthHandler + Send + Sync + 'static> Secure9pServer<H> {
    fn tls_acceptor(&self) -> anyhow::Result<TlsAcceptor> {
        use std::fs::File;
        use std::io::BufReader;
        let cert_file = &mut BufReader::new(File::open(&self.cert_path)?);
        let key_file = &mut BufReader::new(File::open(&self.key_path)?);
        let cert_chain = certs(cert_file)?
            .into_iter()
            .map(|c| rustls::pki_types::CertificateDer::from(c).into_owned())
            .collect();
        let mut keys = rsa_private_keys(key_file)?;
        let key = rustls::pki_types::PrivateKeyDer::from(
            rustls::pki_types::PrivatePkcs1KeyDer::from(keys.remove(0)),
        );
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
        let ns = format!("/srv/namespaces/{}", agent);
        log_event(json!({"event":"handshake","agent":agent,"ns":ns}));
        if sandbox::enforce(&ns, Cap::READ, &self.policy) {
            stream.write_all(b"1").await?;
        } else if let Some(h) = &self.validator {
            h.log(&agent, "capability_denied", &ns, "");
        }
        Ok(())
    }
}

#[cfg(feature = "secure9p")]
fn log_event(v: serde_json::Value) {
    let log_dir = std::env::var("COHESIX_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("secure9p.log"))
    {
        let _ = serde_json::to_writer(&mut f, &v);
        let _ = writeln!(f);
    }
}
