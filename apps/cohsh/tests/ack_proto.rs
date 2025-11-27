// Author: Lukas Bower

use cohsh::proto::{parse_ack, AckStatus};

fn assert_ack(line: &str, expected_status: AckStatus, expected_verb: &str, expected_detail: Option<&str>) {
    let parsed = parse_ack(line).expect("line should parse as ACK");
    assert_eq!(parsed.status, expected_status);
    assert_eq!(parsed.verb, expected_verb);
    assert_eq!(parsed.detail, expected_detail);
}

#[test]
fn parses_golden_ack_lines() {
    assert_ack(
        "OK ATTACH role=queen",
        AckStatus::Ok,
        "ATTACH",
        Some("role=queen"),
    );
    assert_ack(
        "ERR ATTACH reason=unauthenticated",
        AckStatus::Err,
        "ATTACH",
        Some("reason=unauthenticated"),
    );
    assert_ack(
        "ERR AUTH reason=expected-token",
        AckStatus::Err,
        "AUTH",
        Some("reason=expected-token"),
    );
    assert_ack(
        "OK TAIL path=/log/queen.log",
        AckStatus::Ok,
        "TAIL",
        Some("path=/log/queen.log"),
    );
}

#[test]
fn accepts_lines_with_extra_whitespace() {
    assert_ack(
        "  OK ATTACH role=queen   ",
        AckStatus::Ok,
        "ATTACH",
        Some("role=queen"),
    );
    assert_ack(
        "\tERR AUTH reason=expected-token\n",
        AckStatus::Err,
        "AUTH",
        Some("reason=expected-token"),
    );
}

#[test]
fn rejects_unrelated_lines() {
    assert!(parse_ack("TRACE boot complete").is_none());
    assert!(parse_ack("WARN unknown command").is_none());
}
