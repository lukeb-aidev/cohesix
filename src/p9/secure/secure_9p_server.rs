// CLASSIFICATION: COMMUNITY
// Filename: secure_9p_server.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! TLS-wrapped 9P server with policy enforcement.

#[cfg(feature = "secure9p")]
use crate::p9::secure::{
    auth_handler,
    namespace_resolver::{self, MountNamespace},
    policy_engine::PolicyEngine,
    validator_hook::ValidatorHook,
};
#[cfg(feature = "secure9p")]
use cohesix_9p::{policy::SandboxPolicy, FsConfig, FsServer};
#[cfg(feature = "secure9p")]
use rustls::{server::ServerConfig, Certificate, PrivateKey, ServerConnection, StreamOwned};
#[cfg(feature = "secure9p")]
use rustls_pemfile::{certs, pkcs8_private_keys};
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
    let engine = PolicyEngine::load()?;
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
            let id = auth_handler::extract_identity(tls.conn_mut(), &mut cursor).unwrap_or_else(|_| "unknown".into());
            let MountNamespace { root, readonly } = match namespace_resolver::resolve_namespace(&id) {
                Ok(ns) => ns,
                Err(_) => return,
            };
            let socket = std::env::temp_dir().join(format!("secure9p_{id}.sock"));
            let mut fs = FsServer::new(FsConfig { root, port: 0, readonly });
            fs.set_policy(id.clone(), policy_for(&engine, &id));
            fs.set_validator_hook(Arc::new(move |ty, f, agent, _| hook.log(&agent, ty, &f, "")));
            fs.start_socket(&socket).ok();
            if let Ok(mut inner) = UnixStream::connect(&socket) {
                let mut writer = inner.try_clone().unwrap();
                let _ = std::io::copy(&mut cursor, &mut writer);
            }
        });
    }
    Ok(())
}
