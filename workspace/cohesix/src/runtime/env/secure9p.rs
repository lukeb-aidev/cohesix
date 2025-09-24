// CLASSIFICATION: COMMUNITY
// Filename: secure9p.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-12-31

use cohesix_9p::NinepBackend;
use cohesix_secure9p::config::Secure9pConfig;
use cohesix_secure9p::tls_server::{configure_backend_from_policy, Secure9PServer, SecureConfig};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::{env, io};

static SECURE9P_SERVER: OnceCell<Secure9PServer> = OnceCell::new();

pub fn initialize_from_boot() -> io::Result<()> {
    let path = env::var("SECURE9P_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/secure9p.toml"));
    let cfg = Secure9pConfig::load(&path)?;
    let backend = NinepBackend::new(PathBuf::from("/"), false)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    configure_backend_from_policy(&backend, &cfg);
    let mut server = Secure9PServer::new(SecureConfig::from(&cfg), backend)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    server.start()?;
    SECURE9P_SERVER
        .set(server)
        .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "secure9p already initialized"))
}
