// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate canonical live capture, exact acknowledgement replay and passive bounds.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::Result;
use cohsh::trace_capture::{self, Capture, CaptureWriter};
use cohsh_core::trace::{CaptureMetadata, TraceLog, TracePolicy};
use std::io::{Read, Write};
use std::time::Duration;
use tempfile::TempDir;

fn metadata(policy: TracePolicy) -> CaptureMetadata {
    CaptureMetadata {
        backend: 1,
        completion: 0,
        target_sha256: [1; 32],
        session_sha256: [2; 32],
        manifest_sha256: [3; 32],
        image_sha256: [4; 32],
        policy_sha256: trace_capture::policy_digest(policy, 60_000),
        captured_unix_ms: 1_000,
        max_duration_ms: 60_000,
    }
}

#[test]
fn redaction_precedes_persistence_and_writer_bytes_are_unchanged() -> Result<()> {
    let policy = TracePolicy::new(4096, 512, 256);
    let capture = Capture::new(policy, metadata(policy), vec!["KNOWN_CANARY".to_owned()])?;
    let mut writer = CaptureWriter::new(Vec::new(), Some(capture.clone()));
    for piece in [
        b"[console] OK AUTH\n".as_slice(),
        b"[console] OK CAT path=/proc/boot\n",
        b"{\"payload\":\"{\\\"secret\\\":\\\"HIDDEN_CANARY\\\"}\"}\n",
        b"token=KNOWN_CANARY\n",
        b"END\n",
    ] {
        writer.write_all(piece)?;
    }
    let temp = TempDir::new()?;
    let path = temp.path().join("capture.trace");
    assert_eq!(capture.lock().unwrap().finish(&path, true)?, 0);
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("CANARY"));
    assert!(!text.contains("OK AUTH"));
    let trace = TraceLog::decode(&bytes, policy)?;
    assert_eq!(trace.ack_lines, ["[console] OK CAT path=/proc/boot", "END"]);
    assert_eq!(trace.capture, Some(metadata(policy)));
    let lines = trace_capture::replay_lines(&trace, &metadata(policy).policy_sha256)?;
    assert_eq!(lines.last().map(String::as_str), Some("END"));
    assert!(trace_capture::replay_lines(&trace, &[0; 32]).is_err());
    Ok(())
}

#[test]
fn byte_limit_and_partial_line_preserve_valid_prefix_without_stopping_output() -> Result<()> {
    for (bytes, expected) in [(vec![b'x'; 1024], 2), (b"OK PING\npartial".to_vec(), 1)] {
        let policy = TracePolicy::new(512, 128, 64);
        let capture = Capture::new(policy, metadata(policy), vec![])?;
        let mut output = Vec::new();
        let mut writer = CaptureWriter::new(&mut output, Some(capture.clone()));
        writer.write_all(&bytes)?;
        writer.flush()?;
        assert_eq!(output, bytes);
        let temp = TempDir::new()?;
        let path = temp.path().join("partial.trace");
        assert_eq!(capture.lock().unwrap().finish(&path, true)?, expected);
        let trace = trace_capture::read_trace(&path, policy)?;
        assert_eq!(trace.capture.as_ref().unwrap().completion, expected);
        trace_capture::replay_lines(&trace, &metadata(policy).policy_sha256)?;
        assert!(
            trace_capture::CapturedNamespace::new(&trace, &metadata(policy).policy_sha256).is_err()
        );
    }
    Ok(())
}

