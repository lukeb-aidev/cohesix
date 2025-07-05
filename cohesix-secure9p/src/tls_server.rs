// CLASSIFICATION: COMMUNITY
// Filename: tls_server.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rustls_pemfile::{certs, pkcs8_private_keys};
use webpki_roots::TLS_SERVER_ROOTS;
use ninep::server::{FsServer, InMemoryFs, FsConfig};
use ninep::Stream as NinepStream;

/// Stream wrapper implementing `ninep::Stream` for TLS connections.
pub struct TlsStream {
    inner: StreamOwned<ServerConnection, TcpStream>,
}

impl TlsStream {
    pub fn new(conn: StreamOwned<ServerConnection, TcpStream>) -> Self {
        Self { inner: conn }
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl NinepStream for TlsStream {
    fn try_clone(&self) -> ninep::Result<Self> {
        let conn = self.inner.get_ref().try_clone()?;
        let server = self.inner.conn.clone();
        Ok(Self { inner: StreamOwned::new(server, conn) })
    }
}

/// Server configuration loaded from disk.
pub struct SecureConfig {
    pub port: u16,
    pub cert: String,
    pub key: String,
}

pub struct Secure9PServer {
    cfg: SecureConfig,
}

impl Secure9PServer {
    pub fn new(cfg: SecureConfig) -> Self {
        Self { cfg }
    }

    pub fn run(&self) -> std::io::Result<()> {
        let mut cert_file = File::open(&self.cfg.cert)?;
        let mut key_file = File::open(&self.cfg.key)?;
        let mut cert_buf = Vec::new();
        let mut key_buf = Vec::new();
        cert_file.read_to_end(&mut cert_buf)?;
        key_file.read_to_end(&mut key_buf)?;
        let certs = certs(&mut &cert_buf[..])?.into_iter().map(rustls::Certificate).collect();
        let mut keys = pkcs8_private_keys(&mut &key_buf[..])?;
        let key = rustls::PrivateKey(keys.remove(0));
        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_client_cert_verifier(rustls::server::AllowAnyAnonymousOrAuthenticatedClient::new(TLS_SERVER_ROOTS.0.clone()))
            .with_single_cert(certs, key)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let listener = TcpListener::bind(("0.0.0.0", self.cfg.port))?;
        for stream in listener.incoming() {
            match stream {
                Ok(tcp) => {
                    let cfg = Arc::new(config.clone());
                    std::thread::spawn(move || {
                        if let Err(e) = handle_client(tcp, cfg) {
                            eprintln!("secure9p client error: {e}");
                        }
                    });
                }
                Err(e) => eprintln!("incoming connection failed: {e}"),
            }
        }
        Ok(())
    }
}

fn handle_client(tcp: TcpStream, cfg: Arc<ServerConfig>) -> std::io::Result<()> {
    let mut conn = ServerConnection::new(cfg).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let mut stream = StreamOwned::new(conn, tcp);
    let mut tls_stream = TlsStream::new(stream);
    let fs_server = FsServer::new(FsConfig { root: "/".into(), port: 0, readonly: false });
    fs_server.serve_stream(tls_stream).join().expect("thread join");
    Ok(())
}
