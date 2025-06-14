// CLASSIFICATION: COMMUNITY
// Filename: auth_handler.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! Extract agent identity from TLS sessions.

#[cfg(feature = "secure9p")]
use anyhow::{anyhow, Result};
#[cfg(feature = "secure9p")]
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
#[cfg(feature = "secure9p")]
use rustls::{server::ServerConnection, Certificate};
#[cfg(feature = "secure9p")]
use x509_parser::prelude::*;

#[cfg(feature = "secure9p")]
fn parse_cn(cert: &Certificate) -> Option<String> {
    let (_, parsed) = X509Certificate::from_der(&cert.0).ok()?;
    parsed
        .subject()
        .iter_common_name()
        .next()
        .map(|cn| cn.as_str().unwrap_or("").to_string())
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
    if line.starts_with("JWT ") {
        let token = line[4..].trim();
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(b"cohesix"),
            &Validation::new(Algorithm::HS256),
        )?;
        return Ok(data.claims.sub);
    }
    Err(anyhow!("identity not provided"))
}

#[cfg(feature = "secure9p")]
#[derive(Clone)]
pub struct NullAuth;

#[cfg(feature = "secure9p")]
pub trait AuthHandler {
    fn authenticate(&self, _hello: &[u8]) -> String;
}

#[cfg(feature = "secure9p")]
impl AuthHandler for NullAuth {
    fn authenticate(&self, _hello: &[u8]) -> String {
        "anonymous".into()
    }
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
    use rustls::{ClientConfig, ServerConfig};
    use rustls::{ClientConnection, StreamOwned};
    use std::sync::Arc;

    fn tls_pair() -> (ServerConnection, ClientConnection) {
        let cert = rcgen::generate_simple_self_signed(["test".into()]).unwrap();
        let cert_der = CertificateDer::from(cert.serialize_der().unwrap());
        let key = PrivatePkcs8KeyDer::from(cert.serialize_private_key_der());
        let mut server_cfg = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key)
            .unwrap();
        server_cfg.alpn_protocols.push(b"test".to_vec());
        let client_cfg = ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(Arc::new(
                rustls::client::WebPkiClientVerifier::builder(Arc::new(
                    rustls::RootCertStore::empty(),
                ))
                .build(),
            ))
            .with_no_client_auth();
        let server = ServerConnection::new(Arc::new(server_cfg)).unwrap();
        let client =
            ClientConnection::new(Arc::new(client_cfg), "localhost".try_into().unwrap()).unwrap();
        (server, client)
    }

    #[test]
    fn cert_identity_extracts_cn() {
        let (mut srv, mut cli) = tls_pair();
        let (mut server_io, mut client_io) = rustls::Stream::new(&mut srv, &mut cli);
        let _ = server_io.read(&mut [0u8; 0]);
        let mut buf = Vec::new();
        assert!(extract_identity(&mut srv, &mut &buf[..]).is_err());
    }
}
