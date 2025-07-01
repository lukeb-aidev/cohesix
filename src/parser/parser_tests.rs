// CLASSIFICATION: COMMUNITY
// Filename: parser_tests.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31
//! Parser test scaffolding for Coh_CC

use crate::prelude::*;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_parser_test() {
        let input = "fn main() {}";
        let parsed = parse_function(input);
        assert!(parsed.is_ok());
    }
}

fn parse_function(source: &str) -> Result<(), String> {
    // Very small parser for unit tests: only understands `fn name() {}`
    let source = source.trim();
    if let Some(stripped) = source.strip_prefix("fn ") {
        if let Some((name, rest)) = stripped.split_once('(') {
            let rest = rest.trim();
            if rest.starts_with(")") && rest.ends_with("{}") {
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Ok(());
                }
            }
        }
    }
    Err("parse error".into())
}
