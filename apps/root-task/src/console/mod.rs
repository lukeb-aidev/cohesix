// Author: Lukas Bower

#![allow(unsafe_code)]

//! Shared console command parser and rate limiter for the root task console.

#[cfg(feature = "kernel")]
mod io;
#[cfg(feature = "kernel")]
pub use io::Console;

use core::fmt;

use crate::platform::Platform;

use heapless::String;

#[cfg(feature = "kernel")]
use crate::sel4::{self, BootInfoExt};
#[cfg(feature = "kernel")]
use crate::uart::pl011;
#[cfg(feature = "kernel")]
use core::fmt::Write as FmtWrite;
#[cfg(feature = "kernel")]
use sel4_sys::seL4_CPtr;

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
    BootInfo,
    Caps,
    Mem,
    Ping,
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

#[cfg(feature = "canonical_cspace")]
pub fn start(ep_slot: u32, _bi: &sel4_sys::seL4_BootInfo) {
    log::info!("[console] ready on endpoint slot=0x{:04x}", ep_slot);
    loop {
        let msg =
            unsafe { sel4_sys::seL4_Recv(ep_slot as sel4_sys::seL4_CPtr, core::ptr::null_mut()) };
        match msg {
            0 => log::info!("[console] recv: help | bi | ls-caps | echo"),
            _ => log::info!("[console] unknown verb id={}", msg),
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

#[cfg(feature = "kernel")]
pub struct CohesixConsole {
    ep_slot: seL4_CPtr,
    uart_slot: seL4_CPtr,
    parser: CommandParser,
}

#[cfg(feature = "kernel")]
impl CohesixConsole {
    #[must_use]
    pub fn new(ep_slot: seL4_CPtr, uart_slot: seL4_CPtr) -> Self {
        Self {
            ep_slot,
            uart_slot,
            parser: CommandParser::new(),
        }
    }

    fn emit(&self, text: &str) {
        pl011::write_str(text);
    }

    fn emit_line(&self, text: &str) {
        self.emit(text);
        self.emit("\r\n");
    }

    fn prompt(&self) {
        self.emit("cohesix> ");
    }

    fn bootinfo(&self) -> &'static sel4_sys::seL4_BootInfo {
        unsafe { &*sel4_sys::seL4_GetBootInfo() }
    }

    fn print_help(&self) {
        self.emit_line("Commands:");
        self.emit_line("  help  - Show this help");
        self.emit_line("  bi    - Show bootinfo summary");
        self.emit_line("  caps  - Show capability slots");
        self.emit_line("  mem   - Show untyped summary");
        self.emit_line("  ping  - Respond with pong");
        self.emit_line("  quit  - Exit the console session");
    }

    fn print_bootinfo(&self) {
        let bi = self.bootinfo();
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[bi] node_bits={} empty=[0x{:04x}..0x{:04x}) ",
            bi.initThreadCNodeSizeBits, bi.empty.start, bi.empty.end,
        );
        if let Some(ptr) = bi.ipc_buffer_ptr() {
            let addr = ptr.as_ptr() as usize;
            let width = core::mem::size_of::<usize>() * 2;
            let _ = write!(line, "ipc=0x{addr:0width$x}");
        } else {
            let _ = line.push_str("ipc=<none>");
        }
        self.emit_line(line.as_str());
    }

    fn print_caps(&self) {
        let bi = self.bootinfo();
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[caps] root=0x{:04x} ep=0x{:04x} uart=0x{:04x}",
            bi.init_cnode_cap(),
            self.ep_slot,
            self.uart_slot,
        );
        self.emit_line(line.as_str());
    }

    fn print_mem(&self) {
        let bi = self.bootinfo();
        let count = (bi.untyped.end - bi.untyped.start) as usize;
        let mut ram_ut = 0usize;
        for desc in bi.untypedList.iter().take(count) {
            if desc.isDevice == 0 {
                ram_ut += 1;
            }
        }
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[mem] untyped caps={} ram_ut={} device_ut={}",
            count,
            ram_ut,
            count.saturating_sub(ram_ut),
        );
        self.emit_line(line.as_str());
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Help => self.print_help(),
            Command::BootInfo => self.print_bootinfo(),
            Command::Caps => self.print_caps(),
            Command::Mem => self.print_mem(),
            Command::Ping => self.emit_line("pong"),
            Command::Quit => self.emit_line("quit not supported on root console"),
            Command::Log => self.emit_line("log streaming unavailable"),
            Command::Attach { .. }
            | Command::Tail { .. }
            | Command::Spawn(_)
            | Command::Kill(_) => self.emit_line("command not implemented"),
        }
    }

    fn reset_parser(&mut self) {
        self.parser = CommandParser::new();
    }

    fn echo(&self, byte: u8) {
        match byte {
            b'\r' | b'\n' => self.emit("\r\n"),
            0x08 | 0x7f => self.emit("\x08 \x08"),
            _ => {
                let mut buf = [0u8; 4];
                let s = char::from(byte).encode_utf8(&mut buf);
                self.emit(s);
            }
        }
    }

    pub fn run(&mut self) -> ! {
        pl011::init_pl011();
        log::info!(
            "[console] starting root shell ep=0x{ep:04x} uart=0x{uart:04x}",
            ep = self.ep_slot,
            uart = self.uart_slot,
        );
        self.emit_line("Cohesix console ready.");
        self.prompt();

        loop {
            if let Some(byte) = pl011::poll_byte() {
                let mapped = match byte {
                    b'\r' | b'\n' => {
                        self.echo(b'\n');
                        Some(b'\n')
                    }
                    0x08 | 0x7f => {
                        self.echo(byte);
                        Some(byte)
                    }
                    _ => {
                        self.echo(byte);
                        Some(byte)
                    }
                };

                if let Some(token) = mapped {
                    match self.parser.push_byte(token) {
                        Ok(Some(command)) => {
                            self.handle_command(command);
                            self.reset_parser();
                            self.prompt();
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let mut line = String::<128>::new();
                            let _ = write!(line, "error: {err}");
                            self.emit_line(line.as_str());
                            self.reset_parser();
                            self.prompt();
                        }
                    }
                }
            } else {
                sel4::yield_now();
            }
        }
    }
}

#[cfg(feature = "kernel")]
impl FmtWrite for CohesixConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.emit(s);
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
            v if v.eq_ignore_ascii_case("bi") => Ok(Command::BootInfo),
            v if v.eq_ignore_ascii_case("caps") => Ok(Command::Caps),
            v if v.eq_ignore_ascii_case("mem") => Ok(Command::Mem),
            v if v.eq_ignore_ascii_case("ping") => Ok(Command::Ping),
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
