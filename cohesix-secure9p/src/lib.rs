// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use std::sync::Arc;
use cohesix_9p::{FsConfig, FsServer, InProcessStream};
use rustls::{ClientConfig, ClientConnection, ServerConfig, ServerConnection, StreamOwned};
use rcgen::generate_simple_self_signed;
use anyhow::Result;

struct NoVerifier;
impl rustls::client::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn tls_pair() -> Result<(
    StreamOwned<ServerConnection, InProcessStream>,
    StreamOwned<ClientConnection, InProcessStream>,
)> {
    let (srv_raw, cli_raw) = InProcessStream::pair();
    let cert = generate_simple_self_signed(["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let key_der = cert.serialize_private_key_der();

    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![rustls::Certificate(cert_der.clone())], rustls::PrivateKey(key_der))?;

    let client_cfg = ClientConfig::builder()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();

    let server_conn = ServerConnection::new(Arc::new(server_cfg))?;
    let client_conn = ClientConnection::new(Arc::new(client_cfg), "localhost".try_into().unwrap())?;

    let mut server_stream = StreamOwned::new(server_conn, srv_raw);
    let mut client_stream = StreamOwned::new(client_conn, cli_raw);

    while server_stream.conn.is_handshaking() || client_stream.conn.is_handshaking() {
        let _ = client_stream.conn.complete_io(&mut client_stream.sock);
        let _ = server_stream.conn.complete_io(&mut server_stream.sock);
    }

    Ok((server_stream, client_stream))
}

/// Start a Secure9P server and return the client TLS stream.
pub fn start_secure9p(cfg: FsConfig) -> Result<StreamOwned<ClientConnection, InProcessStream>> {
    let mut server = FsServer::new(cfg);
    let (srv_tls, cli_tls) = tls_pair()?;
    server.start_on_stream(srv_tls)?;
    log::info!("[Secure9P] TLS initialized over in-process transport");
    Ok(cli_tls)
}

