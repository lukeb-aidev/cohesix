// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix shell prototype used to exercise attach and tail flows against a mocked transport.
//!
//! The real CLI will speak Secure9P to the NineDoor server. Milestone 1 provides
//! a deterministic simulation so that operators and tests can observe the root
//! task log stream while the transport stack is still under construction.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use cohesix_ticket::Role;
use secure9p_wire::{FrameHeader, SessionId};

/// Result of executing a single shell command.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandStatus {
    /// Continue reading commands.
    Continue,
    /// Exit the shell loop.
    Quit,
}

/// Simple representation of an attached session.
#[derive(Debug, Clone)]
pub struct Session {
    id: SessionId,
    role: Role,
}

impl Session {
    /// Construct a new session wrapper.
    pub fn new(id: SessionId, role: Role) -> Self {
        Self { id, role }
    }

    /// Return the role associated with this session.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Return the session identifier.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }
}

/// Transport abstraction used by the shell to interact with the system.
pub trait Transport {
    /// Attach to the transport using the specified role and optional ticket payload.
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session>;

    /// Stream a log-like file and return the accumulated contents.
    fn tail(&self, session: &Session, path: &str) -> Result<Vec<String>>;
}

/// Mocked transport that simulates a subset of the NineDoor behaviours.
#[derive(Debug, Clone)]
pub struct MockTransport {
    logs: HashMap<String, Vec<String>>,
    next_session: u64,
}

impl Default for MockTransport {
    fn default() -> Self {
        let mut logs = HashMap::new();
        logs.insert(
            "/log/queen.log".to_owned(),
            vec![
                "Cohesix boot: root-task online (ticket role: Queen)".to_owned(),
                format!(
                    "spawned user-component endpoint {:?}",
                    FrameHeader::new(SessionId::from_raw(1), 0)
                ),
                "tick 1".to_owned(),
                "PING 1".to_owned(),
                "PONG 1".to_owned(),
                "tick 2".to_owned(),
                "tick 3".to_owned(),
                "root-task shutdown".to_owned(),
            ],
        );
        Self {
            logs,
            next_session: 1,
        }
    }
}

impl Transport for MockTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<Session> {
        let id = SessionId::from_raw(self.next_session);
        self.next_session += 1;
        Ok(Session::new(id, role))
    }

    fn tail(&self, session: &Session, path: &str) -> Result<Vec<String>> {
        if path != "/log/queen.log" {
            return Err(anyhow!("path {path} is unsupported in the mock"));
        }
        if session.role() != Role::Queen {
            return Err(anyhow!(
                "role {role:?} cannot tail {path}",
                role = session.role()
            ));
        }
        self.logs
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow!("path {path} not found"))
    }
}

/// Clap-compatible role selector used by the CLI entry point.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum RoleArg {
    /// Queen orchestration role.
    Queen,
    /// Worker heartbeat role.
    WorkerHeartbeat,
}

impl fmt::Display for RoleArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queen => write!(f, "queen"),
            Self::WorkerHeartbeat => write!(f, "worker-heartbeat"),
        }
    }
}

impl From<RoleArg> for Role {
    fn from(value: RoleArg) -> Self {
        match value {
            RoleArg::Queen => Role::Queen,
            RoleArg::WorkerHeartbeat => Role::WorkerHeartbeat,
        }
    }
}

/// Shell driver responsible for parsing commands and invoking the transport.
pub struct Shell<T: Transport, W: Write> {
    transport: T,
    session: Option<Session>,
    writer: W,
}

impl<T: Transport, W: Write> Shell<T, W> {
    /// Create a new shell given a transport and output writer.
    pub fn new(transport: T, writer: W) -> Self {
        Self {
            transport,
            session: None,
            writer,
        }
    }

    /// Write a line directly to the shell output.
    pub fn write_line(&mut self, message: &str) -> Result<()> {
        writeln!(self.writer, "{message}")?;
        Ok(())
    }

    /// Attach to the transport using the supplied role and optional ticket payload.
    pub fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<()> {
        let session = self.transport.attach(role, ticket)?;
        writeln!(
            self.writer,
            "attached session {:?} as {:?}",
            session.id(),
            session.role()
        )?;
        self.session = Some(session);
        Ok(())
    }

    /// Execute commands from a buffered reader until EOF or `quit` is encountered.
    pub fn run_script<R: BufRead>(&mut self, reader: R) -> Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if self.execute(&line)?.eq(&CommandStatus::Quit) {
                break;
            }
        }
        Ok(())
    }

    /// Run an interactive REPL against stdin.
    pub fn repl(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut line = String::new();
        loop {
            write!(self.writer, "coh> ")?;
            self.writer.flush()?;
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                writeln!(self.writer)?;
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.execute(trimmed)? == CommandStatus::Quit {
                break;
            }
        }
        Ok(())
    }

    /// Execute a single command line.
    pub fn execute(&mut self, line: &str) -> Result<CommandStatus> {
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else {
            return Ok(CommandStatus::Continue);
        };
        match cmd {
            "help" => {
                writeln!(
                    self.writer,
                    "Available commands: help, tail <path>, attach <role>, login <role>, quit"
                )?;
                Ok(CommandStatus::Continue)
            }
            "tail" => {
                let Some(path) = parts.next() else {
                    return Err(anyhow!("tail requires a path"));
                };
                let session = self
                    .session
                    .as_ref()
                    .context("attach to a session before running tail")?;
                for line in self.transport.tail(session, path)? {
                    writeln!(self.writer, "{line}")?;
                }
                Ok(CommandStatus::Continue)
            }
            "attach" | "login" => {
                let Some(role_arg) = parts.next() else {
                    return Err(anyhow!("{cmd} requires a role"));
                };
                let role = parse_role(role_arg)?;
                let ticket = parts.next();
                self.attach(role, ticket)?;
                Ok(CommandStatus::Continue)
            }
            "quit" => {
                writeln!(self.writer, "closing session")?;
                Ok(CommandStatus::Quit)
            }
            unknown => Err(anyhow!("unknown command '{unknown}'")),
        }
    }

    /// Consume the shell and return owned transport and writer.
    pub fn into_parts(self) -> (T, W) {
        (self.transport, self.writer)
    }
}

fn parse_role(input: &str) -> Result<Role> {
    match input {
        "queen" => Ok(Role::Queen),
        "worker-heartbeat" => Ok(Role::WorkerHeartbeat),
        other => Err(anyhow!("unknown role '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn attach_and_tail_logs() {
        let transport = MockTransport::default();
        let buffer = Vec::new();
        let mut shell = Shell::new(transport, Cursor::new(buffer));
        shell.attach(Role::Queen, Some("ticket-1")).unwrap();
        shell.execute("tail /log/queen.log").unwrap();
        let (_transport, cursor) = shell.into_parts();
        let output = cursor.into_inner();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Cohesix boot: root-task online"));
        assert!(rendered.contains("tick 1"));
    }

    #[test]
    fn parse_role_rejects_unknown() {
        assert!(parse_role("other").is_err());
    }

    #[test]
    fn execute_quit_command() {
        let transport = MockTransport::default();
        let mut output = Vec::new();
        {
            let mut shell = Shell::new(transport, &mut output);
            shell.attach(Role::Queen, None).unwrap();
            assert_eq!(shell.execute("quit").unwrap(), CommandStatus::Quit);
        }
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("closing session"));
    }
}
