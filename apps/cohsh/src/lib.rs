// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix shell prototype speaking directly to the NineDoor Secure9P server.
//!
//! User-facing prompts, banners, and help text are rendered locally while
//! commands such as `attach` and `tail` are forwarded to the configured
//! transport. The client prefixes transport acknowledgements with `[console]`
//! so callers can distinguish remote answers from local UX noise.

pub mod proto;

#[cfg(feature = "tcp")]
pub mod transport;

#[cfg(feature = "tcp")]
pub use transport::tcp::{tcp_debug_enabled, TcpTransport};
#[cfg(feature = "tcp")]
pub use transport::COHSH_TCP_PORT;

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(feature = "tcp")]
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
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
    ///
    /// Worker roles must supply a non-empty identity string via `ticket` so the
    /// session can bind to the correct worker namespace.
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session>;

    /// Return a human-readable label describing the transport implementation.
    fn kind(&self) -> &'static str {
        "transport"
    }

    /// Perform a lightweight health probe using the underlying protocol.
    fn ping(&mut self, session: &Session) -> Result<String>;

    /// Stream a log-like file and return the accumulated contents.
    fn tail(&mut self, session: &Session, path: &str) -> Result<Vec<String>>;

    /// Append bytes to an append-only file within the NineDoor namespace.
    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()>;

    /// Drain acknowledgement lines accumulated since the previous call.
    fn drain_acknowledgements(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Return the TCP endpoint if the transport supports TCP diagnostics.
    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        None
    }
}

impl<T> Transport for Box<T>
where
    T: Transport + ?Sized,
{
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        (**self).attach(role, ticket)
    }

    fn kind(&self) -> &'static str {
        (**self).kind()
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        (**self).ping(session)
    }

    fn tail(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        (**self).tail(session, path)
    }

    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        (**self).write(session, path, payload)
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        (**self).drain_acknowledgements()
    }

    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        (**self).tcp_endpoint()
    }
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
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let mut connection = self
            .server
            .connect()
            .context("failed to open NineDoor session")?;
        connection
            .version(MAX_MSIZE)
            .context("version negotiation failed")?;
        let identity = match role {
            Role::Queen => None,
            Role::WorkerHeartbeat | Role::WorkerGpu => {
                let provided = ticket
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow!(
                            "role {:?} requires an identity provided via the ticket argument",
                            role
                        )
                    })?;
                Some(provided)
            }
        };
        let attach_result = match identity {
            Some(id) => connection.attach_with_identity(ROOT_FID, role, Some(id)),
            None => connection.attach(ROOT_FID, role),
        };
        attach_result.context("attach request failed")?;
        self.next_fid = ROOT_FID + 1;
        let session = Session::new(connection.session_id(), role);
        self.connection = Some(connection);
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "mock"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        let fid = self.allocate_fid();
        let connection = self
            .connection
            .as_mut()
            .context("attach to a session before running ping")?;
        connection
            .walk(ROOT_FID, fid, &[])
            .context("ping walk failed")?;
        connection.clunk(fid).context("ping clunk failed")?;
        Ok(format!("attached as {:?} via mock", session.role()))
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

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let components = parse_path(path)?;
        let fid = self.allocate_fid();
        let connection = self
            .connection
            .as_mut()
            .context("attach to a session before running write")?;
        connection
            .walk(ROOT_FID, fid, &components)
            .with_context(|| format!("failed to walk to {path}"))?;
        connection
            .open(fid, OpenMode::write_append())
            .with_context(|| format!("failed to open {path}"))?;
        let written = connection
            .write(fid, payload)
            .with_context(|| format!("failed to write {path}"))?;
        connection.clunk(fid).context("failed to clunk fid")?;
        if written as usize != payload.len() {
            return Err(anyhow!(
                "short write to {path}: expected {} bytes, wrote {written}",
                payload.len()
            ));
        }
        Ok(())
    }
}

/// QEMU-backed transport that boots the Cohesix image and streams serial logs.
#[derive(Debug)]
pub struct QemuTransport {
    qemu_bin: PathBuf,
    out_dir: PathBuf,
    extra_qemu_args: Vec<String>,
    gic_version: String,
    log_lines: Arc<Mutex<Vec<String>>>,
    child: Option<Child>,
    stdout_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    session_id: u64,
}

