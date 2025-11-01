// Author: Lukas Bower

//! Shared console command parser and rate limiter for the root task console.

#[cfg(feature = "kernel")]
mod io;
#[cfg(feature = "kernel")]
pub use io::Console;

use core::fmt;

use crate::platform::Platform;

#[cfg(feature = "canonical_cspace")]
use core::cell::OnceCell;
#[cfg(feature = "canonical_cspace")]
use core::mem;

#[cfg(feature = "canonical_cspace")]
use crate::bootstrap::cspace::DestCNode;
#[cfg(feature = "canonical_cspace")]
use crate::bootstrap::log::force_uart_line;
#[cfg(feature = "canonical_cspace")]
use crate::sel4::{self, MSG_MAX_WORDS};
#[cfg(feature = "canonical_cspace")]
use heapless::Vec;

#[cfg(feature = "canonical_cspace")]
const MAX_CANONICAL_REPLY_BYTES: usize = 224;

#[cfg(feature = "canonical_cspace")]
static MINI_CONSOLE: OnceCell<MiniConsole> = OnceCell::new();

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

#[cfg(feature = "canonical_cspace")]
struct MiniConsole {
    empty_start: u32,
    empty_end: u32,
    node_id: u32,
    node_count: u32,
}

#[cfg(feature = "canonical_cspace")]
impl MiniConsole {
    const fn new(empty_start: u32, empty_end: u32, node_id: u32, node_count: u32) -> Self {
        Self {
            empty_start,
            empty_end,
            node_id,
            node_count,
        }
    }

    fn log_line(&self, body: &str) {
        let mut line = String::<MAX_CANONICAL_REPLY_BYTES>::new();
        if line.push_str("[console] ").is_ok() && line.push_str(body).is_ok() {
            force_uart_line(line.as_str());
        } else {
            force_uart_line("[console] <line truncated>");
        }
    }

    fn decode_command(&self, info: sel4_sys::seL4_MessageInfo) -> Option<String<{ MAX_LINE_LEN }>> {
        let mut command = String::<{ MAX_LINE_LEN }>::new();
        let length_words = info.length() as usize;
        if length_words == 0 {
            return Some(command);
        }
        let word_bytes = mem::size_of::<sel4_sys::seL4_Word>();
        for index in 0..length_words {
            let word = crate::sel4::message_register(index);
            for byte in word.to_le_bytes().into_iter().take(word_bytes) {
                if byte == 0 {
                    return Some(command);
                }
                if byte == b'\n' || byte == b'\r' {
                    return Some(command);
                }
                if !byte.is_ascii() {
                    continue;
                }
                if command.push(byte as char).is_err() {
                    return Some(command);
                }
            }
        }
        Some(command)
    }

    fn reply_with(&self, text: &str) {
        let mut framed: Vec<sel4_sys::seL4_Word, MSG_MAX_WORDS> = Vec::new();
        let word_bytes = mem::size_of::<sel4_sys::seL4_Word>();
        for chunk in text.as_bytes().chunks(word_bytes) {
            let mut word = 0usize;
            for (shift, byte) in chunk.iter().enumerate() {
                word |= (*byte as usize) << (shift * 8);
            }
            if framed.push(word).is_err() {
                break;
            }
        }
        let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, framed.len() as sel4_sys::seL4_Word);
        for (index, word) in framed.iter().enumerate() {
            crate::sel4::set_message_register(index, *word);
        }
        crate::sel4::reply(info);
    }

    fn handle(&self, info: sel4_sys::seL4_MessageInfo, _badge: sel4_sys::seL4_Word) -> bool {
        let Some(mut command) = self.decode_command(info) else {
            return false;
        };
        while command
            .chars()
            .last()
            .is_some_and(|ch| ch == ' ' || ch == '\t')
        {
            command.pop();
        }
        let trimmed = command.as_str().trim();
        if trimmed.is_empty() {
            self.log_line("empty command");
            self.reply_with("error: empty command");
            return true;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap();
        let rest = parts.next().map(str::trim).unwrap_or("");
        if verb.eq_ignore_ascii_case("help") {
            self.log_line("help");
            self.reply_with("help, bi, ls-caps, echo <text>");
            true
        } else if verb.eq_ignore_ascii_case("bi") {
            let mut response = String::<MAX_CANONICAL_REPLY_BYTES>::new();
            let _ = write!(
                &mut response,
                "node={} nodes={} empty=[0x{:04x}..0x{:04x})",
                self.node_id, self.node_count, self.empty_start, self.empty_end,
            );
            self.log_line("bi");
            self.reply_with(response.as_str());
            true
        } else if verb.eq_ignore_ascii_case("ls-caps") {
            let mut response = String::<MAX_CANONICAL_REPLY_BYTES>::new();
            let _ = write!(
                &mut response,
                "init-cnode=0x{:04x} window=[0x{:04x}..0x{:04x})",
                sel4::seL4_CapInitThreadCNode,
                self.empty_start,
                self.empty_end,
            );
            self.log_line("ls-caps");
            self.reply_with(response.as_str());
            true
        } else if verb.eq_ignore_ascii_case("echo") {
            let mut echoed = String::<MAX_CANONICAL_REPLY_BYTES>::new();
            if echoed.push_str(rest).is_err() {
                self.reply_with("error: echo too long");
            } else {
                self.reply_with(echoed.as_str());
            }
            self.log_line("echo");
            true
        } else {
            let mut response = String::<MAX_CANONICAL_REPLY_BYTES>::new();
            let _ = write!(&mut response, "error: unknown verb '{verb}'");
            self.log_line(response.as_str());
            self.reply_with(response.as_str());
            true
        }
    }
}

#[cfg(feature = "canonical_cspace")]
fn mini_console() -> Option<&'static MiniConsole> {
    MINI_CONSOLE.get()
}

#[cfg(feature = "canonical_cspace")]
#[must_use]
pub fn start(dest: &DestCNode, bi: &sel4_sys::seL4_BootInfo) -> bool {
    if mini_console().is_some() {
        return true;
    }
    let endpoint_slot = dest.empty_start;
    let endpoint = endpoint_slot as sel4_sys::seL4_CPtr;
    sel4::set_ep(endpoint);
    let console = MiniConsole::new(dest.empty_start, dest.empty_end, bi.nodeId, bi.numNodes);
    let _ = MINI_CONSOLE.set(console);
    let mut banner = String::<MAX_CANONICAL_REPLY_BYTES>::new();
    let _ = write!(banner, "[console] ready on slot=0x{:04x}", endpoint);
    force_uart_line(banner.as_str());
    true
}

#[cfg(feature = "canonical_cspace")]
pub fn try_handle_message(info: sel4_sys::seL4_MessageInfo, badge: sel4_sys::seL4_Word) -> bool {
    let Some(console) = mini_console() else {
        return false;
    };
    if info.length() == 0 && badge == 0 {
        return false;
    }
    console.handle(info, badge)
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
