// Author: Lukas Bower

use std::io::Cursor;

use cohsh::{NineDoorTransport, Shell};

#[test]
fn planned_commands_return_stub_message() {
    let mut output = Vec::new();
    {
        let transport = NineDoorTransport::new(nine_door::NineDoor::new());
        let mut shell = Shell::new(transport, &mut output);
        shell
            .execute("mount service /mnt")
            .expect("planned command should not error");
        shell
            .execute("spawn heartbeat")
            .expect("planned command should not error");
    }
    let rendered = String::from_utf8(output).expect("utf8 output");
    assert!(rendered.contains("'mount' is planned but not implemented"));
    assert!(rendered.contains("'spawn' is planned but not implemented"));
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
