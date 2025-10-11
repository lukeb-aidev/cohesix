// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix shell prototype speaking directly to the NineDoor Secure9P server.
//! Milestone 2 replaces the mock transport with the live codec and synthetic
//! namespace so operators can tail logs using the real filesystem protocol.

use std::fmt;
use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use cohesix_ticket::Role;
use nine_door::{InProcessConnection, NineDoor};
use secure9p_wire::{OpenMode, SessionId, MAX_MSIZE};

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

const ROOT_FID: u32 = 1;

/// Transport abstraction used by the shell to interact with the system.
pub trait Transport {
    /// Attach to the transport using the specified role and optional ticket payload.
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session>;

    /// Stream a log-like file and return the accumulated contents.
    fn tail(&mut self, session: &Session, path: &str) -> Result<Vec<String>>;
}

/// Live transport backed by the in-process NineDoor Secure9P server.
#[derive(Debug)]
pub struct NineDoorTransport {
    server: NineDoor,
    connection: Option<InProcessConnection>,
    next_fid: u32,
}

impl NineDoorTransport {
    /// Create a new transport bound to the supplied server instance.
    pub fn new(server: NineDoor) -> Self {
        Self {
            server,
            connection: None,
            next_fid: ROOT_FID,
        }
    }

    fn allocate_fid(&mut self) -> u32 {
        let fid = self.next_fid;
        self.next_fid = self.next_fid.wrapping_add(1);
        fid
    }
}

impl Transport for NineDoorTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<Session> {
        let mut connection = self
            .server
            .connect()
            .context("failed to open NineDoor session")?;
        connection
            .version(MAX_MSIZE)
            .context("version negotiation failed")?;
        connection
            .attach(ROOT_FID, role)
            .context("attach request failed")?;
        self.next_fid = ROOT_FID + 1;
        let session = Session::new(connection.session_id(), role);
        self.connection = Some(connection);
        Ok(session)
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        let components = parse_path(path)?;
        let fid = self.allocate_fid();
        let connection = self
            .connection
            .as_mut()
            .context("attach to a session before running tail")?;
        connection
            .walk(ROOT_FID, fid, &components)
            .with_context(|| format!("failed to walk to {path}"))?;
        connection
            .open(fid, OpenMode::read_only())
            .with_context(|| format!("failed to open {path}"))?;
        let mut offset = 0u64;
        let mut buffer = Vec::new();
        loop {
            let chunk = connection
                .read(fid, offset, connection.negotiated_msize())
                .with_context(|| format!("failed to read {path}"))?;
            if chunk.is_empty() {
                break;
            }
            offset = offset
                .checked_add(chunk.len() as u64)
                .context("offset overflow during read")?;
            buffer.extend_from_slice(&chunk);
            if chunk.len() < connection.negotiated_msize() as usize {
                break;
            }
        }
        connection.clunk(fid).context("failed to clunk fid")?;
        let text = String::from_utf8(buffer).context("log is not valid UTF-8")?;
        Ok(text.lines().map(|line| line.to_owned()).collect())
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

fn parse_path(path: &str) -> Result<Vec<String>> {
    if !path.starts_with('/') {
        return Err(anyhow!("paths must be absolute"));
    }
    let mut components = Vec::new();
    for component in path.split('/').skip(1) {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." {
            return Err(anyhow!("path component '{component}' is not permitted"));
        }
        if component.as_bytes().iter().any(|&b| b == 0) {
            return Err(anyhow!("path component contains NUL byte"));
        }
        components.push(component.to_owned());
    }
    Ok(components)
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
        let transport = NineDoorTransport::new(NineDoor::new());
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
        let transport = NineDoorTransport::new(NineDoor::new());
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
