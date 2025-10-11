# Coding Guidelines (Rust)
- Rust stable; forbid `unsafe` except small, reviewed modules.
- No TCP or CUDA inside the seL4 VM.
- Traits for providers/transports; avoid global mutable state.
- Deterministic error enums; no panics on user input.
- `cargo fmt` + `clippy -D warnings` before commit.
