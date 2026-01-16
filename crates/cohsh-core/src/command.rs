// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Console command parser and rate limiter shared across Cohesix.
// Author: Lukas Bower

//! Console command parser and rate limiter shared across Cohesix.

use core::fmt;

use heapless::String;

use crate::verb::ConsoleVerb;

/// Maximum length accepted for a single console line.
pub const MAX_LINE_LEN: usize = 256;

/// Maximum number of characters permitted in a role identifier when parsing `attach`.
pub const MAX_ROLE_LEN: usize = 16;
/// Maximum number of characters accepted for ticket material presented to `attach`.
pub const MAX_TICKET_LEN: usize = 192;
/// Maximum number of characters accepted for file paths.
pub const MAX_PATH_LEN: usize = 96;
/// Maximum number of characters accepted for JSON payloads.
pub const MAX_JSON_LEN: usize = 192;
/// Maximum number of characters accepted for worker identifiers.
pub const MAX_ID_LEN: usize = 32;
/// Maximum number of characters accepted for echo payloads.
pub const MAX_ECHO_LEN: usize = 128;

const MAX_FAILED_LOGINS: u32 = 3;
const RATE_LIMIT_WINDOW_MS: u64 = 60_000;
const COOLDOWN_MS: u64 = 90_000;

/// Console command variants supported by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Command {
    Help,
    BootInfo,
    Caps,
    Mem,
    Ping,
    Test,
    Attach {
        role: String<MAX_ROLE_LEN>,
        ticket: Option<String<MAX_TICKET_LEN>>,
    },
    Tail {
        path: String<MAX_PATH_LEN>,
    },
    Cat {
        path: String<MAX_PATH_LEN>,
    },
    Ls {
        path: String<MAX_PATH_LEN>,
    },
    Echo {
        path: String<MAX_PATH_LEN>,
        payload: String<MAX_ECHO_LEN>,
    },
    Log,
    Quit,
    NetTest,
    NetStats,
    Spawn(String<MAX_JSON_LEN>),
    Kill(String<MAX_ID_LEN>),
    CacheLog {
        count: Option<u16>,
    },
}

impl Command {
    /// Return the verb associated with the command.
    #[must_use]
    pub fn verb(&self) -> ConsoleVerb {
        match self {
            Self::Help => ConsoleVerb::Help,
            Self::BootInfo => ConsoleVerb::BootInfo,
            Self::Caps => ConsoleVerb::Caps,
            Self::Mem => ConsoleVerb::Mem,
            Self::Ping => ConsoleVerb::Ping,
            Self::Test => ConsoleVerb::Test,
            Self::Attach { .. } => ConsoleVerb::Attach,
            Self::Tail { .. } => ConsoleVerb::Tail,
            Self::Cat { .. } => ConsoleVerb::Cat,
            Self::Ls { .. } => ConsoleVerb::Ls,
            Self::Echo { .. } => ConsoleVerb::Echo,
            Self::Log => ConsoleVerb::Log,
            Self::Quit => ConsoleVerb::Quit,
            Self::NetTest => ConsoleVerb::NetTest,
            Self::NetStats => ConsoleVerb::NetStats,
            Self::Spawn(_) => ConsoleVerb::Spawn,
            Self::Kill(_) => ConsoleVerb::Kill,
            Self::CacheLog { .. } => ConsoleVerb::CacheLog,
        }
    }
}

/// Errors surfaced by the console parser.
#[derive(Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ConsoleError {
    LineTooLong,
    EmptyLine,
    InvalidVerb,
    MissingArgument(&'static str),
    ValueTooLong(&'static str),
    RateLimited(u64),
    InvalidValue(&'static str),
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong => write!(f, "console line exceeded maximum length"),
            Self::EmptyLine => write!(f, "empty command"),
            Self::InvalidVerb => write!(f, "unsupported console command"),
            Self::MissingArgument(arg) => write!(f, "missing required argument: {arg}"),
            Self::ValueTooLong(arg) => write!(f, "argument {arg} exceeds allowed length"),
            Self::InvalidValue(arg) => write!(f, "invalid value for argument {arg}"),
            Self::RateLimited(delay) => {
                write!(
                    f,
                    "authentication attempts temporarily blocked ({delay} ms)"
                )
            }
        }
    }
}

