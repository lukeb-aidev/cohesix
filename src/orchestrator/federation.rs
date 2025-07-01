// CLASSIFICATION: COMMUNITY
// Filename: federation.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::prelude::*;
/// Queen federation utilities.
//
/// Queens announce themselves via `/srv/federation/beacon` and
/// exchange state with mutual authentication derived from a
/// shared federation key.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use sha2::Sha256;
use hkdf::Hkdf;

/// Summary: federation helper
pub struct Federation {
    queen_id: String,
}

impl Federation {
    /// Create a new federation helper.
    pub fn new(queen_id: &str) -> Self {
        Self { queen_id: queen_id.into() }
    }

    /// Announce this Queen to peers.
    pub fn announce(&self) -> anyhow::Result<()> {
        fs::create_dir_all("/srv/federation")?;
        fs::write("/srv/federation/beacon", self.queen_id.as_bytes())?;
        self.log_event("announce")?;
        Ok(())
    }

    /// Propagate a shared namespace directory to peers.
    pub fn propagate_namespace(&self, path: &str) -> anyhow::Result<()> {
        let tgt = format!("/srv/federation/shared/{}", self.queen_id);
        fs::create_dir_all(&tgt)?;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let dest = format!("{}/{}", tgt, entry.file_name().to_string_lossy());
            if entry.path().is_file() {
                fs::copy(entry.path(), dest).ok();
            }
        }
        self.log_event("propagate")?;
        Ok(())
    }

    /// Establish secure link with another queen using `queen_federation.key`.
    pub fn establish_secure(&self, peer_id: &str) -> anyhow::Result<()> {
        let key = fs::read("/boot/queen_federation.key")?;
        let hk = Hkdf::<Sha256>::new(None, &key);
        let mut digest = [0u8; 32];
        hk.expand(b"cohesix-federation", &mut digest)
            .map_err(|_| anyhow::anyhow!("hkdf expand"))?;
        let path = format!("/srv/federation/{}.auth", peer_id);
        fs::write(&path, &digest)?;
        self.log_event("secure_link")?;
        Ok(())
    }

    fn log_event(&self, msg: &str) -> anyhow::Result<()> {
        fs::create_dir_all("/srv/federation")?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/federation/events.log")?;
        writeln!(f, "{} {} {}", timestamp(), self.queen_id, msg)?;
        Ok(())
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
