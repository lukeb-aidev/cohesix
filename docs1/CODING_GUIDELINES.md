# Coding Guidelines
- Rust stable.
- Forbid `unsafe` except in narrow, reviewed modules.
- Small, testable crates; traits for provider/transport.
- Deterministic error enums; no panics on user input.
- Plain-text logging only; no frameworks in v0.
- `cargo fmt` + `clippy -D warnings` before commit.
