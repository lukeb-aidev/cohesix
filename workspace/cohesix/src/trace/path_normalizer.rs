// CLASSIFICATION: COMMUNITY
// Filename: path_normalizer.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-03-08

//! Trace path normalization and enforcement utilities.
//!
//! Loads `/etc/cohtrace_rules.json` (or `COHTRACE_RULES_PATH`) and validates
//! that trace file operations remain within approved directories. Rules may
//! optionally rewrite legacy prefixes to canonical ones. All parsing performs
//! schema validation to prevent partially configured rule sets from being
//! accepted at runtime.

extern crate alloc;

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_RULE_PATH: &str = "/etc/cohtrace_rules.json";
const RULE_PATH_ENV: &str = "COHTRACE_RULES_PATH";
const MAX_RULES: usize = 64;

/// Errors produced while loading or enforcing path rules.
#[derive(Debug)]
pub enum PathRuleError {
    /// Rule file could not be read.
    Io(std::io::Error),
    /// Schema validation failed.
    InvalidSchema(String),
    /// A path violates configured rules.
    RuleViolation(String),
}

impl core::fmt::Display for PathRuleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PathRuleError::Io(err) => write!(f, "failed to read cohtrace rules: {err}"),
            PathRuleError::InvalidSchema(msg) => {
                write!(f, "cohtrace rules schema violation: {msg}")
            }
            PathRuleError::RuleViolation(msg) => write!(f, "cohtrace rule violation: {msg}"),
        }
    }
}

impl std::error::Error for PathRuleError {}

#[derive(Deserialize, Serialize)]
struct RawRules {
    allowed_roots: Vec<String>,
    #[serde(default)]
    rewrites: Vec<RawRewrite>,
}

#[derive(Deserialize, Serialize)]
struct RawRewrite {
    from: String,
    to: String,
}

#[derive(Clone, Debug)]
struct RewriteRule {
    from: PathBuf,
    to: PathBuf,
}

/// Normalizes trace paths according to loaded rules.
#[derive(Clone, Debug)]
pub struct PathNormalizer {
    allowed_roots: Vec<PathBuf>,
    rewrites: Vec<RewriteRule>,
}

impl PathNormalizer {
    /// Load the global path normalizer, caching the parsed rule set.
    pub fn global() -> Result<&'static PathNormalizer, PathRuleError> {
        static CACHE: OnceLock<PathNormalizer> = OnceLock::new();
        if let Some(norm) = CACHE.get() {
            return Ok(norm);
        }
        let instance = PathNormalizer::load()?;
        let _ = CACHE.set(instance);
        CACHE
            .get()
            .ok_or_else(|| PathRuleError::InvalidSchema("failed to initialize rules cache".into()))
    }

    /// Load rules from disk, validating schema and paths.
    pub fn load() -> Result<PathNormalizer, PathRuleError> {
        let rules_path = env::var(RULE_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_RULE_PATH));
        let contents = fs::read_to_string(&rules_path).map_err(PathRuleError::Io)?;
        let raw: RawRules = serde_json::from_str(&contents)
            .map_err(|e| PathRuleError::InvalidSchema(format!("invalid JSON: {e}")))?;
        validate_raw_rules(&raw)?;
        let allowed_roots = raw
            .allowed_roots
            .iter()
            .map(|root| canonicalize_lossy(root))
            .collect();
        let rewrites = raw
            .rewrites
            .iter()
            .map(|r| RewriteRule {
                from: canonicalize_lossy(&r.from),
                to: canonicalize_lossy(&r.to),
            })
            .collect();
        Ok(PathNormalizer {
            allowed_roots,
            rewrites,
        })
    }

    /// Normalize and validate the provided path. Returns an absolute path
    /// anchored within one of the allowed roots.
    pub fn normalize<P: AsRef<Path>>(&self, input: P) -> Result<PathBuf, PathRuleError> {
        let absolute = absolutize(input.as_ref());
        let canonical = match fs::canonicalize(&absolute) {
            Ok(real) => real,
            Err(_) => absolute,
        };
        let rewritten = self.apply_rewrites(&canonical);
        for root in &self.allowed_roots {
            if rewritten.starts_with(root) {
                return Ok(rewritten);
            }
        }
        Err(PathRuleError::RuleViolation(format!(
            "{} is outside permitted trace roots",
            rewritten.display()
        )))
    }

    fn apply_rewrites(&self, path: &Path) -> PathBuf {
        for rule in &self.rewrites {
            if path.starts_with(&rule.from) {
                if let Ok(rest) = path.strip_prefix(&rule.from) {
                    return clean_join(&rule.to, rest);
                }
            }
        }
        path.to_path_buf()
    }
}

