// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared help text for Cohesix console and cohsh CLI.
// Author: Lukas Bower

//! Shared help text for Cohesix console and cohsh CLI.

/// Root console help lines emitted by the serial console.
pub const ROOT_CONSOLE_HELP_LINES: &[&str] = &[
    "  help                    - Show commands available on this console",
    "  ping                    - Check console liveness",
    "  bi                      - Show BootInfo and profile identity",
    "  caps                    - Show capability slots",
    "  caps mcs                - Show bounded live MCS authority/object counts",
    "  mem                     - Show untyped memory summary",
    "  smp [activity|dump]      - Show activity or debug-only scheduler state",
    "  smp mcs                 - Show generated/live MCS admission",
    "  smp poll-time           - Show Pi root elapsed-time observations",
    "  cachelog [n]            - Show bounded recent cache operations",
];

/// Session operations available after the event pump starts, not at early bootstrap.
pub const ROOT_SESSION_HELP_LINES: &[&str] = &[
    "Session and namespace (role/profile restrictions apply):",
    "  attach <role> [ticket]   - Select an authorized session",
    "  ls <path>               - List a namespace directory",
    "  cat <path>              - Read a bounded namespace file",
    "  tail <path> [lines]      - Read a finite tail (1..256 lines)",
    "  log                     - Tail /log/queen.log",
    "  echo <path> <payload>   - Append one line (root uses path-first syntax)",
    "  spawn <JSON>            - Compatibility Worker request; not production",
    "  kill <worker_id>        - Compatibility termination; not production",
    "  nettest                 - Admit network test; inspect netstats for result",
    "  netstats                - Show network counters and test state",
    "  reboot                  - Request Queen-authorized platform restart",
    "  quit                    - Close console session",
    "Use host cohsh for test and man <command>; ACK means admission, not completion.",
];

/// cohsh CLI help lines for verbs backed by the console grammar.
pub const COHSH_CONSOLE_HELP_LINES: &[&str] = &[
    "  help                         - Show this command index",
    "  man [command]                - Read complete local usage and examples",
    "Session:",
    "  attach <role> [ticket]        - Attach (login is an alias)",
    "  detach                       - Close attachment; keep shell open",
    "  quit                         - Close session and exit",
    "  ping                         - Check attached session liveness",
    "Namespace and logs:",
    "  ls <path>                    - List a directory; man ls includes the tree",
    "  cat <path>                   - Read a bounded file",
    "  tail <path> [lines]           - Read a finite tail (1..256 lines)",
    "  log                          - Tail /log/queen.log",
    "  log dump <file.txt> [--force] - Save retained log to a host file",
    "  echo <text> > <path>          - Append one line; never truncate",
    "Control (requires write authority; man authority explains production):",
    "  spawn <heartbeat|gpu|lora> [opts] - Compatibility request; ACK is admission only",
    "  kill <worker_id>             - Request compatibility Worker termination",
    "  bind <src> <dst>             - Request compatibility namespace binding",
    "  mount <service> <path>       - Request compatibility service mount",
    "  lifecycle <command>          - cordon, drain, resume, quiesce, reset",
    "  telemetry push <src> --device <id> - Upload bounded host telemetry",
    "Diagnostics (target/transport support varies):",
    "  bi                           - Inspect target BootInfo",
    "  caps                         - Inspect target capability slots",
    "  caps mcs                     - Inspect bounded MCS authority",
    "  smp [activity|dump]           - Inspect activity or debug scheduler state",
    "  smp mcs                      - Inspect generated/live MCS admission",
    "  smp poll-time                - Inspect Pi root elapsed-time observations",
    "  nettest                      - Admit network self-test; inspect netstats",
    "  netstats                     - Read counters and network-test result",
    "  reboot                       - Request Queen-authorized platform restart",
    "Host tools:",
    "  test [options]               - Self-tests; man test lists modes and flags",
    "  pool bench <options>         - Mutating pool benchmark; man pool",
    "  tcp-diag [port]              - Connection diagnostics (TCP-enabled builds)",
];

#[cfg(test)]
mod tests {
    use super::{ROOT_CONSOLE_HELP_LINES, ROOT_SESSION_HELP_LINES};

    #[test]
    fn event_help_covers_the_console_grammar_without_duplicate_entries() {
        let lines: alloc::vec::Vec<_> = ROOT_CONSOLE_HELP_LINES
            .iter()
            .chain(ROOT_SESSION_HELP_LINES.iter())
            .collect();
        for (index, line) in lines.iter().enumerate() {
            assert!(
                !lines[..index].contains(line),
                "duplicate help line: {line}"
            );
        }
        for verb in [
            "help", "bi", "caps", "smp", "mem", "ping", "attach", "tail", "cat", "ls", "echo",
            "log", "quit", "nettest", "netstats", "reboot", "spawn", "kill", "cachelog",
        ] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.split_whitespace().next() == Some(verb)),
                "missing root help for {verb}"
            );
        }
        assert!(lines
            .iter()
            .any(|line| line.contains("Use host cohsh for test")));
    }

    #[test]
    fn root_help_lists_mcs_diagnostics_as_explicit_commands() {
        assert!(ROOT_CONSOLE_HELP_LINES
            .iter()
            .any(|line| line.trim_start().starts_with("caps mcs ")));
        assert!(ROOT_CONSOLE_HELP_LINES
            .iter()
            .any(|line| line.trim_start().starts_with("smp mcs ")));
    }
}
