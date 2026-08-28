// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate TCP CLI script execution with framed console protocol.
// Author: Lukas Bower
#![cfg(feature = "tcp")]

use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::prelude::*;

const TEST_AUTH_TOKEN: &str = "tcp-cli-script-test-token";
const NETSTATS_BODY: [&str; 24] = [
    "netstats: rx_pkts=12 tx_pkts=9 rx_used=4 tx_used=2 polls=37",
    "netstats: generation=7 udp_rx=3 udp_tx=5 tcp_accepts=2 tcp_auth=2 tcp_rx_bytes=384 tcp_recv_ready=8 tcp_recv_budget_hits=1 tcp_tx_bytes=512",
    "netstats: tcp_smoke_out=4 tcp_smoke_out_failures=0",
    "netstats: tcp_post_flush_polls=6 tcp_post_flush_exhaustions=0 cyw43_tail_polls=0 cyw43_tail_hits=0 cyw43_tail_idle=0 cyw43_tail_budget_errors=0",
    "netstats: cyw43_quantum runs=18 turns=22 max_turns=3 max_elapsed_us=71 operator_yields=5 checkpoint_ms=250",
    "netstats: cyw43_quantum_exit idle=8 dispatch=10 turn_cap=0 time_cap=0 physical=4 guard=0",
    "netstats: proof_policy m26d_net_first=no physical_input_yield=enabled",
    "netstats: local_seat_net_mirror=2 local_seat_net_mirror_suppressed=1 hdmi=unavailable",
    "netstats: tx_submit=9 tx_complete=9 tx_free=9 tx_in_flight=0 tx_double_submit=0 tx_zero_len_attempt=0 arp_rx=3 arp_tx=2",
    "netstats: generation=7 mode=dhcp policy=wired active=wired standby=none addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 dhcp=bound",
    "netstats: genet_rx_hw=12 genet_rx_last_len=74 genet_rx_last_ethertype=0x0800",
    "netstats: genet_rxq runtime_cur=0 runtime_hwm=4 runtime_ovf=0 runtime_max_drain=3 runtime_drain_hit=1 runtime_byte_hit=0 root_cur=0 root_hwm=3 root_drops=0 runtime_cmd_drain_seen=1",
    "netstats: genet_direct refresh=fresh snapshot=present phase=pre-idle-service generation=7 sequence=42",
    "netstats: genet_direct_flags flags=0x000001c3 initialized=yes active=yes faulted=no irq_pending=no rx_pending=no tx_pending=no",
    "netstats: genet_direct_before sequence=41 irq_wakes=8 irq_acks=8 raw=0x00000000 mask=0xffffffff active=0x00000000 rdma=5/5 tdma=3/3",
    "netstats: genet_direct_before_ring rx_cursor=12/12 tx_cursor=9/9 rx_packets=12 tx_packets=9 peer_wakes=4 peer_signals=6",
    "netstats: genet_direct_irq badge=0x00000400 wakes=9 acks=9 ack_failures=0 unmask_failures=0",
    "netstats: genet_direct_irq_source raw=0x00000000 mask=0xffffffff active=0x00000000 last=0x00000000",
    "netstats: genet_direct_notification receipts=13 rejected=1 badge_or=0x00000500",
    "netstats: genet_direct_dpc turns=9 budget_hits=0 final_rechecks=9 level_adoptions=0 mcs_quantum_high_us=731 mcs_reasons=0x00000000",
    "netstats: genet_direct_dma rdma_prod=5 rdma_cons=5 tdma_prod=3 tdma_cons=3 rx_packets=12 tx_packets=9",
    "netstats: genet_direct_ring rx_prod=12 rx_cons=12 tx_prod=9 tx_cons=9 rx_valid=yes tx_valid=yes state_changes=0",
    "netstats: genet_direct_peer wakes=4 signals=6 poison_rx=0/0 poison_tx=0/0",
    "netstatus: generation=7 ip=192.168.86.154 gateway=192.168.86.1 src=dhcp-lease dhcp=bound tcp_ready=yes",
];

fn write_frame(stream: &mut std::net::TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
}

fn read_frame(reader: &mut BufReader<std::net::TcpStream>) -> Option<String> {
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let total_len = u32::from_le_bytes(len_buf) as usize;
    let payload_len = total_len.saturating_sub(4);
    let mut payload = vec![0u8; payload_len];
    if reader.read_exact(&mut payload).is_err() {
        return None;
    }
    String::from_utf8(payload).ok()
}