fn validate_raw_rules(raw: &RawRules) -> Result<(), PathRuleError> {
    if raw.allowed_roots.is_empty() {
        return Err(PathRuleError::InvalidSchema(
            "allowed_roots must contain at least one entry".into(),
        ));
    }
    if raw.allowed_roots.len() > MAX_RULES {
        return Err(PathRuleError::InvalidSchema(format!(
            "allowed_roots exceeds max size ({MAX_RULES})"
        )));
    }
    for root in &raw.allowed_roots {
        if !root.starts_with('/') {
            return Err(PathRuleError::InvalidSchema(format!(
                "allowed root must be absolute: {root}"
            )));
        }
    }
    if raw.rewrites.len() > MAX_RULES {
        return Err(PathRuleError::InvalidSchema(format!(
            "rewrites exceeds max size ({MAX_RULES})"
        )));
    }
    for rewrite in &raw.rewrites {
        if !rewrite.from.starts_with('/') || !rewrite.to.starts_with('/') {
            return Err(PathRuleError::InvalidSchema(format!(
                "rewrite entries must use absolute paths: {} -> {}",
                rewrite.from, rewrite.to
            )));
        }
    }
    Ok(())
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return clean_path(path);
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    clean_join(&cwd, path)
}

fn clean_join(base: &Path, tail: &Path) -> PathBuf {
    let mut joined = base.to_path_buf();
    if !tail.as_os_str().is_empty() {
        joined.push(tail);
    }
    clean_path(&joined)
}

fn clean_path(path: &Path) -> PathBuf {
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut absolute = path.is_absolute();
    for component in path.components() {
        match component {
            Component::RootDir => {
                absolute = true;
                stack.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop();
            }
            Component::Normal(part) => stack.push(PathBuf::from(part)),
            Component::Prefix(prefix) => {
                // Windows prefixes propagate as-is.
                absolute = true;
                stack.clear();
                stack.push(PathBuf::from(prefix.as_os_str()));
            }
        }
    }
    let mut out = PathBuf::new();
    if absolute {
        out.push(Path::new("/"));
    }
    for part in stack {
        out.push(part);
    }
    out
}

fn canonicalize_lossy(path: &str) -> PathBuf {
    let cleaned = clean_path(Path::new(path));
    if cleaned.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn load_and_normalize() {
        let tmp = tempdir().unwrap();
        let allow = tmp.path().join("history");
        fs::create_dir_all(&allow).unwrap();
        let rules = RawRules {
            allowed_roots: vec![allow.to_string_lossy().to_string()],
            rewrites: vec![RawRewrite {
                from: "/legacy".into(),
                to: allow.to_string_lossy().to_string(),
            }],
        };
        let file = tmp.path().join("rules.json");
        let mut writer = fs::File::create(&file).unwrap();
        write!(writer, "{}", serde_json::to_string(&rules).unwrap()).unwrap();
        env::set_var(RULE_PATH_ENV, &file);
        let normalizer = PathNormalizer::load().unwrap();
        let normalized = normalizer
            .normalize(allow.join("snap.json"))
            .expect("normalize");
        assert!(normalized.starts_with(&allow));
        let legacy = normalizer
            .normalize(Path::new("/legacy/snap.json"))
            .expect("rewrite");
        assert!(legacy.starts_with(&allow));
        env::remove_var(RULE_PATH_ENV);
    }

    #[test]
    fn detects_rule_violation() {
        let tmp = tempdir().unwrap();
        let allow = tmp.path().join("history");
        fs::create_dir_all(&allow).unwrap();
        let rules = RawRules {
            allowed_roots: vec![allow.to_string_lossy().to_string()],
            rewrites: Vec::new(),
        };
        let file = tmp.path().join("rules.json");
        fs::write(&file, serde_json::to_vec(&rules).unwrap()).unwrap();
        env::set_var(RULE_PATH_ENV, &file);
        let normalizer = PathNormalizer::load().unwrap();
        let err = normalizer.normalize(Path::new("/etc/passwd")).unwrap_err();
        assert!(matches!(err, PathRuleError::RuleViolation(_)));
        env::remove_var(RULE_PATH_ENV);
    }

    #[test]
    fn validates_schema() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("rules.json");
        fs::write(&file, "{\"allowed_roots\": []}").unwrap();
        env::set_var(RULE_PATH_ENV, &file);
        let err = PathNormalizer::load().unwrap_err();
        assert!(matches!(err, PathRuleError::InvalidSchema(_)));
        env::remove_var(RULE_PATH_ENV);
    }
}
