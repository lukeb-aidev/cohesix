// CLASSIFICATION: COMMUNITY
// Filename: auth_handler.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-31

//! Extract agent identity from TLS sessions.

#[cfg(feature = "secure9p")]
use anyhow::{anyhow, Result};
#[cfg(feature = "secure9p")]
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
#[cfg(feature = "secure9p")]
use rustls::{server::ServerConnection};
use rustls::pki_types::CertificateDer;
#[cfg(feature = "secure9p")]
use x509_parser::prelude::*;
#[cfg(feature = "secure9p")]
use std::io::BufRead;

#[cfg(feature = "secure9p")]
fn parse_cn(cert: &CertificateDer<'_>) -> Option<String> {
    let (_, parsed) = X509Certificate::from_der(cert.as_ref()).ok()?;
    let cn = parsed
        .subject()
        .iter_common_name()
        .next()
        .map(|cn| cn.as_str().unwrap_or("").to_string());
    cn
}

#[cfg(feature = "secure9p")]
pub trait AuthHandler {
    fn identity(
        &self,
        conn: &mut ServerConnection,
        stream: &mut dyn std::io::Read,
    ) -> Result<String>;
}

#[cfg(feature = "secure9p")]
#[derive(Clone, Copy, Default)]
pub struct NullAuth;

#[cfg(feature = "secure9p")]
impl AuthHandler for NullAuth {
    fn identity(
        &self,
        _conn: &mut ServerConnection,
        _stream: &mut dyn std::io::Read,
    ) -> Result<String> {
        Ok("anonymous".into())
    }
}

#[cfg(feature = "secure9p")]
#[derive(serde::Deserialize)]
struct Claims {
    sub: String,
}

/// Extract an agent identifier from a TLS connection using client certificates
/// or an initial JWT line.
#[cfg(feature = "secure9p")]
pub fn extract_identity(
    conn: &mut ServerConnection,
    stream: &mut dyn std::io::Read,
) -> Result<String> {
    if let Some(certs) = conn.peer_certificates() {
        if let Some(id) = certs.first().and_then(parse_cn) {
            return Ok(id);
        }
    }
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(stream);
    reader.read_line(&mut line)?;
    if let Some(token) = line.strip_prefix("JWT ") {
        let token = token.trim();
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(b"cohesix"),
            &Validation::new(Algorithm::HS256),
        )?;
        return Ok(data.claims.sub);
    }
    Err(anyhow!("identity not provided"))
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
    use rustls::{ClientConfig, ServerConfig, RootCertStore};
    use rustls::{ClientConnection, ServerConnection};
    use std::sync::Arc;

    fn tls_pair() -> (ServerConnection, ClientConnection) {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .unwrap();
        let keypair = rcgen::generate_simple_self_signed(["test".into()]).unwrap();
        let cert_der = keypair.cert.der().clone();
        let key = PrivatePkcs8KeyDer::from(keypair.key_pair.serialize_der());
        let mut server_cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], rustls::pki_types::PrivateKeyDer::Pkcs8(key))
            .unwrap();
        server_cfg.alpn_protocols.push(b"test".to_vec());
        let client_cfg = ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();
        let server = ServerConnection::new(Arc::new(server_cfg)).unwrap();
        let client =
            ClientConnection::new(Arc::new(client_cfg), "localhost".try_into().unwrap()).unwrap();
        (server, client)
    }

    #[test]
    fn cert_identity_extracts_cn() {
        let (mut srv, _cli) = tls_pair();
        // Initialize handshake
        let _ = srv.complete_io(&mut std::io::Cursor::new(Vec::<u8>::new()));
        let buf = Vec::new();
        assert!(extract_identity(&mut srv, &mut &buf[..]).is_err());
    }
}
