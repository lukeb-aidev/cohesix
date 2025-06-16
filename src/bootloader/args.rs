// CLASSIFICATION: COMMUNITY
// Filename: args.rs v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Bootloader Argument Parser
//
// Parses a simple key=value command‑line string passed by the
// first‐stage bootloader (U‑Boot, GRUB, PXE, etc.) and exposes it
// to later boot stages.  The grammar is intentionally minimal so
// it can be re‑implemented in other languages for early initrd.
//
// Example cmdline:
//
//   root=/dev/nvme0n1p2 rw console=ttyS0,115200 panic=10
//
// # Public API
// * [`BootArgs`] – read‑only view of parsed arguments
// * [`parse_cmdline`] – convert raw string → [`BootArgs`]
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;

/// Key/value map of boot parameters.
///
/// Values are stored as owned `String`s for simplicity.
#[derive(Debug, Clone, Default)]
pub struct BootArgs {
    map: HashMap<String, String>,
}

impl BootArgs {
    /// Retrieve the value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }

    /// Convenience accessor: return `true` if `key` exists (`key[=value]`).
    pub fn has_flag(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
}

/// Parse a raw bootloader command‑line string.
///
/// Splits on ASCII whitespace, then parses `key=value` pairs.  If a token
/// lacks an `=` sign, it is treated as a valueless flag with value `"1"`.
///
/// # Examples
///
/// ```
/// use cohesix::bootloader::args::{parse_cmdline, BootArgs};
///
/// let args = parse_cmdline("root=/dev/sda1 quiet").unwrap();
/// assert_eq!(args.get("root"), Some("/dev/sda1"));
/// assert!(args.has_flag("quiet"));
/// ```
pub fn parse_cmdline(cmdline: &str) -> Result<BootArgs, &'static str> {
    let mut map = HashMap::new();

    for token in cmdline.split_ascii_whitespace() {
        if token.trim().is_empty() {
            continue;
        }
        let (k, v) = if let Some(eq) = token.find('=') {
            (&token[..eq], &token[eq + 1..])
        } else {
            (token, "1")
        };

        if k.is_empty() {
            return Err("empty key in cmdline");
        }
        map.insert(k.to_owned(), v.to_owned());
    }

    Ok(BootArgs { map })
}


// ───────────────────────────── tests ─────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_values() {
        let args = parse_cmdline("root=/dev/sda rw").unwrap();
        assert_eq!(args.get("root"), Some("/dev/sda"));
        assert_eq!(args.get("rw"), Some("1"));
    }

    #[test]
    fn rejects_empty_key() {
        assert!(parse_cmdline("=novalue").is_err());
    }
}
