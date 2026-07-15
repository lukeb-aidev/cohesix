// Author: Lukas Bower
// Purpose: Validate dynamic Cargo build-script directives before writing them to standard output.
// Copyright 2026 Lukas Bower

/// Validates one complete Cargo build-script directive without its trailing newline.
pub fn validate_cargo_directive(directive: &str) -> Result<(), &'static str> {
    if !directive.starts_with("cargo:") {
        return Err("Cargo build-script directive must start with `cargo:`");
    }
    if directive.chars().any(char::is_control) {
        return Err("Cargo build-script directive contains a control character");
    }
    Ok(())
}

/// Emits one validated Cargo build-script directive or terminates the build script.
pub fn emit_cargo_directive(directive: String) {
    if let Err(error) = validate_cargo_directive(&directive) {
        eprintln!("refusing unsafe Cargo build-script output: {error}");
        std::process::exit(1);
    }
    println!("{directive}");
}

/// Formats untrusted text as one escaped Rust string literal.
pub fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::{rust_string_literal, validate_cargo_directive};

    #[test]
    fn accepts_one_well_formed_directive() {
        assert!(validate_cargo_directive("cargo:rerun-if-changed=/safe/path").is_ok());
    }

    #[test]
    fn rejects_missing_cargo_prefix() {
        assert!(validate_cargo_directive("rerun-if-changed=/safe/path").is_err());
    }

    #[test]
    fn rejects_control_character_injection() {
        for directive in [
            "cargo:rerun-if-changed=/safe/path\ncargo:rustc-cfg=test",
            "cargo:rustc-env=KEY=value\rhidden",
            "cargo:warning=visible\u{000b}hidden",
        ] {
            assert!(validate_cargo_directive(directive).is_err());
        }
    }

    #[test]
    fn rust_string_literal_escapes_code_breakout_and_control_characters() {
        let literal = rust_string_literal("quote\" slash\\ newline\n carriage\r");

        assert_eq!(literal, "\"quote\\\" slash\\\\ newline\\n carriage\\r\"");
        assert!(!literal.chars().any(char::is_control));
    }
}