#[test]
fn tcp_script_executes_against_basic_server() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    thread::spawn(move || {
        if let Some(stream) = listener.incoming().next() {
            let mut stream = stream.expect("accept stream");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed.starts_with("TAIL") {
                    write_frame(&mut stream, "OK TAIL path=/log/queen.log");
                    write_frame(&mut stream, "queen boot");
                    write_frame(&mut stream, "heart line");
                    write_frame(&mut stream, "END");
                } else if trimmed.eq_ignore_ascii_case("quit") {
                    write_frame(&mut stream, "OK QUIT");
                    let mut trailing = [0u8; 1];
                    assert_eq!(
                        stream.read(&mut trailing).unwrap(),
                        0,
                        "cohsh must half-close after OK QUIT"
                    );
                    break;
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
        }
    });

    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("cohsh")
        .join("tcp_basic.coh");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .env("COHSH_TCP_PORT", port.to_string())
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .timeout(Duration::from_secs(10))
        .assert();

    let assert = assert
        .success()
        .stdout(predicate::str::contains("Welcome to Cohesix"))
        .stdout(predicate::str::contains("attached session"))
        .stdout(predicate::str::contains("as Queen"))
        .stdout(predicate::str::contains("Cohesix command surface:"))
        .stdout(predicate::str::contains("queen boot"))
        .stdout(predicate::str::contains("heart line"))
        .stdout(predicate::str::contains("[console] OK QUIT"))
        .stdout(predicate::str::contains("closing session"));
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.find("[console] OK QUIT") < stdout.find("closing session"),
        "QUIT acknowledgement must precede local close output: {stdout:?}"
    );
}

#[test]
fn tcp_script_forwards_netstats_and_nettest() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept stream");
        drop(listener);
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut commands = Vec::new();
        while let Some(line) = read_frame(&mut reader) {
            let trimmed = line.trim();
            commands.push(trimmed.to_owned());
            if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                write_frame(&mut stream, "OK AUTH");
            } else if trimmed.starts_with("ATTACH") {
                write_frame(&mut stream, "OK ATTACH role=queen");
            } else if trimmed.eq_ignore_ascii_case("netstats") {
                for line in NETSTATS_BODY {
                    write_frame(&mut stream, line);
                }
                write_frame(&mut stream, "OK NETSTATS");
            } else if trimmed.eq_ignore_ascii_case("nettest") {
                write_frame(
                    &mut stream,
                    "ERR NETTEST reason=policy detail=wifi-host-eapol-pending",
                );
            } else if trimmed.eq_ignore_ascii_case("quit") {
                write_frame(&mut stream, "OK QUIT");
                let mut trailing = [0u8; 1];
                assert_eq!(
                    stream.read(&mut trailing).unwrap(),
                    0,
                    "cohsh must half-close after OK QUIT"
                );
                break;
            } else if trimmed == "PING" {
                write_frame(&mut stream, "PONG");
                write_frame(&mut stream, "OK PING reply=pong");
            }
        }
        commands
    });

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let script_path = std::env::temp_dir().join(format!("cohsh-nettest-{unique}.coh"));
    fs::write(
        &script_path,
        "attach queen\nnetstats\nEXPECT OK\nnettest\nEXPECT ERR\nEXPECT SUBSTR wifi-host-eapol-pending\nquit\n",
    )
    .expect("write script");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .env("COHSH_TCP_PORT", port.to_string())
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .timeout(Duration::from_secs(10))
        .assert();

    let _ = fs::remove_file(&script_path);
    let assert = assert
        .success()
        .stdout(predicate::str::contains(
            "netstatus: generation=7 ip=192.168.86.154",
        ))
        .stdout(predicate::str::contains(
            "[console] ERR NETTEST reason=policy detail=wifi-host-eapol-pending",
        ))
        .stdout(predicate::str::contains("unknown command 'netstats'").not())
        .stdout(predicate::str::contains("unknown command 'nettest'").not());

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let body: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("netstats:") || line.starts_with("netstatus:"))
        .collect();
    assert_eq!(
        body, NETSTATS_BODY,
        "NETSTATS body order changed: {stdout:?}"
    );
    assert_eq!(
        stdout.matches("[console] OK NETSTATS").count(),
        1,
        "expected exactly one NETSTATS acknowledgement: {stdout:?}"
    );
    assert!(
        stdout.contains("closing session"),
        "script did not complete QUIT: {stdout:?}"
    );
    assert!(
        stdout.find("[console] OK QUIT") < stdout.find("closing session"),
        "QUIT acknowledgement must precede local close output: {stdout:?}"
    );

    let commands = server.join().expect("join TCP mock server");
    let netstats = commands
        .iter()
        .position(|line| line.eq_ignore_ascii_case("netstats"))
        .expect("NETSTATS command on accepted connection");
    let nettest = commands
        .iter()
        .position(|line| line.eq_ignore_ascii_case("nettest"))
        .expect("NETTEST command on accepted connection");
    let quit = commands
        .iter()
        .position(|line| line.eq_ignore_ascii_case("quit"))
        .expect("QUIT command on accepted connection");
    assert!(
        netstats < nettest && nettest < quit,
        "commands did not complete in order on one TCP connection: {commands:?}"
    );
}

