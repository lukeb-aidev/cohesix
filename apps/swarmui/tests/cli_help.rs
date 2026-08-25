// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify SwarmUI help exits before desktop initialization.
// Author: Lukas Bower

use std::process::Command;

const EXPECTED_HELP: &str = "\
SwarmUI desktop client for Cohesix

Usage: swarmui [OPTIONS]

Options:
      --replay <FILE>             Load a Hive CBOR snapshot for offline replay
      --replay-trace <FILE>       Load a Secure9P trace for offline replay
      --mint-ticket               Mint a capability ticket and exit
      --role <ROLE>               Role for --mint-ticket
      --ticket-subject <SUBJECT>  Subject identity for --mint-ticket
      --ticket-config <FILE>      Ticket configuration for --mint-ticket
      --ticket-secret <SECRET>    Ticket signing secret for --mint-ticket
  -h, --help                      Print help
";

#[test]
fn help_flags_exit_before_desktop_initialization() {
    for flag in ["-h", "--help"] {
        let binary = std::env::var_os("CARGO_BIN_EXE_swarmui")
            .expect("Cargo must provide the SwarmUI integration-test binary");
        let output = Command::new(binary)
            .arg(flag)
            .env("SWARMUI_TRANSPORT", "invalid-test-transport")
            .output()
            .expect("run packaged SwarmUI test binary");

        assert!(
            output.status.success(),
            "{flag} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), EXPECTED_HELP);
        assert!(output.stderr.is_empty(), "{flag} wrote to stderr");
    }
}
