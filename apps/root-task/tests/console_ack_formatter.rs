// Author: Lukas Bower

use heapless::String;
use root_task::console::proto::{render_ack, AckLine, AckStatus, LineFormatError};
use root_task::serial::DEFAULT_LINE_CAPACITY;

fn render_line(ack: AckLine<'_>) -> Result<String<{ DEFAULT_LINE_CAPACITY }>, LineFormatError> {
    let mut buf: String<{ DEFAULT_LINE_CAPACITY }> = String::new();
    render_ack(&mut buf, &ack)?;
    Ok(buf)
}

#[test]
fn renders_expected_ack_lines() {
    let cases = [
        (
            "OK ATTACH role=queen",
            AckLine {
                status: AckStatus::Ok,
                verb: "ATTACH",
                detail: Some("role=queen"),
            },
        ),
        (
            "ERR ATTACH reason=unauthenticated",
            AckLine {
                status: AckStatus::Err,
                verb: "ATTACH",
                detail: Some("reason=unauthenticated"),
            },
        ),
        (
            "ERR AUTH reason=expected-token",
            AckLine {
                status: AckStatus::Err,
                verb: "AUTH",
                detail: Some("reason=expected-token"),
            },
        ),
        (
            "OK TAIL path=/log/queen.log",
            AckLine {
                status: AckStatus::Ok,
                verb: "TAIL",
                detail: Some("path=/log/queen.log"),
            },
        ),
    ];

    for (expected, ack) in cases {
        let rendered = render_line(ack).expect("formatter should succeed");
        assert_eq!(rendered, expected);
    }
}

#[test]
fn rejects_lines_that_exceed_capacity() {
    let detail = "x".repeat(DEFAULT_LINE_CAPACITY + 32);
    let ack = AckLine {
        status: AckStatus::Ok,
        verb: "TAIL",
        detail: Some(detail.as_str()),
    };

    let mut buf: String<{ DEFAULT_LINE_CAPACITY }> = String::new();
    assert_eq!(render_ack(&mut buf, &ack), Err(LineFormatError::Truncated));
}
