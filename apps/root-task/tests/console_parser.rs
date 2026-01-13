// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate console parser command and length handling.
// Author: Lukas Bower

use root_task::console::{Command, CommandParser, ConsoleError, MAX_LINE_LEN};

#[test]
fn rejects_overlong_lines() {
    let mut parser = CommandParser::new();
    for _ in 0..MAX_LINE_LEN {
        parser.push_byte(b'a').unwrap();
    }
    assert!(matches!(
        parser.push_byte(b'b'),
        Err(ConsoleError::LineTooLong)
    ));
}

#[test]
fn parses_quit_command() {
    let mut parser = CommandParser::new();
    for byte in b"quit" {
        parser.push_byte(*byte).unwrap();
    }
    let command = parser.push_byte(b'\n').unwrap().unwrap();
    assert!(matches!(command, Command::Quit));
}

#[test]
fn invalid_verb_rejected() {
    let mut parser = CommandParser::new();
    for byte in b"unknown" {
        parser.push_byte(*byte).unwrap();
    }
    match parser.push_byte(b'\n') {
        Err(ConsoleError::InvalidVerb) => {}
        other => panic!("unexpected parser result: {other:?}"),
    }
}

#[test]
fn attach_accepts_optional_ticket() {
    let mut parser = CommandParser::new();
    for byte in b"attach queen secret" {
        parser.push_byte(*byte).unwrap();
    }
    match parser.push_byte(b'\n').unwrap().unwrap() {
        Command::Attach { role, ticket } => {
            assert_eq!(role.as_str(), "queen");
            assert_eq!(ticket.unwrap().as_str(), "secret");
        }
        other => panic!("unexpected command {other:?}"),
    }
}

#[test]
fn control_characters_are_ignored() {
    let mut parser = CommandParser::new();
    for byte in [0x01u8, b'h', b'e', 0x7f, b'e', b'l', b'p'] {
        parser.push_byte(byte).unwrap();
    }
    let command = parser.push_byte(b'\n').unwrap().unwrap();
    assert!(matches!(command, Command::Help));
}