#[test]
fn tcp_script_fails_when_quit_ack_is_not_followed_by_peer_eof() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept stream");
        drop(listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("server read timeout");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        while let Some(line) = read_frame(&mut reader) {
            let trimmed = line.trim();
            if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                write_frame(&mut stream, "OK AUTH");
            } else if trimmed.starts_with("ATTACH") {
                write_frame(&mut stream, "OK ATTACH role=queen");
            } else if trimmed.eq_ignore_ascii_case("quit") {
                write_frame(&mut stream, "OK QUIT");
                let mut trailing = [0u8; 1];
                assert_eq!(stream.read(&mut trailing).unwrap(), 0);
                release_rx.recv().expect("release held peer");
                break;
            }
        }
    });

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let script_path = std::env::temp_dir().join(format!("cohsh-quit-eof-{unique}.coh"));
    fs::write(&script_path, "attach queen\nquit\n").expect("write script");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .arg("--retry-timeout-ms")
        .arg("50")
        .env("COHSH_TCP_PORT", port.to_string())
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .timeout(Duration::from_secs(5))
        .assert();

    let _ = fs::remove_file(&script_path);
    let assert = assert
        .failure()
        .stderr(predicate::str::contains(
            "timeout waiting for QUIT peer close",
        ))
        .stdout(predicate::str::contains("[console] OK QUIT").not())
        .stdout(predicate::str::contains("closing session").not());
    assert!(
        !assert.get_output().stdout.is_empty(),
        "CLI should retain pre-QUIT script output"
    );

    release_tx.send(()).expect("release server");
    server.join().expect("join TCP mock server");
}

#[test]
fn tcp_script_fails_when_peer_closes_before_quit_ack() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept stream");
        drop(listener);
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        while let Some(line) = read_frame(&mut reader) {
            let trimmed = line.trim();
            if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                write_frame(&mut stream, "OK AUTH");
            } else if trimmed.starts_with("ATTACH") {
                write_frame(&mut stream, "OK ATTACH role=queen");
            } else if trimmed.eq_ignore_ascii_case("quit") {
                break;
            }
        }
    });

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let script_path = std::env::temp_dir().join(format!("cohsh-quit-no-ack-{unique}.coh"));
    fs::write(&script_path, "attach queen\nquit\n").expect("write script");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .arg("--retry-timeout-ms")
        .arg("250")
        .env("COHSH_TCP_PORT", port.to_string())
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .timeout(Duration::from_secs(5))
        .assert();

    let _ = fs::remove_file(&script_path);
    assert
        .failure()
        .stderr(predicate::str::contains("connection closed before OK QUIT"))
        .stdout(predicate::str::contains("[console] OK QUIT").not())
        .stdout(predicate::str::contains("closing session").not());
    server.join().expect("join TCP mock server");
}

#[test]
fn tcp_script_reports_connection_failure() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
    let port = unused_listener.local_addr().expect("listener addr").port();
    drop(unused_listener);

    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("cohsh")
        .join("tcp_basic.coh");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--script")
        .arg(&script_path)
        .env("COHSH_TCP_PORT", port.to_string())
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .timeout(Duration::from_secs(8))
        .assert();

    assert.failure().stderr(predicate::str::contains(
        "failed to connect to Cohesix TCP console",
    ));
}

#[test]
fn tcp_interactive_attach_failure_keeps_prompt() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
    let port = unused_listener.local_addr().expect("listener addr").port();
    drop(unused_listener);

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cohsh"));
    let assert = cmd
        .arg("--transport")
        .arg("tcp")
        .arg("--tcp-port")
        .arg(port.to_string())
        .arg("--role")
        .arg("queen")
        .env("COHSH_AUTH_TOKEN", TEST_AUTH_TOKEN)
        .write_stdin("quit\n")
        .timeout(Duration::from_secs(10))
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("Welcome to Cohesix"))
        .stdout(predicate::str::contains(
            "detached shell: run 'attach <role>' to connect",
        ))
        .stdout(predicate::str::contains("coh> "))
        .stderr(predicate::str::contains("TCP attach failed"));
}