#[test]
fn hostile_counts_and_metadata_tamper_are_rejected() -> Result<()> {
    use sha2::{Digest, Sha256};
    let policy = TracePolicy::new(1024, 256, 128);
    let trace = TraceLog {
        capture: Some(metadata(policy)),
        frames: vec![],
        ack_lines: vec![],
    };
    let mut bytes = trace.encode(policy)?;
    bytes[20] ^= 1;
    assert!(TraceLog::decode(&bytes, policy).is_err());
    for name in ["password=LIST_CANARY", "auth_ref:LIST_CANARY"] {
        let hostile = TraceLog {
            capture: Some(metadata(policy)),
            frames: vec![cohsh_core::trace::TraceFrame {
                request: b"LS /host".to_vec(),
                response: serde_json::to_vec(&vec![name])?,
            }],
            ack_lines: vec![],
        };
        assert!(
            trace_capture::CapturedNamespace::new(&hostile, &metadata(policy).policy_sha256)
                .is_err()
        );
    }
    bytes[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
    let length = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..length]);
    bytes[length..].copy_from_slice(&digest);
    assert!(TraceLog::decode(&bytes, policy).is_err());
    Ok(())
}

#[test]
#[cfg(feature = "rest")]
fn actual_rest_read_is_captured_and_replay_has_no_writer() -> Result<()> {
    use cohsh::Transport;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> Result<Vec<String>> {
        let mut requests = Vec::new();
        // The transport consults optional bounds before CAT and its ACK preview.
        // This fixture exercises the documented absent-metadata path explicitly.
        for response in [
            None,
            Some(
                r#"{"status":"OK","verb":"CAT","path":"/proc/root/reachable","lines":["reachable=yes"],"end":true}"#,
            ),
            None,
        ] {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut request = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                stream.read_exact(&mut byte)?;
                request.extend_from_slice(&byte);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
                anyhow::ensure!(request.len() <= 8192, "HTTP request bound");
            }
            let (status, body) = response
                .map(|body| ("200 OK", body))
                .unwrap_or(("404 Not Found", "{}"));
            stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())?;
            requests.push(String::from_utf8(request)?);
        }
        Ok(requests)
    });
    let policy = TracePolicy::new(4096, 512, 256);
    let mut header = metadata(policy);
    header.backend = 2;
    let capture = Capture::new(policy, header.clone(), vec!["REST_CANARY".to_owned()])?;
    let rest =
        cohsh::RestTransport::new(format!("http://{address}"), Some("REST_CANARY".to_owned()));
    let mut transport = trace_capture::CaptureTransport::new(rest, capture.clone());
    let session = transport.attach(cohesix_ticket::Role::Queen, None)?;
    assert_eq!(
        transport.read(&session, "/proc/root/reachable")?,
        ["reachable=yes"]
    );
    let requests = server.join().unwrap()?;
    assert!(requests[0].starts_with("GET /v1/meta/bounds "));
    assert!(requests[1].starts_with("GET /v1/fs/cat?"));
    assert!(requests[2].starts_with("GET /v1/meta/bounds "));
    let temp = TempDir::new()?;
    let path = temp.path().join("rest.trace");
    capture.lock().unwrap().finish(&path, true)?;
    let trace = trace_capture::read_trace(&path, policy)?;
    let mut replay = trace_capture::CapturedNamespace::new(&trace, &header.policy_sha256)?;
    let session = replay.attach(cohesix_ticket::Role::Queen, None)?;
    assert_eq!(
        replay.read(&session, "/proc/root/reachable")?,
        ["reachable=yes"]
    );
    assert!(replay.read(&session, "/proc/not-captured").is_err());
    assert!(replay.write(&session, "/queen/ctl", b"anything").is_err());
    assert!(!String::from_utf8_lossy(&std::fs::read(path)?).contains("REST_CANARY"));
    Ok(())
}

