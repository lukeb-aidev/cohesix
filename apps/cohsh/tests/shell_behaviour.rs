// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Cohsh shell UX behaviors and command error handling.
// Author: Lukas Bower

use std::io::Cursor;

use cohesix_ticket::Role;
use cohsh::{NineDoorTransport, Shell};

#[test]
fn queen_commands_require_session() {
    let transport = NineDoorTransport::new(nine_door::NineDoor::new());
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .execute("spawn heartbeat ticks=10")
        .expect_err("spawn should require attachment");
    assert!(err.to_string().contains("attach to a session"));
}

#[test]
fn queen_commands_succeed_when_attached() {
    let mut output = Vec::new();
    {
        let transport = NineDoorTransport::new(nine_door::NineDoor::new());
        let mut shell = Shell::new(transport, &mut output);
        shell.attach(Role::Queen, None).unwrap();
        shell
            .execute("spawn heartbeat ticks=10")
            .expect("spawn should succeed");
        shell
            .execute("bind /log /shadow")
            .expect("bind should succeed");
    }
}

#[test]
fn unknown_commands_are_reported() {
    let transport = NineDoorTransport::new(nine_door::NineDoor::new());
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .execute("unknown-cmd")
        .expect_err("unknown command should fail");
    assert!(err.to_string().contains("unknown command"));
}

#[test]
fn tail_without_session_fails() {
    let transport = NineDoorTransport::new(nine_door::NineDoor::new());
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .execute("tail /log/queen.log")
        .expect_err("tail should require session");
    assert!(err
        .to_string()
        .contains("attach to a session before running tail"));
}

#[test]
fn path_depth_is_capped() {
    let transport = NineDoorTransport::new(nine_door::NineDoor::new());
    let mut shell = Shell::new(transport, Cursor::new(Vec::new()));
    let err = shell
        .execute("ls /a/b/c/d/e/f/g/h/i")
        .expect_err("deep paths should be rejected");
    assert!(err.to_string().contains("maximum depth"));
}

#[test]
fn script_runner_ignores_commented_lines() {
    let script = b"# comment\n\nhelp # inline\n";
    let transport = NineDoorTransport::new(nine_door::NineDoor::new());
    let mut output = Vec::new();
    {
        let mut shell = Shell::new(transport, &mut output);
        shell
            .run_script(Cursor::new(&script[..]))
            .expect("script should succeed");
    }
    let rendered = String::from_utf8(output).expect("utf8 output");
    assert!(rendered.contains("Cohesix command surface:"));
    assert!(!rendered.contains("comment"));
}
