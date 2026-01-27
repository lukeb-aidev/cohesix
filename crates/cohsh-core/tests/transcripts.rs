// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare console transcripts across serial/TCP/core transports.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketKey, TicketToken,
};
use cohsh::queen;
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::{
    parse_role, role_label, Command as ConsoleCommand, CommandParser, ConsoleError, RoleParseMode,
};

const BOOT_SCRIPT: &str = "scripts/cohsh/boot_v0.coh";
const AUTH_TOKEN: &str = "changeme";
const CONVERGE_SCENARIO: &str = "converge_v0";
const CONVERGE_TAIL_PATH: &str = "/worker/worker-1/telemetry";
const QUEEN_LOG_PATH: &str = "/log/queen.log";
const QUEEN_SECRET: &str = "queen-secret";
const WORKER_SECRET: &str = "worker-secret";
const INVALID_SECRET: &str = "invalid-secret";
const CONSOLE_ACK_FANOUT: usize = 1;

enum ScriptOp {
    Command(String),
    Wait(u64),
}

struct Scenario {
    name: &'static str,
    ops: Vec<ScriptOp>,
    script_path: PathBuf,
    ack_fanout: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct AuthThrottle {
    failures: u32,
    blocked_until_ms: u64,
}

impl AuthThrottle {
    const BASE_BACKOFF_MS: u64 = 250;
    const MAX_SHIFT: u32 = 8;

    fn register_failure(&mut self, now_ms: u64) {
        let shift = self.failures.min(Self::MAX_SHIFT);
        let delay = Self::BASE_BACKOFF_MS.saturating_mul(1u64 << shift);
        self.failures = self.failures.saturating_add(1);
        self.blocked_until_ms = now_ms.saturating_add(delay);
    }

    fn register_success(&mut self) {
        self.failures = 0;
        self.blocked_until_ms = 0;
    }

