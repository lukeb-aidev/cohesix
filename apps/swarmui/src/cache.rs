// Author: Lukas Bower
// Purpose: Provide bounded CBOR snapshot caching for SwarmUI offline inspection.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const SNAPSHOT_VERSION: u8 = 1;
const MAX_KEY_LEN: usize = 96;

/// Cache errors surfaced by snapshot operations.
#[derive(Debug)]
pub enum CacheError {
    /// Snapshot key is invalid or unsafe.
    InvalidKey(String),
    /// Cache is disabled.
    Disabled,
    /// Snapshot exceeds configured size bounds.
    TooLarge {
        /// Actual snapshot size in bytes.
        actual: usize,
        /// Maximum snapshot size allowed by configuration.
        max: usize,
    },
    /// Snapshot has expired.
    Expired,
    /// Underlying I/O error.
    Io(io::Error),
    /// CBOR decode failure.
    Decode(String),
    /// Snapshot version mismatch.
    Version(u8),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::InvalidKey(key) => write!(f, "invalid snapshot key '{key}'"),
            CacheError::Disabled => write!(f, "snapshot cache disabled"),
            CacheError::TooLarge { actual, max } => {
                write!(f, "snapshot too large ({actual} > {max} bytes)")
            }
            CacheError::Expired => write!(f, "snapshot expired"),
            CacheError::Io(err) => write!(f, "cache io error: {err}"),
            CacheError::Decode(err) => write!(f, "cache decode error: {err}"),
            CacheError::Version(version) => write!(f, "snapshot version {version} unsupported"),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<io::Error> for CacheError {
    fn from(err: io::Error) -> Self {
        CacheError::Io(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotEntry {
    version: u8,
    created_ms: u64,
    expires_ms: u64,
    payload: Vec<u8>,
}

/// Snapshot metadata returned to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    /// Timestamp when the snapshot was captured.
    pub created_ms: u64,
    /// Timestamp when the snapshot expires.
    pub expires_ms: u64,
    /// CBOR payload bytes.
    pub payload: Vec<u8>,
}

/// Bounded snapshot cache rooted at `$DATA_DIR/snapshots`.
#[derive(Debug, Clone)]
pub struct SnapshotCache {
    root: PathBuf,
    max_bytes: usize,
    ttl: Duration,
}

impl SnapshotCache {
    /// Create a new snapshot cache with size and TTL bounds.
    pub fn new(root: PathBuf, max_bytes: usize, ttl: Duration) -> Self {
        Self {
            root,
            max_bytes,
            ttl,
        }
    }

    /// Return the cache root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the maximum size allowed for a snapshot file.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Write a snapshot payload to cache with bounded size.
    pub fn write(&self, key: &str, payload: &[u8]) -> Result<SnapshotRecord, CacheError> {
        let path = self.snapshot_path(key)?;
        fs::create_dir_all(&self.root)?;
        let created_ms = now_ms();
        let expires_ms = created_ms.saturating_add(self.ttl.as_millis() as u64);
        let entry = SnapshotEntry {
            version: SNAPSHOT_VERSION,
            created_ms,
            expires_ms,
            payload: payload.to_vec(),
        };
        let encoded =
            serde_cbor::to_vec(&entry).map_err(|err| CacheError::Decode(err.to_string()))?;
        if encoded.len() > self.max_bytes {
            return Err(CacheError::TooLarge {
                actual: encoded.len(),
                max: self.max_bytes,
            });
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, encoded)?;
        fs::rename(&tmp, &path)?;
        Ok(SnapshotRecord {
            created_ms,
            expires_ms,
            payload: payload.to_vec(),
        })
    }

    /// Read a snapshot payload from cache, rejecting expired entries.
    pub fn read(&self, key: &str) -> Result<SnapshotRecord, CacheError> {
        let path = self.snapshot_path(key)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() as usize > self.max_bytes {
            return Err(CacheError::TooLarge {
                actual: metadata.len() as usize,
                max: self.max_bytes,
            });
        }
        let bytes = fs::read(&path)?;
        let entry: SnapshotEntry =
            serde_cbor::from_slice(&bytes).map_err(|err| CacheError::Decode(err.to_string()))?;
        if entry.version != SNAPSHOT_VERSION {
            return Err(CacheError::Version(entry.version));
        }
        let now = now_ms();
        if now > entry.expires_ms {
            let _ = fs::remove_file(&path);
            return Err(CacheError::Expired);
        }
        Ok(SnapshotRecord {
            created_ms: entry.created_ms,
            expires_ms: entry.expires_ms,
            payload: entry.payload,
        })
    }

    fn snapshot_path(&self, key: &str) -> Result<PathBuf, CacheError> {
        let key = sanitize_key(key)?;
        Ok(self.root.join(format!("{key}.cbor")))
    }
}

fn sanitize_key(key: &str) -> Result<String, CacheError> {
    let trimmed = key.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_KEY_LEN {
        return Err(CacheError::InvalidKey(trimmed.to_owned()));
    }
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
            continue;
        }
        return Err(CacheError::InvalidKey(trimmed.to_owned()));
    }
    Ok(trimmed.to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
