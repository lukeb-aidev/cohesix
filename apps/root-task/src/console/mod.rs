// Author: Lukas Bower

//! Shared console command parser and rate limiter for the root task console.

#[cfg(feature = "kernel")]
mod io;
#[cfg(feature = "kernel")]
pub use io::Console;

use core::fmt;

use crate::platform::Platform;

use heapless::String;

/// Maximum length accepted for a single console line.
pub const MAX_LINE_LEN: usize = 128;

/// Maximum number of characters permitted in a role identifier when parsing `attach`.
pub const MAX_ROLE_LEN: usize = 16;
/// Maximum number of characters accepted for ticket material presented to `attach`.
pub const MAX_TICKET_LEN: usize = 128;
const MAX_PATH_LEN: usize = 96;
const MAX_JSON_LEN: usize = 192;
const MAX_ID_LEN: usize = 32;

const MAX_FAILED_LOGINS: u32 = 3;
const RATE_LIMIT_WINDOW_MS: u64 = 60_000;
const COOLDOWN_MS: u64 = 90_000;

/// Console command variants supported by the parser.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Command {
    Help,
    Attach {
        role: String<MAX_ROLE_LEN>,
        ticket: Option<String<MAX_TICKET_LEN>>,
    },
    Tail {
        path: String<MAX_PATH_LEN>,
    },
    Log,
    Quit,
    Spawn(String<MAX_JSON_LEN>),
    Kill(String<MAX_ID_LEN>),
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
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong => write!(f, "console line exceeded maximum length"),
            Self::EmptyLine => write!(f, "empty command"),
            Self::InvalidVerb => write!(f, "unsupported console command"),
            Self::MissingArgument(arg) => write!(f, "missing required argument: {arg}"),
            Self::ValueTooLong(arg) => write!(f, "argument {arg} exceeds allowed length"),
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

/// Minimal console loop used during early kernel bring-up.
pub fn run<P: Platform>(platform: &P) -> ! {
    use core::fmt::Write as _;

    struct Adapter<'a, P: Platform>(&'a P);

    impl<'a, P: Platform> core::fmt::Write for Adapter<'a, P> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.as_bytes() {
                self.0.putc(*byte);
            }
            Ok(())
        }
    }

    let mut writer = Adapter(platform);
    let _ = writeln!(writer, "[cohesix:root-task] online");

    loop {
        if let Some(byte) = platform.getc_nonblock() {
            platform.putc(byte);
            if byte == b'\r' || byte == b'\n' {
                let _ = writeln!(writer);
            }
        }
        core::hint::spin_loop();
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

    /// Consume a single input byte, returning a command when a full line is available.
    pub fn push_byte(&mut self, byte: u8) -> Result<Option<Command>, ConsoleError> {
        match byte {
            b'\r' => Ok(None),
            b'\n' => {
                if self.buffer.is_empty() {
                    self.buffer.clear();
                    return Err(ConsoleError::EmptyLine);
                }
                let command = self.parse_line()?;
                self.buffer.clear();
                Ok(Some(command))
            }
            0x08 | 0x7f => {
                self.buffer.pop();
                Ok(None)
            }
            _ => {
                if self.buffer.len() >= MAX_LINE_LEN - 1 {
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

    fn parse_line(&self) -> Result<Command, ConsoleError> {
        let line = self.buffer.trim();
        if line.is_empty() {
            return Err(ConsoleError::EmptyLine);
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap();
        let remainder = parts.next().map(str::trim).unwrap_or("");
        match verb {
            v if v.eq_ignore_ascii_case("help") => Ok(Command::Help),
            v if v.eq_ignore_ascii_case("log") => Ok(Command::Log),
            v if v.eq_ignore_ascii_case("quit") => Ok(Command::Quit),
            v if v.eq_ignore_ascii_case("tail") => {
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
            v if v.eq_ignore_ascii_case("attach") => {
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
            v if v.eq_ignore_ascii_case("spawn") => {
                if remainder.is_empty() {
                    return Err(ConsoleError::MissingArgument("payload"));
                }
                let mut buf = String::new();
                buf.push_str(remainder)
                    .map_err(|_| ConsoleError::ValueTooLong("payload"))?;
                Ok(Command::Spawn(buf))
            }
            v if v.eq_ignore_ascii_case("kill") => {
                if remainder.is_empty() {
                    return Err(ConsoleError::MissingArgument("worker"));
                }
                let ident = remainder.split_whitespace().next().unwrap();
                let mut buf = String::new();
                buf.push_str(ident)
                    .map_err(|_| ConsoleError::ValueTooLong("worker"))?;
                Ok(Command::Kill(buf))
            }
            _ => Err(ConsoleError::InvalidVerb),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
