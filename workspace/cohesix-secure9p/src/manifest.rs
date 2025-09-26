// CLASSIFICATION: COMMUNITY
// Filename: manifest.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-09-26

use sha2::{Digest, Sha512};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use subtle::ConstantTimeEq;

const SIGNATURE_EXTENSION: &str = "sha512";
const ALGORITHM: &str = "sha512";

/// Represents a Secure9P manifest signature record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSignature {
    pub algorithm: String,
    pub digest: String,
}

impl ManifestSignature {
    /// Compute a manifest signature from raw manifest bytes.
    pub fn compute(data: &[u8]) -> Self {
        let mut hasher = Sha512::new();
        hasher.update(data);
        let digest = hasher.finalize();
        Self {
            algorithm: ALGORITHM.to_string(),
            digest: hex::encode(digest),
        }
    }

    /// Returns the canonical signature file path for a manifest.
    pub fn signature_path(manifest_path: &Path) -> PathBuf {
        let mut out = manifest_path.to_path_buf();
        out.set_extension(SIGNATURE_EXTENSION);
        out
    }

    /// Load a manifest signature from disk.
    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        parse_signature(&contents).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid manifest signature format in {}", path.display()),
            )
        })
    }

    /// Write the signature to disk using a simple record format.
    pub fn write(&self, path: &Path, header: Option<&str>) -> io::Result<()> {
        let mut body = String::new();
        if let Some(prefix) = header {
            body.push_str(prefix);
            if !prefix.ends_with('\n') {
                body.push('\n');
            }
        }
        body.push_str(&self.encode_record());
        if !body.ends_with('\n') {
            body.push('\n');
        }
        fs::write(path, body)
    }

    /// Encode the signature as a single record line.
    pub fn encode_record(&self) -> String {
        format!("{}:{}", self.algorithm, self.digest)
    }

    /// Verify that the signature matches the manifest contents.
    pub fn verify_bytes(&self, data: &[u8]) -> io::Result<()> {
        if self.algorithm.to_lowercase() != ALGORITHM {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported manifest signature algorithm: {}",
                    self.algorithm
                ),
            ));
        }
        let expected = ManifestSignature::compute(data);
        if bool::from(self.digest.as_bytes().ct_eq(expected.digest.as_bytes())) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "manifest signature mismatch",
            ))
        }
    }

    /// Convenience helper that validates the manifest contents against the stored signature.
    pub fn verify_manifest(manifest_path: &Path, data: &[u8]) -> io::Result<()> {
        let signature_path = ManifestSignature::signature_path(manifest_path);
        let signature = ManifestSignature::load(&signature_path)?;
        signature.verify_bytes(data)
    }
}

fn parse_signature(text: &str) -> Option<ManifestSignature> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let (algorithm, digest) = if let Some(idx) = trimmed.find(':') {
            (&trimmed[..idx], trimmed[idx + 1..].trim())
        } else if let Some(idx) = trimmed.find('=') {
            (&trimmed[..idx], trimmed[idx + 1..].trim())
        } else {
            return None;
        };
        let algorithm = algorithm.trim().to_lowercase();
        let digest = digest.trim();
        if algorithm != ALGORITHM {
            return None;
        }
        if digest.len() != 128 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return Some(ManifestSignature {
            algorithm,
            digest: digest.to_lowercase(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn signature_round_trip() {
        let data = b"namespace test";
        let sig = ManifestSignature::compute(data);
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.sha512");
        sig.write(&path, Some("# test")).expect("write signature");
        let loaded = ManifestSignature::load(&path).expect("load signature");
        assert_eq!(sig.digest, loaded.digest);
        loaded.verify_bytes(data).expect("verify");
    }

    #[test]
    fn rejects_wrong_digest() {
        let data = b"hello";
        let sig = ManifestSignature::compute(data);
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.sha512");
        sig.write(&path, None).expect("write signature");
        let loaded = ManifestSignature::load(&path).expect("load signature");
        let err = loaded.verify_bytes(b"different").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn parse_skips_comments() {
        let digest = "0".repeat(128);
        let text = format!("// comment\n# note\nsha512:{}\n", digest);
        assert!(parse_signature(&text).is_some());
    }
}