/// Simple leaky bucket rate limiter tracking failed login attempts.
#[derive(Debug, Default)]
pub struct RateLimiter {
    failures: u32,
    window_start_ms: Option<u64>,
    blocked_until_ms: Option<u64>,
}

impl RateLimiter {
    /// Register the outcome of a login attempt.
    pub fn register_attempt(&mut self, success: bool, now_ms: u64) -> Result<(), ConsoleError> {
        if success {
            self.failures = 0;
            self.window_start_ms = None;
            self.blocked_until_ms = None;
            return Ok(());
        }

        if let Some(until) = self.blocked_until_ms {
            if now_ms < until {
                return Err(ConsoleError::RateLimited(until - now_ms));
            }
        }

        let window_start = match self.window_start_ms {
            Some(start) if now_ms.saturating_sub(start) <= RATE_LIMIT_WINDOW_MS => start,
            _ => {
                self.failures = 0;
                now_ms
            }
        };

        self.window_start_ms = Some(window_start);
        self.failures += 1;

        if self.failures >= MAX_FAILED_LOGINS {
            let blocked_until = now_ms + COOLDOWN_MS;
            self.blocked_until_ms = Some(blocked_until);
            self.failures = 0;
            self.window_start_ms = None;
            return Err(ConsoleError::RateLimited(
                blocked_until.saturating_sub(now_ms),
            ));
        }

        Ok(())
    }
}

/// Finite-state console parser building commands from incoming bytes.
#[derive(Debug, Default)]
pub struct CommandParser {
    buffer: String<MAX_LINE_LEN>,
    rate_limiter: RateLimiter,
}

impl CommandParser {
    /// Create a new parser instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear any partially buffered command bytes.
    pub fn clear_buffer(&mut self) -> bool {
        let had_data = !self.buffer.is_empty();
        self.buffer.clear();
        had_data
    }

    /// Consume a single input byte, returning a command when a full line is available.
    pub fn push_byte(&mut self, byte: u8) -> Result<Option<Command>, ConsoleError> {
        match byte {
            b'\r' => Ok(None),
            b'\n' => {
                if self.buffer.is_empty() {
                    self.buffer.clear();
                    return Err(ConsoleError::EmptyLine);
                }
                let command = parse_line_inner(self.buffer.as_str())?;
                self.buffer.clear();
                Ok(Some(command))
            }
            0x08 | 0x7f => {
                self.buffer.pop();
                Ok(None)
            }
            _ => {
                if self.buffer.len() >= MAX_LINE_LEN {
                    self.buffer.clear();
                    return Err(ConsoleError::LineTooLong);
                }
                let ch = byte as char;
                if ch.is_control() {
                    return Ok(None);
                }
                self.buffer
                    .push(ch)
                    .map_err(|_| ConsoleError::LineTooLong)?;
                Ok(None)
            }
        }
    }

    /// Update the login rate limiter with the outcome of an authentication attempt.
    pub fn record_login_attempt(&mut self, success: bool, now_ms: u64) -> Result<(), ConsoleError> {
        self.rate_limiter.register_attempt(success, now_ms)
    }

    /// Parse a full command line without consuming parser state.
    pub fn parse_line_str(line: &str) -> Result<Command, ConsoleError> {
        parse_line_inner(line)
    }
}

