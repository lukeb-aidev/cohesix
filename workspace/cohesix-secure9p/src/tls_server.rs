// CLASSIFICATION: COMMUNITY
// Filename: tls_server.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-12-31

use crate::config::Secure9pConfig;
use crate::reconcile::PolicyReconciler;
use cohesix_9p::{policy::SandboxPolicy, NinepBackend};
use log::{error, info, warn};
use ninep::Stream;
use rustls::{
    server::AllowAnyAuthenticatedClient, Certificate, OwnedTrustAnchor, PrivateKey, RootCertStore,
};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use webpki_roots::TLS_SERVER_ROOTS;

pub struct SecureConfig {
    pub port: u16,
    pub cert: PathBuf,
    pub key: PathBuf,
    pub ca_cert: Option<PathBuf>,
    pub require_client_auth: bool,
}

impl From<&Secure9pConfig> for SecureConfig {
    fn from(cfg: &Secure9pConfig) -> Self {
        Self {
            port: cfg.port,
            cert: cfg.cert.clone(),
            key: cfg.key.clone(),
            ca_cert: cfg.ca_cert.clone(),
            require_client_auth: cfg.require_client_auth,
        }
    }
}

struct ServerThread {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for ServerThread {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            if let Err(e) = handle.join() {
                error!("secure9p thread join error: {:?}", e);
            }
        }
    }
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct HeartbeatTelemetry {
    inner: Arc<HeartbeatTelemetryInner>,
}

struct HeartbeatTelemetryInner {
    entries: Mutex<HashMap<String, HeartbeatEntry>>,
    counter: AtomicU64,
}

struct HeartbeatEntry {
    peer: String,
    alive: Arc<AtomicBool>,
    last_activity: Instant,
    missed: u32,
}

impl HeartbeatTelemetry {
    fn new() -> Self {
        let inner = HeartbeatTelemetryInner {
            entries: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    fn register(&self, peer: String) -> HeartbeatRegistration {
        let id = format!(
            "mount-{}",
            self.inner.counter.fetch_add(1, Ordering::Relaxed)
        );
        let alive = Arc::new(AtomicBool::new(true));
        let mut guard = self.inner.entries.lock().unwrap();
        guard.insert(
            id.clone(),
            HeartbeatEntry {
                peer: peer.clone(),
                alive: alive.clone(),
                last_activity: Instant::now(),
                missed: 0,
            },
        );
        drop(guard);
        info!("secure9p mount registered: id={} peer={}", id, peer);
        HeartbeatRegistration {
            telemetry: self.clone(),
            id,
            alive,
        }
    }

    fn mark_activity(&self, id: &str) {
        if let Some(entry) = self.inner.entries.lock().unwrap().get_mut(id) {
            entry.last_activity = Instant::now();
            entry.missed = 0;
        }
    }

    fn mark_closed(&self, id: &str) {
        if let Some(entry) = self.inner.entries.lock().unwrap().get_mut(id) {
            entry.alive.store(false, Ordering::SeqCst);
            entry.last_activity = Instant::now();
        }
    }

    fn spawn_loop(&self) -> io::Result<HeartbeatThread> {
        HeartbeatThread::start(self.clone())
    }

    fn emit_tick(&self) {
        let mut remove = Vec::new();
        let mut guard = self.inner.entries.lock().unwrap();
        let now = Instant::now();
        for (id, entry) in guard.iter_mut() {
            if entry.alive.load(Ordering::Relaxed) {
                let idle = now.duration_since(entry.last_activity).as_secs();
                info!(
                    "secure9p heartbeat: id={} peer={} idle={}s",
                    id, entry.peer, idle
                );
                entry.missed = 0;
            } else {
                entry.missed += 1;
                if entry.missed == 1 {
                    warn!(
                        "secure9p heartbeat missed once: id={} peer={}",
                        id, entry.peer
                    );
                } else {
                    error!(
                        "secure9p heartbeat ALERT: id={} peer={} missed {} intervals",
                        id, entry.peer, entry.missed
                    );
                    remove.push(id.clone());
                }
            }
        }
        for id in remove {
            guard.remove(&id);
        }
    }

    #[cfg(test)]
    fn entry_count(&self) -> usize {
        self.inner.entries.lock().unwrap().len()
    }
}

struct HeartbeatRegistration {
    telemetry: HeartbeatTelemetry,
    id: String,
    alive: Arc<AtomicBool>,
}

impl HeartbeatRegistration {
    fn touch(&self) {
        self.telemetry.mark_activity(&self.id);
    }
}

impl Drop for HeartbeatRegistration {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.telemetry.mark_closed(&self.id);
    }
}

struct HeartbeatThread {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HeartbeatThread {
    fn start(telemetry: HeartbeatTelemetry) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let handle = thread::Builder::new()
            .name("secure9p-heartbeat".into())
            .spawn(move || heartbeat_loop(telemetry, thread_running))?;
        Ok(Self {
            running,
            handle: Some(handle),
        })
    }
}

impl Drop for HeartbeatThread {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            if let Err(err) = handle.join() {
                error!("secure9p heartbeat join error: {:?}", err);
            }
        }
    }
}