    fn check(&self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.blocked_until_ms {
            Err(self.blocked_until_ms.saturating_sub(now_ms))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct TicketValidator {
    records: Vec<(Role, TicketKey)>,
}

impl TicketValidator {
    fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    fn register(&mut self, role: Role, secret: &str) {
        self.records.push((role, TicketKey::from_secret(secret)));
    }

    fn validate(&self, role: Role, ticket: Option<&str>) -> bool {
        let ticket = ticket.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        if role == Role::Queen && ticket.is_none() {
            return true;
        }
        let Some(ticket) = ticket else {
            return false;
        };
        let key = self
            .records
            .iter()
            .find_map(|(entry_role, key)| (*entry_role == role).then_some(key));
        let Some(key) = key else {
            return false;
        };
        let Ok(decoded) = TicketToken::decode(ticket, key) else {
            return false;
        };
        decoded.claims().role == role
    }
}

struct ConsoleHarness {
    parser: CommandParser,
    throttle: AuthThrottle,
    validator: TicketValidator,
    now_ms: u64,
    session: Option<Role>,
    next_session_id: u64,
    ack_fanout: usize,
}

impl ConsoleHarness {
    fn new(validator: TicketValidator, ack_fanout: usize) -> Self {
        Self {
            parser: CommandParser::new(),
            throttle: AuthThrottle::default(),
            validator,
            now_ms: 0,
            session: None,
            next_session_id: 1,
            ack_fanout: ack_fanout.max(1),
        }
    }

    fn advance_time(&mut self, delta_ms: u64) {
        self.now_ms = self.now_ms.saturating_add(delta_ms);
    }

    fn handle_command(&mut self, command: ConsoleCommand) -> Vec<String> {
        let verb_label = command.verb().ack_label();
        match command {
            ConsoleCommand::Attach { role, ticket } => {
                self.handle_attach(role.as_str(), ticket.as_deref())
            }
            ConsoleCommand::Tail { path } => {
                if self.session.is_none() {
                    return vec![render_ack_line(
                        AckStatus::Err,
                        verb_label,
                        Some("reason=unauthenticated"),
                    )];
                }
                let mut lines = Vec::new();
                let detail = format!("path={}", path.as_str());
                lines.push(render_ack_line(
                    AckStatus::Ok,
                    verb_label,
                    Some(detail.as_str()),
                ));
                for line in tail_lines(path.as_str()) {
                    lines.push((*line).to_owned());
                }
                lines.push("END".to_owned());
                lines
            }
            ConsoleCommand::Log => {
                if self.session.is_none() {
                    return vec![render_ack_line(
                        AckStatus::Err,
                        verb_label,
                        Some("reason=unauthenticated"),
                    )];
                }
                let mut lines = Vec::new();
                lines.push(render_ack_line(AckStatus::Ok, verb_label, None));
                for line in tail_lines(QUEEN_LOG_PATH) {
                    lines.push((*line).to_owned());
                }
                lines.push("END".to_owned());
                lines
            }
            ConsoleCommand::Echo { path, payload } => {
                if self.session.is_none() {
                    return vec![render_ack_line(
                        AckStatus::Err,
                        verb_label,
                        Some("reason=unauthenticated"),
                    )];
                }
                let detail = format!("path={} bytes={}", path.as_str(), payload.as_bytes().len());
                vec![render_ack_line(
                    AckStatus::Ok,
                    verb_label,
                    Some(detail.as_str()),
                )]
            }
            ConsoleCommand::Spawn(payload) => {
                if self.session.is_none() {
                    return vec![render_ack_line(
                        AckStatus::Err,
                        verb_label,
                        Some("reason=unauthenticated"),
                    )];
                }
                let detail = format!("payload={}", payload.as_str());
                vec![render_ack_line(
                    AckStatus::Ok,
                    verb_label,
                    Some(detail.as_str()),
                )]
            }
            ConsoleCommand::Quit => {
                let mut lines = Vec::new();
                let count = if self.session.is_some() {
                    self.ack_fanout
                } else {
                    1
                };
                self.session = None;
                for _ in 0..count {
                    lines.push(render_ack_line(AckStatus::Ok, verb_label, None));
                }
                lines
            }
            ConsoleCommand::Ping => vec![
                "PONG".to_owned(),
                render_ack_line(AckStatus::Ok, verb_label, Some("reply=pong")),
            ],
            ConsoleCommand::Help => {
                let mut lines = Vec::new();
                lines.push("Commands:".to_owned());
                for entry in cohsh_core::help::ROOT_CONSOLE_HELP_LINES {
                    lines.push((*entry).to_owned());
                }
                lines.push(render_ack_line(AckStatus::Ok, verb_label, None));
                lines
            }
            ConsoleCommand::BootInfo
            | ConsoleCommand::Caps
            | ConsoleCommand::Mem
            | ConsoleCommand::Test
            | ConsoleCommand::NetTest
            | ConsoleCommand::NetStats
            | ConsoleCommand::Cat { .. }
            | ConsoleCommand::Ls { .. }
            | ConsoleCommand::Kill(_)
            | ConsoleCommand::CacheLog { .. } => vec![render_ack_line(
                AckStatus::Err,
                verb_label,
                Some("reason=unsupported"),
            )],
        }
    }

    fn handle_attach(&mut self, role: &str, ticket: Option<&str>) -> Vec<String> {
        if let Err(delay) = self.throttle.check(self.now_ms) {
            let detail = format!("reason=throttled delay_ms={delay}");
            return vec![render_ack_line(
                AckStatus::Err,
                "ATTACH",
                Some(detail.as_str()),
            )];
        }

        let Some(requested_role) = parse_role(role, RoleParseMode::AllowWorkerAlias) else {
            return vec![render_ack_line(
                AckStatus::Err,
                "ATTACH",
                Some("reason=invalid-role"),
            )];
        };

        let validated = self.validator.validate(requested_role, ticket);
        if let Err(err) = self.parser.record_login_attempt(validated, self.now_ms) {
            let detail = match err {
                ConsoleError::RateLimited(delay) => {
                    format!("reason=rate-limited delay_ms={delay}")
                }
                other => format!("reason={other}"),
            };
            return vec![render_ack_line(
                AckStatus::Err,
                "ATTACH",
                Some(detail.as_str()),
            )];
        }

        if validated {
            self.session = Some(requested_role);
            self.next_session_id = self.next_session_id.wrapping_add(1);
            self.throttle.register_success();
            let detail = format!("role={}", role_label(requested_role));
            let mut lines = Vec::new();
            for _ in 0..self.ack_fanout {
                lines.push(render_ack_line(
                    AckStatus::Ok,
                    "ATTACH",
                    Some(detail.as_str()),
                ));
            }
            return lines;
        }

        self.throttle.register_failure(self.now_ms);
        vec![render_ack_line(
            AckStatus::Err,
            "ATTACH",
            Some("reason=denied"),
        )]
    }
}

fn tail_lines(path: &str) -> &'static [&'static str] {
    if path == QUEEN_LOG_PATH {
        &["line one", "line two"]
    } else if path == CONVERGE_TAIL_PATH {
        &["tick 1", "tick 2"]
    } else {
        &[]
    }
}

fn render_ack_line(status: AckStatus, verb: &str, detail: Option<&str>) -> String {
    let mut line = String::new();
    let ack = AckLine {
        status,
        verb,
        detail,
    };
    render_ack(&mut line, &ack).expect("render ack");
    line
}

fn render_parse_error(err: ConsoleError) -> String {
    let detail = match err {
        ConsoleError::RateLimited(delay) => format!("reason=rate-limited delay_ms={delay}"),
        other => format!("reason={other}"),
    };
    render_ack_line(AckStatus::Err, "PARSE", Some(detail.as_str()))
}

fn cohsh_core_output_root() -> PathBuf {
    transcript_support::output_root().join("cohsh-core")
}

fn parse_script(contents: &str) -> Vec<ScriptOp> {
    let mut ops = Vec::new();
    for raw_line in contents.lines() {
        let trimmed = raw_line.trim_end();
        let without_comment = trimmed
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(trimmed);
        let text = without_comment.trim();
        if text.is_empty() {
            continue;
        }
        let keyword = text.split_whitespace().next().unwrap_or("");
        if keyword.eq_ignore_ascii_case("EXPECT") {
            continue;
        }
        if keyword.eq_ignore_ascii_case("HELP") {
            continue;
        }
        if keyword.eq_ignore_ascii_case("WAIT") {
            let rest = text.strip_prefix("WAIT").unwrap_or(text).trim();
            let millis: u64 = rest
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse()
                .expect("WAIT ms");
            ops.push(ScriptOp::Wait(millis));
            continue;
        }
        ops.push(ScriptOp::Command(translate_command(text)));
    }
    ops
}

fn translate_command(line: &str) -> String {
    let mut parts = line.split_whitespace();
    let Some(keyword) = parts.next() else {
        return line.to_owned();
    };
    if keyword.eq_ignore_ascii_case("log") && parts.next().is_none() {
        return format!("tail {QUEEN_LOG_PATH}");
    }
    if keyword.eq_ignore_ascii_case("spawn") {
        let rest = line.strip_prefix(keyword).unwrap_or("").trim();
        if rest.starts_with('{') {
            return line.to_owned();
        }
        let mut args = rest.split_whitespace();
        let Some(role) = args.next() else {
            return line.to_owned();
        };
        let payload = queen::spawn(role, args).expect("spawn payload");
        let payload = payload.trim_end_matches('\n');
        let path = queen::queen_ctl_path();
        return format!("echo {path} {payload}");
    }
    line.to_owned()
}

fn issue_token(secret: &str, role: Role, subject: Option<&str>) -> String {
    let issuer = TicketIssuer::new(secret);
    let budget = match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerHeartbeat | Role::WorkerGpu | Role::WorkerBus | Role::WorkerLora => {
            BudgetSpec::default_heartbeat()
        }
    };
    let claims = TicketClaims::new(
        role,
        budget,
        subject.map(str::to_owned),
        MountSpec::empty(),
        unix_time_ms(),
    );
    let token = issuer.issue(claims).expect("issue token");
    token.encode().expect("encode token")
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn build_validator() -> TicketValidator {
    let mut validator = TicketValidator::new();
    validator.register(Role::Queen, QUEEN_SECRET);
    validator.register(Role::WorkerHeartbeat, WORKER_SECRET);
    validator
}

fn build_abuse_script(invalid_worker_token: &str) -> String {
    format!(
        r#"# Author: Lukas Bower
# Purpose: Transcript abuse coverage for invalid ticket and throttled login.
attach worker-heartbeat {invalid_worker_token}
EXPECT ERR
attach worker-heartbeat {invalid_worker_token}
EXPECT ERR
"#
    )
}

fn build_converge_script() -> String {
    format!(
        r#"# Author: Lukas Bower
# Purpose: Transcript convergence script.
help
attach queen
log
spawn heartbeat ticks=1 ttl_s=30
tail {CONVERGE_TAIL_PATH}
quit
"#
    )
}

fn run_serial_transcript(ops: &[ScriptOp], harness: &mut ConsoleHarness) -> Vec<String> {
    let mut transcript = Vec::new();
    for op in ops {
        match op {
            ScriptOp::Wait(ms) => harness.advance_time(*ms),
            ScriptOp::Command(line) => {
                for byte in line.as_bytes() {
                    match harness.parser.push_byte(*byte) {
                        Ok(Some(cmd)) => transcript.extend(harness.handle_command(cmd)),
                        Ok(None) => {}
                        Err(err) => {
                            transcript.push(render_parse_error(err));
                            let _ = harness.parser.clear_buffer();
                        }
                    }
                }
                match harness.parser.push_byte(b'\n') {
                    Ok(Some(cmd)) => transcript.extend(harness.handle_command(cmd)),
                    Ok(None) => {}
                    Err(err) => {
                        transcript.push(render_parse_error(err));
                        let _ = harness.parser.clear_buffer();
                    }
                }
            }
        }
    }
    transcript
}

fn run_core_transcript(ops: &[ScriptOp], harness: &mut ConsoleHarness) -> Vec<String> {
    let mut transcript = Vec::new();
    for op in ops {
        match op {
            ScriptOp::Wait(ms) => harness.advance_time(*ms),
            ScriptOp::Command(line) => match CommandParser::parse_line_str(line) {
                Ok(cmd) => transcript.extend(harness.handle_command(cmd)),
                Err(err) => transcript.push(render_parse_error(err)),
            },
        }
    }
    transcript
}

fn run_tcp_transcript(script_path: &Path, harness: ConsoleHarness) -> (Vec<String>, Option<u64>) {
    let (port, sent_lines, timing, handle) = spawn_tcp_server(harness);
    let _output = run_cohsh_tcp(script_path, port);
    let _ = handle.join();
    let lines = sent_lines.lock().expect("tcp transcript lock").clone();
    let elapsed_ms = timing.lock().expect("tcp timing lock").take();
    (lines, elapsed_ms)
}

fn spawn_tcp_server(
    mut harness: ConsoleHarness,
) -> (
    u16,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Option<u64>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind tcp");
    let port = listener.local_addr().expect("local addr").port();
    let sent_lines = Arc::new(Mutex::new(Vec::new()));
    let sent_clone = Arc::clone(&sent_lines);
    let timing = Arc::new(Mutex::new(None));
    let timing_clone = Arc::clone(&timing);
    let handle = thread::spawn(move || {
        let mut start_time: Option<Instant> = None;
        let (stream, _) = listener.accept().expect("accept tcp");
        let mut stream = stream;
        let reader_stream = stream.try_clone().expect("clone tcp");
        let mut reader = BufReader::new(reader_stream);
        let record_line = |line: &str| {
            let mut guard = sent_clone.lock().expect("tcp transcript lock");
            guard.push(line.to_owned());
        };
        loop {
            let Some(line) = read_frame(&mut reader) else {
                break;
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("AUTH ") {
                record_line("OK AUTH");
                let _ = write_frame(&mut stream, "OK AUTH");
                continue;
            }
            if start_time.is_none() {
                start_time = Some(Instant::now());
            }
            let command = match CommandParser::parse_line_str(trimmed) {
                Ok(cmd) => cmd,
                Err(err) => {
                    let line = render_parse_error(err);
                    record_line(&line);
                    let _ = write_frame(&mut stream, &line);
                    continue;
                }
            };
            let outputs = harness.handle_command(command);
            for output in outputs {
                record_line(&output);
                let _ = write_frame(&mut stream, &output);
            }
        }
        if let Some(start) = start_time {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let mut guard = timing_clone.lock().expect("tcp timing lock");
            *guard = Some(elapsed_ms);
        }
    });
    (port, sent_lines, timing, handle)
}

fn write_frame(stream: &mut TcpStream, line: &str) -> std::io::Result<()> {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes())?;
    stream.write_all(line.as_bytes())?;
    Ok(())
}

fn read_frame(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let total_len = u32::from_le_bytes(len_buf) as usize;
    if total_len < 4 {
        return None;
    }
    let payload_len = total_len - 4;
    let mut payload = vec![0u8; payload_len];
    if reader.read_exact(&mut payload).is_err() {
        return None;
    }
    String::from_utf8(payload).ok()
}

fn run_cohsh_tcp(script_path: &Path, port: u16) -> String {
    let output = ProcessCommand::new("cargo")
        .current_dir(transcript_support::repo_root())
        .args([
            "run",
            "-p",
            "cohsh",
            "--features",
            "tcp",
            "--",
            "--transport",
            "tcp",
            "--tcp-host",
            "127.0.0.1",
            "--tcp-port",
            &port.to_string(),
            "--auth-token",
            AUTH_TOKEN,
            "--script",
            script_path.to_str().expect("script path"),
        ])
        .output()
        .expect("run cohsh tcp");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("cohsh tcp failed: {stderr}");
    }
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn transcripts_match_across_transports() {
    let boot_script_path = transcript_support::repo_root().join(BOOT_SCRIPT);
    let boot_contents = fs::read_to_string(&boot_script_path).expect("read boot script");
    let boot_ops = parse_script(&boot_contents);

    let invalid_worker_token = issue_token(INVALID_SECRET, Role::WorkerHeartbeat, Some("worker-1"));
    let abuse_contents = build_abuse_script(&invalid_worker_token);
    let abuse_ops = parse_script(&abuse_contents);

    let converge_contents = build_converge_script();
    let converge_ops = parse_script(&converge_contents);

    let transcripts_dir = cohsh_core_output_root();
    let abuse_script_path = transcripts_dir.join("abuse").join("script.coh");
    if let Some(parent) = abuse_script_path.parent() {
        fs::create_dir_all(parent).expect("create abuse script dir");
    }
    fs::write(&abuse_script_path, abuse_contents).expect("write abuse script");

    let converge_script_path = transcripts_dir.join(CONVERGE_SCENARIO).join("script.coh");
    if let Some(parent) = converge_script_path.parent() {
        fs::create_dir_all(parent).expect("create converge script dir");
    }
    fs::write(&converge_script_path, converge_contents).expect("write converge script");

    let scenarios = [
        Scenario {
            name: "boot_v0",
            ops: boot_ops,
            script_path: boot_script_path,
            ack_fanout: CONSOLE_ACK_FANOUT,
        },
        Scenario {
            name: "abuse",
            ops: abuse_ops,
            script_path: abuse_script_path,
            ack_fanout: CONSOLE_ACK_FANOUT,
        },
        Scenario {
            name: CONVERGE_SCENARIO,
            ops: converge_ops,
            script_path: converge_script_path,
            ack_fanout: CONSOLE_ACK_FANOUT,
        },
    ];

    for scenario in scenarios {
        let mut serial_harness = ConsoleHarness::new(build_validator(), scenario.ack_fanout);
        let mut core_harness = ConsoleHarness::new(build_validator(), scenario.ack_fanout);
        let tcp_harness = ConsoleHarness::new(build_validator(), 1);

        let serial_start = Instant::now();
        let serial_lines = run_serial_transcript(&scenario.ops, &mut serial_harness);
        let serial_elapsed = serial_start.elapsed().as_millis() as u64;

        let core_start = Instant::now();
        let core_lines = run_core_transcript(&scenario.ops, &mut core_harness);
        let core_elapsed = core_start.elapsed().as_millis() as u64;

        let (tcp_lines, tcp_elapsed) = run_tcp_transcript(&scenario.script_path, tcp_harness);

        let serial_path = transcript_support::compare_transcript(
            "cohsh-core",
            scenario.name,
            "serial.txt",
            &serial_lines,
        );
        let core_path = transcript_support::compare_transcript(
            "cohsh-core",
            scenario.name,
            "core.txt",
            &core_lines,
        );
        let tcp_path = transcript_support::compare_transcript(
            "cohsh-core",
            scenario.name,
            "tcp.txt",
            &tcp_lines,
        );

        transcript_support::diff_files(&serial_path, &core_path)
            .unwrap_or_else(|diff| panic!("serial vs core drift:\n{diff}"));
        transcript_support::diff_files(&serial_path, &tcp_path)
            .unwrap_or_else(|diff| panic!("serial vs tcp drift:\n{diff}"));

        if scenario.name == CONVERGE_SCENARIO {
            transcript_support::write_timing("cohsh-core", scenario.name, "serial", serial_elapsed);
            transcript_support::write_timing("cohsh-core", scenario.name, "core", core_elapsed);
            transcript_support::write_timing(
                "cohsh-core",
                scenario.name,
                "tcp",
                tcp_elapsed.unwrap_or(0),
            );
        }
    }
}
