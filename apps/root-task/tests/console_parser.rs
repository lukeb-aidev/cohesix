// Author: Lukas Bower

use root_task::console::{Command, CommandParser, ConsoleError, MAX_LINE_LEN};

#[test]
fn rejects_overlong_lines() {
    let mut parser = CommandParser::new();
    for _ in 0..(MAX_LINE_LEN - 1) {
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