fn heartbeat_loop(telemetry: HeartbeatTelemetry, running: Arc<AtomicBool>) {
    while running.load(Ordering::Relaxed) {
        thread::sleep(HEARTBEAT_INTERVAL);
        telemetry.emit_tick();
    }
}

pub struct Secure9PServer {
    cfg: SecureConfig,
    backend: NinepBackend,
    tls: Arc<ServerConfig>,
    telemetry: HeartbeatTelemetry,
    thread: Option<ServerThread>,
    heartbeat: Option<HeartbeatThread>,
}

impl Secure9PServer {
    pub fn new(cfg: SecureConfig, backend: NinepBackend) -> io::Result<Self> {
        let tls = Arc::new(build_tls_config(&cfg)?);
        Ok(Self {
            cfg,
            backend,
            tls,
            telemetry: HeartbeatTelemetry::new(),
            thread: None,
            heartbeat: None,
        })
    }

    pub fn apply_policy(&self, policies: &[(String, SandboxPolicy)]) {
        for (agent, policy) in policies {
            self.backend.set_agent_policy(agent, policy.clone());
        }
    }

    pub fn start(&mut self) -> io::Result<()> {
        if self.thread.is_some() {
            return Ok(());
        }
        let listener = TcpListener::bind(("0.0.0.0", self.cfg.port))?;
        listener.set_nonblocking(true)?;
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let tls = self.tls.clone();
        let backend = self.backend.clone();
        let telemetry = self.telemetry.clone();
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(self.cfg.port);
        self.cfg.port = port;
        let handle = thread::Builder::new()
            .name(format!("secure9p-{port}"))
            .spawn(move || accept_loop(listener, backend, tls, thread_running, telemetry))?;
        info!("Secure9P server listening on 0.0.0.0:{port}");
        self.thread = Some(ServerThread {
            running,
            handle: Some(handle),
        });
        if self.heartbeat.is_none() {
            self.heartbeat = Some(self.telemetry.spawn_loop()?);
        }
        Ok(())
    }
}

fn build_tls_config(cfg: &SecureConfig) -> io::Result<ServerConfig> {
    let certs = load_certs(&cfg.cert)?;
    let key = load_key(&cfg.key)?;
    let builder = ServerConfig::builder().with_safe_defaults();
    let config = if cfg.require_client_auth {
        let store = load_client_store(cfg)?;
        let verifier = AllowAnyAuthenticatedClient::new(store);
        builder
            .with_client_cert_verifier(Arc::new(verifier))
            .with_single_cert(certs, key)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
    };
    Ok(config)
}

fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs = certs(&mut reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .into_iter()
        .map(Certificate)
        .collect();
    Ok(certs)
}

fn load_key(path: &Path) -> io::Result<PrivateKey> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    if let Ok(mut keys) = pkcs8_private_keys(&mut reader) {
        if let Some(key) = keys.pop() {
            return Ok(PrivateKey(key));
        }
    }
    reader.rewind()?;
    let mut rsa_keys =
        rsa_private_keys(&mut reader).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    rsa_keys
        .pop()
        .map(PrivateKey)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found"))
}