#[test]
#[cfg(all(feature = "tcp", feature = "in-process"))]
fn actual_tcp_cli_capture_replays_without_connecting() -> Result<()> {
    use std::net::TcpListener;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let server = std::thread::spawn(move || -> Result<Vec<String>> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut requests = Vec::new();
        loop {
            let mut length = [0; 4];
            if stream.read_exact(&mut length).is_err() {
                break;
            }
            let length = u32::from_le_bytes(length) as usize;
            anyhow::ensure!((4..=8192).contains(&length), "frame bound");
            let mut bytes = vec![0; length - 4];
            stream.read_exact(&mut bytes)?;
            let request = String::from_utf8(bytes)?;
            requests.push(request.clone());
            let lines: &[&str] = if request.starts_with("AUTH ") {
                &["OK AUTH"]
            } else if request.starts_with("ATTACH ") {
                &["OK ATTACH role=queen"]
            } else if request.starts_with("CAT ") {
                &["OK CAT path=/proc/root/reachable", "reachable=yes", "END"]
            } else if request.eq_ignore_ascii_case("QUIT") {
                &["OK QUIT"]
            } else {
                anyhow::bail!("unexpected request {request}");
            };
            for line in lines {
                stream.write_all(&((line.len() + 4) as u32).to_le_bytes())?;
                stream.write_all(line.as_bytes())?;
            }
            if request.eq_ignore_ascii_case("QUIT") {
                break;
            }
        }
        Ok(requests)
    });
    let temp = TempDir::new()?;
    let script = temp.path().join("capture.coh");
    std::fs::write(&script, "attach queen\ncat /proc/root/reachable\nquit\n")?;
    let trace = temp.path().join("capture.trace");
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    command
        .args([
            "--transport",
            "tcp",
            "--tcp-port",
            &port.to_string(),
            "--auth-token",
            "NETWORK_CANARY",
            "--script",
        ])
        .arg(&script)
        .arg("--record-trace")
        .arg(&trace)
        .timeout(Duration::from_secs(15))
        .assert()
        .success();
    let requests = server.join().unwrap()?;
    assert_eq!(
        requests,
        [
            "AUTH NETWORK_CANARY",
            "ATTACH queen ",
            "CAT /proc/root/reachable",
            "quit"
        ]
    );
    let bytes = std::fs::read(&trace)?;
    assert!(!String::from_utf8_lossy(&bytes).contains("NETWORK_CANARY"));
    let policy = TracePolicy::new(
        cohsh::TRACE_MAX_BYTES,
        cohsh::SECURE9P_MSIZE,
        cohsh_core::MAX_LINE_LEN as u32,
    );
    let decoded = TraceLog::decode(&bytes, policy)?;
    assert!(decoded
        .frames
        .iter()
        .any(|frame| frame.request == b"CAT /proc/root/reachable"));
    let expected = trace_capture::replay_lines(
        &decoded,
        &trace_capture::policy_digest(
            policy,
            cohsh::CohshPolicy::from_generated().trace.max_duration_ms,
        ),
    )?
    .join("\n")
        + "\n";
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
        .args(["--transport", "tcp", "--tcp-port", "1", "--replay-trace"])
        .arg(&trace)
        .timeout(Duration::from_secs(5))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(output, expected.as_bytes());
    Ok(())
}

#[test]
fn failed_read_invalidates_prior_observation_and_reconnect_can_replace_it() -> Result<()> {
    use cohsh::Transport;
    use cohsh_core::trace::TraceFrame;
    let policy = TracePolicy::new(4096, 512, 256);
    let mut trace = TraceLog {
        capture: Some(metadata(policy)),
        frames: vec![TraceFrame {
            request: b"CAT /proc/root/reachable".to_vec(),
            response: br#"["reachable=yes"]"#.to_vec(),
        }],
        ack_lines: vec![],
    };
    trace.frames.push(TraceFrame {
        request: b"CAT /proc/root/reachable".to_vec(),
        response: br#"{"error":"read-unavailable"}"#.to_vec(),
    });
    let mut replay =
        trace_capture::CapturedNamespace::new(&trace, &metadata(policy).policy_sha256)?;
    let session = replay.attach(cohesix_ticket::Role::Queen, None)?;
    assert!(replay.read(&session, "/proc/root/reachable").is_err());
    trace.frames.push(TraceFrame {
        request: b"CAT /proc/root/reachable".to_vec(),
        response: br#"["reachable=no"]"#.to_vec(),
    });
    let mut replay =
        trace_capture::CapturedNamespace::new(&trace, &metadata(policy).policy_sha256)?;
    assert_eq!(
        replay.read(&session, "/proc/root/reachable")?,
        ["reachable=no"]
    );
    Ok(())
}