impl QemuTransport {
    /// Create a new QEMU transport using the supplied binary, artefact directory, and arguments.
    pub fn new(
        qemu_bin: impl Into<PathBuf>,
        out_dir: impl Into<PathBuf>,
        gic_version: impl Into<String>,
        extra_qemu_args: Vec<String>,
    ) -> Self {
        Self {
            qemu_bin: qemu_bin.into(),
            out_dir: out_dir.into(),
            extra_qemu_args,
            gic_version: gic_version.into(),
            log_lines: Arc::new(Mutex::new(Vec::new())),
            child: None,
            stdout_handle: None,
            stderr_handle: None,
            session_id: 1,
        }
    }

    fn spawn_reader<R>(stream: R, lines: Arc<Mutex<Vec<String>>>, store: bool) -> JoinHandle<()>
    where
        R: Read + Send + 'static,
    {
        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                if store {
                    let mut guard = lines.lock().expect("log mutex poisoned");
                    guard.push(line);
                }
            }
        })
    }

    fn artefacts(&self) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
        let staging = self.out_dir.join("staging");
        let elfloader = staging.join("elfloader");
        let kernel = staging.join("kernel.elf");
        let rootserver = staging.join("rootserver");
        let cpio = self.out_dir.join("cohesix-system.cpio");

        for (path, label) in [
            (&elfloader, "elfloader"),
            (&kernel, "kernel"),
            (&rootserver, "rootserver"),
            (&cpio, "payload cpio"),
        ] {
            if !path.is_file() {
                return Err(anyhow!(
                    "{label} artefact not found at {} (run scripts/cohesix-build-run.sh --no-run first)",
                    path.display()
                ));
            }
        }

        Ok((elfloader, kernel, rootserver, cpio))
    }

    fn wait_for_log(&self, timeout: Duration) -> Result<Vec<String>> {
        let start = Instant::now();
        let sentinel = "root task idling";
        loop {
            {
                let guard = self.log_lines.lock().expect("log mutex poisoned");
                if guard.iter().any(|line| line.contains(sentinel)) {
                    return Ok(guard.clone());
                }
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "timed out waiting for root-task output from QEMU (path: {:#?})",
                    self.out_dir
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn filter_root_log(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| {
                if let Some(stripped) = line.strip_prefix("[cohesix:root-task] ") {
                    Some(stripped.to_owned())
                } else if let Some((_, rest)) = line.split_once(']') {
                    let body = rest.trim_start();
                    if line.contains("[cohesix:root-task]") && !body.is_empty() {
                        Some(body.to_owned())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.stdout_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for QemuTransport {
    fn drop(&mut self) {
        self.stop_child();
    }
}

impl Transport for QemuTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        if self.child.is_some() {
            return Err(anyhow!("QEMU session already active"));
        }

        if !matches!(role, Role::Queen) {
            return Err(anyhow!(
                "QEMU transport currently only supports attaching as the queen role"
            ));
        }
        if let Some(ticket) = ticket {
            if !ticket.trim().is_empty() {
                return Err(anyhow!(
                    "tickets are not required when attaching via the QEMU transport"
                ));
            }
        }

        let (elfloader, kernel, rootserver, cpio) = self.artefacts()?;
        *self.log_lines.lock().expect("log mutex poisoned") = Vec::new();

        let mut cmd = Command::new(&self.qemu_bin);
        cmd.arg("-machine")
            .arg(format!("virt,gic-version={}", self.gic_version))
            .arg("-cpu")
            .arg("cortex-a57")
            .arg("-m")
            .arg("1024")
            .arg("-smp")
            .arg("1")
            .arg("-serial")
            .arg("mon:stdio")
            .arg("-display")
            .arg("none")
            .arg("-kernel")
            .arg(&elfloader)
            .arg("-initrd")
            .arg(&cpio)
            .arg("-device")
            .arg(format!(
                "loader,file={},addr=0x70000000,force-raw=on",
                kernel.display()
            ))
            .arg("-device")
            .arg(format!(
                "loader,file={},addr=0x80000000,force-raw=on",
                rootserver.display()
            ));

        for arg in &self.extra_qemu_args {
            cmd.arg(arg);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("failed to launch qemu-system-aarch64")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture QEMU stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture QEMU stderr")?;

        let stdout_lines = Arc::clone(&self.log_lines);
        self.stdout_handle = Some(Self::spawn_reader(stdout, stdout_lines, true));
        self.stderr_handle = Some(Self::spawn_reader(
            stderr,
            Arc::new(Mutex::new(Vec::new())),
            false,
        ));

        self.child = Some(child);
        let session = Session::new(SessionId::from_raw(self.session_id), role);
        self.session_id = self.session_id.wrapping_add(1);
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "qemu"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        if let Some(child) = self.child.as_mut() {
            if let Some(status) = child.try_wait().context("failed to query QEMU state")? {
                return Err(anyhow!("qemu exited with status {status}"));
            }
            return Ok(format!("ping: qemu running for {:?}", session.role()));
        }
        Err(anyhow!("attach via QEMU before issuing ping"))
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        if path != "/log/queen.log" {
            return Err(anyhow!(
                "QEMU transport currently supports tailing /log/queen.log only"
            ));
        }
        let raw_lines = self.wait_for_log(Duration::from_secs(15))?;
        let cleaned = Self::filter_root_log(&raw_lines);
        self.stop_child();
        Ok(cleaned)
    }

    fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> Result<()> {
        Err(anyhow!(
            "writes are not supported when using the QEMU transport"
        ))
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
        let label = match self {
            Self::Queen => ProtoRole::Queen,
            Self::WorkerHeartbeat => ProtoRole::Worker,
        };
        write!(f, "{}", proto_role_label(label))
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

    fn prompt(&self) -> String {
        "coh> ".to_owned()
    }

    /// Attach to the transport using the supplied role and optional ticket payload.
    /// Worker roles must provide their identity via `ticket`.
    pub fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<()> {
        if self.session.is_some() {
            return Err(anyhow!(
                "already attached; run 'quit' to close the current session"
            ));
        }
        let session = self.transport.attach(role, ticket)?;
        for ack in self.transport.drain_acknowledgements() {
            writeln!(self.writer, "[console] {ack}")?;
        }
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
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            if self.execute(&line)?.eq(&CommandStatus::Quit) {
                break;
            }
        }
        Ok(())
    }

    fn run_pending_attach(&mut self, pending: &mut Option<AutoAttach>) -> Result<()> {
        if let Some(auto) = pending.as_mut() {
            match self.attach(auto.role, auto.ticket.as_deref()) {
                Ok(()) => {
                    if auto.auto_log {
                        if let Err(err) = self.tail_path("/log/queen.log") {
                            writeln!(self.writer, "auto-log failed: {err}")?;
                        }
                    }
                    *pending = None;
                }
                Err(err) => {
                    auto.attempts = auto.attempts.saturating_add(1);
                    eprintln!(
                        "[cohsh] TCP attach failed (attempt {}): {}",
                        auto.attempts, err
                    );
                    if auto.attempts >= auto.max_attempts {
                        self.write_line("detached shell: run 'attach <role>' to connect")?;
                        *pending = None;
                    }
                }
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
            write!(self.writer, "{}", self.prompt())?;
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
            match self.execute(trimmed) {
                Ok(CommandStatus::Quit) => break,
                Ok(CommandStatus::Continue) => {}
                Err(err) => {
                    writeln!(self.writer, "Error: {err}")?;
                }
            }
        }
        Ok(())
    }

    /// Run an interactive REPL that performs a bounded auto-attach before
    /// accepting user commands.
    pub fn repl_with_autologin(&mut self, pending: Option<AutoAttach>) -> Result<()> {
        let mut pending_attach = pending;
        let (tx, rx) = mpsc::channel();
        let input_handle = thread::spawn(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(None);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Some(line)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut prompt_rendered = false;
        loop {
            self.run_pending_attach(&mut pending_attach)?;
            if !prompt_rendered {
                write!(self.writer, "{}", self.prompt())?;
                self.writer.flush()?;
                prompt_rendered = true;
            }

            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    prompt_rendered = false;
                    match self.execute(trimmed) {
                        Ok(CommandStatus::Quit) => break,
                        Ok(CommandStatus::Continue) => {}
                        Err(err) => {
                            writeln!(self.writer, "Error: {err}")?;
                        }
                    }
                }
                Ok(None) => {
                    writeln!(self.writer)?;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = input_handle.join();
        Ok(())
    }

    fn tail_path(&mut self, path: &str) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running tail")?;
        let lines = self.transport.tail(session, path)?;
        for ack in self.transport.drain_acknowledgements() {
            writeln!(self.writer, "[console] {ack}")?;
        }
        for line in lines {
            writeln!(self.writer, "{line}")?;
        }
        Ok(())
    }

    fn write_path(&mut self, path: &str, payload: &[u8]) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running echo")?;
        self.transport.write(session, path, payload)?;
        for ack in self.transport.drain_acknowledgements() {
            writeln!(self.writer, "[console] {ack}")?;
        }
        Ok(())
    }

    #[cfg(feature = "tcp")]
    fn run_tcp_diag(&mut self, port_override: Option<&str>) -> Result<()> {
        let endpoint = self
            .transport
            .tcp_endpoint()
            .unwrap_or_else(|| ("127.0.0.1".to_owned(), COHSH_TCP_PORT));
        let mut port = endpoint.1;
        if let Some(raw_port) = port_override {
            port = raw_port
                .parse::<u16>()
                .context("tcp-diag requires a numeric port")?;
        }
        let host = endpoint.0;
        writeln!(self.writer, "tcp-diag: connecting to {host}:{port}")?;
        match TcpStream::connect((host.as_str(), port)) {
            Ok(stream) => {
                writeln!(self.writer, "tcp-diag: connect succeeded")?;
                if let Ok(local) = stream.local_addr() {
                    writeln!(self.writer, "tcp-diag: local_addr={local}")?;
                }
                if let Ok(peer) = stream.peer_addr() {
                    writeln!(self.writer, "tcp-diag: peer_addr={peer}")?;
                }
            }
            Err(err) => {
                writeln!(self.writer, "tcp-diag: connect failed: {err}")?;
                return Err(anyhow!("tcp-diag failed: {err}"));
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
                self.write_line("Cohesix command surface:")?;
                self.write_line("  help                         - Show this help message")?;
                self.write_line("  attach <role> [ticket]       - Attach to a NineDoor session")?;
                self.write_line("  login <role> [ticket]        - Alias for attach")?;
                self.write_line("  tail <path>                  - Stream a file via NineDoor")?;
                self.write_line("  log                          - Tail /log/queen.log")?;
                self.write_line(
                    "  ping                         - Report attachment status for health checks",
                )?;
                #[cfg(feature = "tcp")]
                self.write_line(
                    "  tcp-diag [port]              - Debug TCP connectivity without protocol traffic",
                )?;
                self.write_line(
                    "  ls [path]                    - Enumerate directory entries (planned)",
                )?;
                self.write_line("  cat <path>                   - Read file contents (planned)")?;
                self.write_line(
                    "  echo <text> > <path>         - Append to a file (adds newline)",
                )?;
                self.write_line(
                    "  spawn <role> [opts]          - Queue worker spawn command (planned)",
                )?;
                self.write_line(
                    "  kill <worker_id>             - Queue worker termination (planned)",
                )?;
                self.write_line("  bind <src> <dst>             - Bind namespace path (planned)")?;
                self.write_line(
                    "  mount <service> <path>       - Mount service namespace (planned)",
                )?;
                self.write_line("  quit                         - Close the session and exit")?;
                Ok(CommandStatus::Continue)
            }
            "ls" | "cat" | "spawn" | "kill" | "bind" | "mount" => {
                self.write_line(&format!(
                    "Error: '{cmd}' is planned but not implemented yet in this build"
                ))?;
                Ok(CommandStatus::Continue)
            }
            "tail" => {
                let Some(path) = parts.next() else {
                    return Err(anyhow!("tail requires a path"));
                };
                self.tail_path(path)?;
                Ok(CommandStatus::Continue)
            }
            "log" => {
                self.tail_path("/log/queen.log")?;
                Ok(CommandStatus::Continue)
            }
            "ping" => {
                if parts.next().is_some() {
                    return Err(anyhow!("ping does not take any arguments"));
                }
                let Some(session) = self.session.as_ref() else {
                    writeln!(self.writer, "ping: not attached")?;
                    return Err(anyhow!("ping: not attached"));
                };
                let response = self.transport.ping(session)?;
                for ack in self.transport.drain_acknowledgements() {
                    writeln!(self.writer, "[console] {ack}")?;
                }
                writeln!(self.writer, "ping: {response}")?;
                Ok(CommandStatus::Continue)
            }
            "tcp-diag" => {
                #[cfg(feature = "tcp")]
                {
                    let port_arg = parts.next();
                    if parts.next().is_some() {
                        return Err(anyhow!("tcp-diag takes at most one argument: port"));
                    }
                    self.run_tcp_diag(port_arg)?;
                }
                #[cfg(not(feature = "tcp"))]
                {
                    return Err(anyhow!("tcp-diag is available only in TCP-enabled builds"));
                }
                Ok(CommandStatus::Continue)
            }
            "echo" => {
                let payload_start = line[4..].trim_start();
                let (raw_text, path_part) = payload_start
                    .split_once('>')
                    .ok_or_else(|| anyhow!("echo requires syntax: echo <text> > <path>"))?;
                let path = path_part.trim();
                if !path.starts_with('/') {
                    return Err(anyhow!("echo target must be an absolute path"));
                }
                let payload = normalise_echo_payload(raw_text);
                self.write_path(path, payload.as_bytes())?;
                Ok(CommandStatus::Continue)
            }
            "attach" | "login" => {
                let args: Vec<&str> = parts.collect();
                let (role, ticket) = parse_attach_args(cmd, &args)?;
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

/// Bounded auto-attach configuration used when starting the interactive shell.
#[derive(Debug, Clone)]
pub struct AutoAttach {
    /// Target role for the initial session.
    pub role: Role,
    /// Optional ticket payload to accompany the role selection.
    pub ticket: Option<String>,
    /// Number of attempts already performed.
    pub attempts: usize,
    /// Maximum attach attempts before deferring to user input.
    pub max_attempts: usize,
    /// Automatically start tailing the queen log after attaching.
    pub auto_log: bool,
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
        if component.as_bytes().contains(&0) {
            return Err(anyhow!("path component contains NUL byte"));
        }
        components.push(component.to_owned());
    }
    Ok(components)
}

fn parse_role(input: &str) -> Result<Role> {
    if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Queen)) {
        Ok(Role::Queen)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Worker)) {
        Ok(Role::WorkerHeartbeat)
    } else {
        Err(anyhow!("unknown role '{input}'"))
    }
}

fn parse_attach_args<'a>(cmd: &str, args: &'a [&'a str]) -> Result<(Role, Option<&'a str>)> {
    match args {
        [] => Err(anyhow!("{cmd} requires a role")),
        [role] => Ok((parse_role(role)?, None)),
        [role, ticket] => Ok((parse_role(role)?, Some(*ticket))),
        _ => Err(anyhow!(
            "{cmd} takes at most two arguments: role and optional ticket"
        )),
    }
}
fn normalise_echo_payload(input: &str) -> String {
    let trimmed = input.trim();
    let content = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let mut payload = content.to_owned();
    if !payload.ends_with('\n') {
        payload.push('\n');
    }
    payload
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

    #[test]
    fn worker_attach_requires_identity() {
        let mut transport = NineDoorTransport::new(NineDoor::new());
        let err = transport
            .attach(Role::WorkerHeartbeat, None)
            .expect_err("worker attach without identity should fail");
        assert!(err
            .to_string()
            .contains("requires an identity provided via the ticket argument"));
    }

    #[test]
    fn normalise_echo_payload_appends_newline() {
        assert_eq!(normalise_echo_payload("'trace'"), "trace\n");
        assert_eq!(normalise_echo_payload("plain"), "plain\n");
    }

    #[test]
    fn help_command_lists_surface() {
        let transport = NineDoorTransport::new(NineDoor::new());
        let mut output = Vec::new();
        {
            let mut shell = Shell::new(transport, &mut output);
            shell.execute("help").unwrap();
        }
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Cohesix command surface:"));
        assert!(rendered.contains("tail <path>"));
        assert!(rendered.contains("ls [path]"));
        assert!(rendered.contains("mount <service> <path>"));
    }
}