fn load_client_store(cfg: &SecureConfig) -> io::Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    if let Some(path) = &cfg.ca_cert {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let certs =
            certs(&mut reader).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        for cert in certs {
            store
                .add(&Certificate(cert))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
    } else {
        store.add_trust_anchors(TLS_SERVER_ROOTS.iter().map(|anchor| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                anchor.subject,
                anchor.spki,
                anchor.name_constraints,
            )
        }));
    }
    Ok(store)
}

fn accept_loop(
    listener: TcpListener,
    backend: NinepBackend,
    tls: Arc<ServerConfig>,
    running: Arc<AtomicBool>,
    telemetry: HeartbeatTelemetry,
) {
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                handle_client(
                    stream,
                    addr,
                    backend.clone(),
                    tls.clone(),
                    telemetry.clone(),
                );
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                error!("secure9p accept error: {e}");
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    backend: NinepBackend,
    tls: Arc<ServerConfig>,
    telemetry: HeartbeatTelemetry,
) {
    if let Err(e) = stream.set_nodelay(true) {
        warn!("secure9p failed to set TCP_NODELAY for {}: {}", addr, e);
    }
    match ServerConnection::new(tls.clone()) {
        Ok(conn) => {
            let mut tls_stream = StreamOwned::new(conn, stream);
            if let Err(e) = tls_stream.conn.complete_io(&mut tls_stream.sock) {
                warn!("secure9p handshake failed for {}: {}", addr, e);
                return;
            }
            log_peer(&tls_stream.conn, addr);
            let registration = telemetry.register(addr.to_string());
            let wrapped = TelemetryStream::new(tls_stream, registration);
            let _ = backend.serve_stream(wrapped);
        }
        Err(e) => warn!("secure9p connection init failed for {}: {}", addr, e),
    }
}

fn log_peer(conn: &ServerConnection, addr: SocketAddr) {
    if let Some(certs) = conn.peer_certificates() {
        if let Some(cert) = certs.first() {
            info!(
                "secure9p TLS client {} presented {} bytes cert",
                addr,
                cert.0.len()
            );
        } else {
            info!("secure9p TLS client {} connected without certificate", addr);
        }
    } else {
        info!("secure9p TLS client {} connected (no cert info)", addr);
    }
}

struct TelemetryStream {
    inner: StreamOwned<ServerConnection, TcpStream>,
    registration: HeartbeatRegistration,
}

impl TelemetryStream {
    fn new(
        inner: StreamOwned<ServerConnection, TcpStream>,
        registration: HeartbeatRegistration,
    ) -> Self {
        registration.touch();
        Self {
            inner,
            registration,
        }
    }
}

impl Read for TelemetryStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buf);
        if let Ok(count) = result {
            if count > 0 {
                self.registration.touch();
            }
        }
        result
    }
}

