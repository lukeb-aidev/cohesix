<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents Cohesix coding guidelines for Rust contributors. -->
<!-- Author: Lukas Bower -->
# Cohesix Coding Guidelines (Rust)

1. **Toolchain**: Rust stable with `rustfmt` and `clippy -D warnings` enforced in CI.
2. **Safety**: `#![forbid(unsafe_code)]` in all crates except tightly scoped, reviewed modules that interact with seL4 syscalls, MMIO/device drivers, allocators, or IPC/bootstrapping. Unsafe blocks must be isolated and justified.
3. **Crate Structure**: Prefer small, role-oriented crates (e.g., `secure9p-codec`, `secure9p-core`, `nine-door`, `worker-heart`). Public APIs must have doc comments and integration tests when behaviour spans crates.
4. **Error Handling**: VM-facing crates must use deterministic, exhaustively documented enums (`Error::Permission`, `Error::NotFound`, etc.). Host tools may use `anyhow` for ergonomics but must preserve deterministic ACK/ERR semantics. No panics on user-controlled input or protocol frames.
5. **Logging**: Plain-text logging through append-only files; avoid heavyweight logging frameworks. Include role and ticket identifiers in log lines for traceability.
6. **Configuration**: No environment-variable magic inside the VM. Configuration flows through 9P control files or compile-time constants.
7. **Testing**: Every new module must provide unit tests. Cross-component behaviour should be validated with integration tests using in-memory transports.
8. **Concurrency**: Use seL4 notification objects and message queues; avoid OS threads inside the VM. Host-side tools may use async runtimes but must gate them behind features.
9. **Dependencies**: Keep the dependency tree minimal. Audit third-party crates; prefer `no_std`-friendly libraries. Never vendor seL4 code.
10. **Style**: Follow idiomatic Rust naming (snake_case, CamelCase types). Provide type aliases for capability IDs and tickets to avoid misuse.
11. **Documentation**: Update the relevant document in `/docs` when adding APIs, changing workflows, or introducing new roles.
12. **Security**: Validate all 9P payload lengths and UTF-8 correctness. Deny default; explicitly grant permissions per role and path.