fn parse_line_inner(line: &str) -> Result<Command, ConsoleError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ConsoleError::EmptyLine);
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap();
    let remainder = parts.next().map(str::trim).unwrap_or("");
    let verb = ConsoleVerb::from_token(verb).ok_or(ConsoleError::InvalidVerb)?;

    match verb {
        ConsoleVerb::Help => Ok(Command::Help),
        ConsoleVerb::BootInfo => Ok(Command::BootInfo),
        ConsoleVerb::Caps => Ok(Command::Caps),
        ConsoleVerb::Mem => Ok(Command::Mem),
        ConsoleVerb::Ping => Ok(Command::Ping),
        ConsoleVerb::Test => {
            if !remainder.is_empty() {
                return Err(ConsoleError::InvalidValue("test"));
            }
            Ok(Command::Test)
        }
        ConsoleVerb::NetTest => Ok(Command::NetTest),
        ConsoleVerb::NetStats => Ok(Command::NetStats),
        ConsoleVerb::Log => Ok(Command::Log),
        ConsoleVerb::CacheLog => {
            if remainder.is_empty() {
                return Ok(Command::CacheLog { count: None });
            }
            let count = remainder
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .parse::<u16>()
                .map_err(|_| ConsoleError::InvalidValue("count"))?;
            Ok(Command::CacheLog { count: Some(count) })
        }
        ConsoleVerb::Quit => Ok(Command::Quit),
        ConsoleVerb::Tail => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("path"));
            }
            let path = remainder.split_whitespace().next().unwrap();
            let mut owned = String::new();
            owned
                .push_str(path)
                .map_err(|_| ConsoleError::ValueTooLong("path"))?;
            Ok(Command::Tail { path: owned })
        }
        ConsoleVerb::Cat => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("path"));
            }
            let path = remainder.split_whitespace().next().unwrap();
            let mut owned = String::new();
            owned
                .push_str(path)
                .map_err(|_| ConsoleError::ValueTooLong("path"))?;
            Ok(Command::Cat { path: owned })
        }
        ConsoleVerb::Ls => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("path"));
            }
            let path = remainder.split_whitespace().next().unwrap();
            let mut owned = String::new();
            owned
                .push_str(path)
                .map_err(|_| ConsoleError::ValueTooLong("path"))?;
            Ok(Command::Ls { path: owned })
        }
        ConsoleVerb::Echo => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("path"));
            }
            let mut echo_parts = remainder.splitn(2, char::is_whitespace);
            let path = echo_parts.next().unwrap();
            let payload = echo_parts.next().unwrap_or("").trim_start();
            if payload.is_empty() {
                return Err(ConsoleError::MissingArgument("payload"));
            }
            let mut path_buf = String::new();
            path_buf
                .push_str(path)
                .map_err(|_| ConsoleError::ValueTooLong("path"))?;
            let mut payload_buf = String::new();
            payload_buf
                .push_str(payload)
                .map_err(|_| ConsoleError::ValueTooLong("payload"))?;
            Ok(Command::Echo {
                path: path_buf,
                payload: payload_buf,
            })
        }
        ConsoleVerb::Attach => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("role"));
            }
            let mut attach_parts = remainder.splitn(2, char::is_whitespace);
            let role = attach_parts.next().unwrap();
            let mut role_buf = String::new();
            role_buf
                .push_str(role)
                .map_err(|_| ConsoleError::ValueTooLong("role"))?;
            let ticket = attach_parts.next().map(|raw| {
                let mut ticket_buf = String::new();
                ticket_buf
                    .push_str(raw)
                    .map_err(|_| ConsoleError::ValueTooLong("ticket"))?;
                Ok(ticket_buf)
            });
            let ticket = match ticket {
                Some(Ok(value)) => Some(value),
                Some(Err(err)) => return Err(err),
                None => None,
            };
            Ok(Command::Attach {
                role: role_buf,
                ticket,
            })
        }
        ConsoleVerb::Spawn => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("payload"));
            }
            let mut buf = String::new();
            buf.push_str(remainder)
                .map_err(|_| ConsoleError::ValueTooLong("payload"))?;
            Ok(Command::Spawn(buf))
        }
        ConsoleVerb::Kill => {
            if remainder.is_empty() {
                return Err(ConsoleError::MissingArgument("worker"));
            }
            let ident = remainder.split_whitespace().next().unwrap();
            let mut buf = String::new();
            buf.push_str(ident)
                .map_err(|_| ConsoleError::ValueTooLong("worker"))?;
            Ok(Command::Kill(buf))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use crate::verb::ALL_VERBS;

    fn heapless_str<const N: usize>(value: &str) -> heapless::String<N> {
        let mut buf = heapless::String::new();
        buf.push_str(value).expect("heapless string");
        buf
    }

    fn parse(input: &str) -> Result<Command, ConsoleError> {
        let mut parser = CommandParser::new();
        for byte in input.as_bytes() {
            if let Some(cmd) = parser.push_byte(*byte)? {
                return Ok(cmd);
            }
        }
        parser.push_byte(b'\n')?.ok_or(ConsoleError::EmptyLine)
    }

    #[test]
    fn help_command_parses() {
        assert_eq!(parse("help\n").unwrap(), Command::Help);
    }

    #[test]
    fn command_verbs_match_console_specs() {
        let commands = [
            Command::Help,
            Command::BootInfo,
            Command::Caps,
            Command::Mem,
            Command::Ping,
            Command::Test,
            Command::NetTest,
            Command::NetStats,
            Command::Log,
            Command::CacheLog { count: None },
            Command::Quit,
            Command::Tail {
                path: heapless_str("/log/queen.log"),
            },
            Command::Cat {
                path: heapless_str("/log/queen.log"),
            },
            Command::Ls {
                path: heapless_str("/log"),
            },
            Command::Echo {
                path: heapless_str("/log/queen.log"),
                payload: heapless_str("hello"),
            },
            Command::Attach {
                role: heapless_str("queen"),
                ticket: None,
            },
            Command::Spawn(heapless_str("{\"spawn\":\"heartbeat\"}")),
            Command::Kill(heapless_str("worker-1")),
        ];

        let verbs: Vec<ConsoleVerb> = commands.iter().map(Command::verb).collect();
        let expected: Vec<ConsoleVerb> = ALL_VERBS.iter().copied().collect();
        assert_eq!(verbs, expected, "console verb inventory drift");
    }

    #[test]
    fn attach_requires_role() {
        assert_eq!(
            parse("attach\n").unwrap_err(),
            ConsoleError::MissingArgument("role")
        );
    }

    #[test]
    fn tail_accepts_paths() {
        let cmd = parse("tail /log/queen.log\n").unwrap();
        match cmd {
            Command::Tail { path } => assert_eq!(path.as_str(), "/log/queen.log"),
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn cat_accepts_paths() {
        let cmd = parse("cat /log/queen.log\n").unwrap();
        match cmd {
            Command::Cat { path } => assert_eq!(path.as_str(), "/log/queen.log"),
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn ls_accepts_paths() {
        let cmd = parse("ls /log\n").unwrap();
        match cmd {
            Command::Ls { path } => assert_eq!(path.as_str(), "/log"),
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn echo_accepts_path_and_payload() {
        let cmd = parse("echo /log/queen.log hello-world\n").unwrap();
        match cmd {
            Command::Echo { path, payload } => {
                assert_eq!(path.as_str(), "/log/queen.log");
                assert_eq!(payload.as_str(), "hello-world");
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn spawn_collects_payload() {
        let cmd = parse("spawn {\"spawn\":\"heartbeat\"}\n").unwrap();
        match cmd {
            Command::Spawn(payload) => assert!(payload.contains("heartbeat")),
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn rate_limiter_blocks_after_failures() {
        let mut parser = CommandParser::new();
        assert!(parser.record_login_attempt(false, 1_000).is_ok());
        assert!(parser.record_login_attempt(false, 10_000).is_ok());
        assert!(parser.record_login_attempt(false, 20_000).is_err());
    }

    #[test]
    fn nettest_command_parses() {
        assert_eq!(parse("nettest\n").unwrap(), Command::NetTest);
    }

    #[test]
    fn netstats_command_parses() {
        assert_eq!(parse("netstats\n").unwrap(), Command::NetStats);
    }

    #[test]
    fn test_command_parses() {
        assert_eq!(parse("test\n").unwrap(), Command::Test);
    }

    #[test]
    fn cachelog_parses_default() {
        assert_eq!(
            parse("cachelog\n").unwrap(),
            Command::CacheLog { count: None }
        );
    }

    #[test]
    fn cachelog_parses_count() {
        assert_eq!(
            parse("cachelog 32\n").unwrap(),
            Command::CacheLog { count: Some(32) }
        );
    }

    #[test]
    fn cachelog_rejects_invalid_count() {
        assert_eq!(
            parse("cachelog nope\n").unwrap_err(),
            ConsoleError::InvalidValue("count")
        );
    }
}
