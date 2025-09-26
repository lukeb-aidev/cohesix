// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.2
// Author: Lukas Bower
// Date Modified: 2029-09-26

use crate::manifest::ManifestSignature;
use crate::reconcile::PolicyReconciler;
use cohesix_9p::policy::SandboxPolicy;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct NamespaceEntry {
    pub agent: String,
    pub root: PathBuf,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PolicyEntry {
    pub agent: String,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Secure9pConfig {
    #[serde(default)]
    pub namespace: Vec<NamespaceEntry>,
    #[serde(default)]
    pub policy: Vec<PolicyEntry>,
    pub port: u16,
    pub cert: PathBuf,
    pub key: PathBuf,
    #[serde(default)]
    pub ca_cert: Option<PathBuf>,
    #[serde(default)]
    pub require_client_auth: bool,
}

impl Secure9pConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        ManifestSignature::verify_manifest(path, text.as_bytes())?;
        let mut cfg: Secure9pConfig =
            toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        cfg.rebase_paths(path);
        Ok(cfg)
    }

    fn rebase_paths(&mut self, path: &Path) {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        self.cert = Self::join_base(base, &self.cert);
        self.key = Self::join_base(base, &self.key);
        if let Some(ca) = &mut self.ca_cert {
            *ca = Self::join_base(base, ca);
        }
        for ns in &mut self.namespace {
            ns.root = Self::join_base(base, &ns.root);
        }
    }

    fn join_base(base: &Path, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    }

    pub fn agent_policies(&self) -> BTreeMap<String, SandboxPolicy> {
        PolicyReconciler::new(self)
            .reconcile()
            .policies
            .into_iter()
            .map(|resolved| (resolved.agent, resolved.policy))
            .collect()
    }
}

impl PolicyEntry {
    pub(crate) fn to_policy(&self) -> SandboxPolicy {
        let mut read = Vec::new();
        let mut write = Vec::new();
        for rule in &self.allow {
            let mut parts = rule.splitn(2, ':');
            let verb = parts.next().unwrap_or_default();
            let path = match parts.next() {
                Some(p) if !p.is_empty() => p.trim().to_string(),
                _ => continue,
            };
            match verb {
                "read" => read.push(path),
                "write" => write.push(path),
                _ => continue,
            }
        }
        SandboxPolicy { read, write }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn signed_manifest(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let manifest_path = dir.path().join("secure9p.toml");
        fs::write(&manifest_path, contents).expect("write manifest");
        let signature = ManifestSignature::compute(contents.as_bytes());
        signature
            .write(
                &ManifestSignature::signature_path(&manifest_path),
                Some("// Generated for test"),
            )
            .expect("write signature");
        (dir, manifest_path)
    }

    #[test]
    fn load_requires_signature_file() {
        let dir = tempdir().expect("tempdir");
        let manifest_path = dir.path().join("secure9p.toml");
        fs::write(&manifest_path, "port = 1\ncert = 'a'\nkey = 'b'\n").unwrap();
        let err = Secure9pConfig::load(&manifest_path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn load_rejects_mismatched_signature() {
        let (_dir, manifest_path) =
            signed_manifest("port = 9\ncert = 'a'\nkey = 'b'\nrequire_client_auth = false\n");
        fs::write(
            ManifestSignature::signature_path(&manifest_path),
            "sha512:0000",
        )
        .unwrap();
        let err = Secure9pConfig::load(&manifest_path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn load_accepts_valid_signature() {
        let (_dir, manifest_path) =
            signed_manifest("port = 10\ncert = 'cert.pem'\nkey = 'key.pem'\n");
        let cfg = Secure9pConfig::load(&manifest_path).expect("load config");
        assert_eq!(cfg.port, 10);
    }
}
