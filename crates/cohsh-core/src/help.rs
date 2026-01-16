// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared help text for Cohesix console and cohsh CLI.
// Author: Lukas Bower

//! Shared help text for Cohesix console and cohsh CLI.

/// Root console help lines emitted by the serial console.
pub const ROOT_CONSOLE_HELP_LINES: &[&str] = &[
    "  help  - Show this help",
    "  bi    - Show bootinfo summary",
    "  caps  - Show capability slots",
    "  mem   - Show untyped summary",
    "  ping  - Respond with pong",
    "  test  - Self-test (host-only; use cohsh)",
    "  cachelog [n] - Dump recent cache operations",
    "  quit  - Exit the console session",
];

/// cohsh CLI help lines for verbs backed by the console grammar.
pub const COHSH_CONSOLE_HELP_LINES: &[&str] = &[
    "  help                         - Show this help message",
    "  attach <role> [ticket]       - Attach to a NineDoor session",
    "  tail <path>                  - Stream a file via NineDoor",
    "  log                          - Tail /log/queen.log",
    "  ping                         - Report attachment status for health checks",
    "  test [--mode <quick|full>] [--json] [--timeout <s>] [--no-mutate] - Run self-tests",
    "  ls <path>                    - Enumerate directory entries",
    "  cat <path>                   - Read file contents",
    "  echo <text> > <path>         - Append to a file (adds newline)",
    "  spawn <role> [opts]          - Queue worker spawn command",
    "  kill <worker_id>             - Queue worker termination",
    "  quit                         - Close the session and exit",
];