impl Write for TelemetryStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let result = self.inner.write(buf);
        if let Ok(count) = result {
            if count > 0 {
                self.registration.touch();
            }
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Stream for TelemetryStream {
    fn try_clone(&self) -> ninep::Result<Self> {
        Err("tls streams do not support cloning".to_string())
    }
}

pub fn configure_backend_from_policy(backend: &NinepBackend, cfg: &Secure9pConfig) {
    let outcome = PolicyReconciler::new(cfg).reconcile();
    for event in &outcome.events {
        info!(
            "secure9p reconcile trace_id={} domain={} agent={} {}",
            event.trace_id, event.domain, event.agent, event.message
        );
    }
    for namespace in outcome.namespaces {
        backend.set_namespace(&namespace.agent, namespace.root, namespace.read_only);
    }
    for policy in outcome.policies {
        backend.set_agent_policy(&policy.agent, policy.policy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninep::protocol::{Format9p, Rdata, Rmessage, Tdata, Tmessage};
    use rcgen::{
        Certificate as RcCert, CertificateParams, DistinguishedName, DnType, IsCa, SanType,
    };
    use rustls::{ClientConfig, ClientConnection, ServerName};
    use std::net::TcpStream;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn heartbeat_entries_removed_after_disconnect() {
        let telemetry = HeartbeatTelemetry::new();
        assert_eq!(telemetry.entry_count(), 0);
        {
            let registration = telemetry.register("127.0.0.1:1000".into());
            assert_eq!(telemetry.entry_count(), 1);
            registration.touch();
        }
        telemetry.emit_tick();
        telemetry.emit_tick();
        assert_eq!(telemetry.entry_count(), 0);
    }

    fn write_file(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).expect("write file");
    }

    fn generate_ca() -> RcCert {
        let mut params = CertificateParams::new(vec!["Cohesix CA".into()]);
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.distinguished_name = DistinguishedName::new();
        RcCert::from_params(params).expect("ca cert")
    }

    fn generate_server_cert(ca: &RcCert) -> (String, String) {
        let mut params = CertificateParams::new(vec!["localhost".into()]);
        params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        params
            .subject_alt_names
            .push(SanType::DnsName("localhost".into()));
        let cert = RcCert::from_params(params).expect("server cert");
        let pem = cert.serialize_pem_with_signer(ca).expect("server pem");
        let key = cert.serialize_private_key_pem();
        (pem, key)
    }

    fn generate_client_cert(ca: &RcCert) -> (Vec<u8>, Vec<u8>) {
        let mut params = CertificateParams::new(vec!["tester".into()]);
        params.distinguished_name.push(DnType::CommonName, "tester");
        let cert = RcCert::from_params(params).expect("client cert");
        let der = cert.serialize_der_with_signer(ca).expect("client der");
        (der, cert.serialize_private_key_der())
    }

    #[test]
    fn tls_handshake_and_attach() {
        let tmp = tempdir().expect("temp dir");
        let ca = generate_ca();
        let (server_pem, server_key) = generate_server_cert(&ca);
        let (client_der, client_key) = generate_client_cert(&ca);

        let ca_pem = ca.serialize_pem().expect("ca pem");
        write_file(&tmp.path().join("ca.pem"), ca_pem.as_bytes());
        write_file(&tmp.path().join("server.pem"), server_pem.as_bytes());
        write_file(&tmp.path().join("server.key"), server_key.as_bytes());

        let cfg = Secure9pConfig {
            namespace: vec![],
            policy: vec![],
            port: 0,
            cert: tmp.path().join("server.pem"),
            key: tmp.path().join("server.key"),
            ca_cert: Some(tmp.path().join("ca.pem")),
            require_client_auth: true,
        };
        let backend = NinepBackend::new(tmp.path().to_path_buf(), false).expect("backend");
        let mut server =
            Secure9PServer::new(SecureConfig::from(&cfg), backend.clone()).expect("server");
        server.start().expect("start");
        let port = server.cfg.port;
        assert!(port > 0);

        let mut root_store = RootCertStore::empty();
        root_store
            .add(&Certificate(ca.serialize_der().expect("ca der")))
            .expect("add ca");
        let client_config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_client_auth_cert(vec![Certificate(client_der)], PrivateKey(client_key))
            .expect("client config");

        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let server_name = ServerName::try_from("localhost").expect("server name");
        let conn =
            ClientConnection::new(Arc::new(client_config), server_name).expect("client conn");
        let mut tls_stream = StreamOwned::new(conn, stream);
        tls_stream
            .conn
            .complete_io(&mut tls_stream.sock)
            .expect("handshake");

        let version = Tmessage {
            tag: 0xffff,
            content: Tdata::Version {
                msize: 8192,
                version: "9P2000".to_string(),
            },
        };
        version.write_to(&mut tls_stream).expect("write version");
        let resp = Rmessage::read_from(&mut tls_stream).expect("read version");
        match resp.content {
            Rdata::Version { .. } => {}
            other => panic!("unexpected response: {other:?}"),
        }

        let attach = Tmessage {
            tag: 1,
            content: Tdata::Attach {
                fid: 1,
                afid: u32::MAX,
                uname: "tester".into(),
                aname: "/".into(),
            },
        };
        attach.write_to(&mut tls_stream).expect("write attach");
        let resp = Rmessage::read_from(&mut tls_stream).expect("read attach");
        match resp.content {
            Rdata::Attach { .. } => {}
            other => panic!("unexpected attach resp: {other:?}"),
        }
    }
}
