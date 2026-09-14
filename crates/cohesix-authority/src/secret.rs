// Author: Lukas Bower
// Purpose: Resolve explicit bounded secret sources without fallback after a selected source fails.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::{string::String, string::ToString};
use core::fmt;
use std::{env, fs::File, io::Read, path::Path};

/// Secret sources are bounded before reading caller-controlled files.
pub const MAX_SECRET_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretError {
    InvalidReference,
    Unavailable,
    InvalidValue,
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidReference => {
                "invalid secret reference; expected env:NAME or file:/absolute/path"
            }
            Self::Unavailable => "selected secret source is unavailable",
            Self::InvalidValue => "secret is empty, oversized, malformed, or a placeholder",
        })
    }
}

impl std::error::Error for SecretError {}

/// Validate syntax without inspecting the environment or private file contents.
pub fn validate_reference(reference: &str) -> Result<(), SecretError> {
    if let Some(name) = reference.strip_prefix("env:") {
        if !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        {
            return Ok(());
        }
    } else if let Some(path) = reference.strip_prefix("file:") {
        if !path.is_empty()
            && path.len() <= 1024
            && Path::new(path).is_absolute()
            && !path.bytes().any(|b| b.is_ascii_control())
            && !Path::new(path)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Ok(());
        }
    }
    Err(SecretError::InvalidReference)
}

/// Resolve exactly the selected source. Explicit references take precedence
/// over compatibility literals at their callers; failure never tries another source.
pub fn resolve_reference(reference: &str) -> Result<String, SecretError> {
    validate_reference(reference)?;
    if let Some(name) = reference.strip_prefix("env:") {
        let value = env::var(name).map_err(|_| SecretError::Unavailable)?;
        return validate_value(&value);
    }
    let path = reference
        .strip_prefix("file:")
        .ok_or(SecretError::InvalidReference)?;
    if !std::fs::metadata(path)
        .map_err(|_| SecretError::Unavailable)?
        .is_file()
    {
        return Err(SecretError::InvalidReference);
    }
    let file = File::open(path).map_err(|_| SecretError::Unavailable)?;
    if !file
        .metadata()
        .map_err(|_| SecretError::Unavailable)?
        .is_file()
    {
        return Err(SecretError::InvalidReference);
    }
    let mut value = String::new();
    file.take((MAX_SECRET_BYTES + 1) as u64)
        .read_to_string(&mut value)
        .map_err(|_| SecretError::InvalidValue)?;
    validate_value(&value)
}

/// Resolve an explicit reference or validate a compatibility literal.
pub fn resolve_value(value: &str) -> Result<String, SecretError> {
    if value.starts_with("env:") || value.starts_with("file:") {
        resolve_reference(value)
    } else {
        validate_value(value)
    }
}

/// Reject live placeholders consistently across console and host clients.
pub use crate::is_placeholder;

pub fn validate_value(value: &str) -> Result<String, SecretError> {
    if value.len() > MAX_SECRET_BYTES {
        return Err(SecretError::InvalidValue);
    }
    let value = value.trim();
    if value.is_empty()
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        || is_placeholder(value)
    {
        return Err(SecretError::InvalidValue);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_syntax_does_not_accept_fallbacks_or_relative_files() {
        for reference in [
            "secret",
            "env:",
            "env:2BAD",
            "file:relative",
            "file:/a/../b",
            "env:A\nB",
        ] {
            assert_eq!(
                validate_reference(reference),
                Err(SecretError::InvalidReference)
            );
        }
        assert_eq!(validate_reference("env:COH_TICKET_KEY"), Ok(()));
        assert_eq!(validate_reference("file:/run/secrets/ticket"), Ok(()));
    }

    #[test]
    fn live_values_reject_known_placeholders_and_framing_injection() {
        for value in [
            "",
            "CHANGEME",
            "bootstrap",
            "worker-gpu",
            "real\nAUTH other",
            "a b",
        ] {
            assert_eq!(validate_value(value), Err(SecretError::InvalidValue));
        }
        assert_eq!(
            validate_value("unique-deployment-token\n"),
            Ok("unique-deployment-token".to_string())
        );
    }
}
