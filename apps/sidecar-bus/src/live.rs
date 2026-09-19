// Author: Lukas Bower
// Purpose: Durably retain mapped field-bus attempts and matching remote acknowledgements before reporting delivery, with ambiguous controls never replayed.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![allow(missing_docs)]

use cohesix_authority::bus::{Endpoint, Point};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = core::result::Result<T, BusError>;
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("not_enabled field_bus_endpoint")]
    NotEnabled,
    #[error("EPERM field_bus_policy")]
    Policy,
    #[error("ELIMIT field_bus_bound")]
    Limit,
    #[error("EBUSY field_bus_owner")]
    Busy,
    #[error("rate_limited field_bus_poll_interval")]
    PollInterval,
    #[error("timeout field_bus")]
    Timeout,
    #[error("disconnected field_bus")]
    Disconnected,
    #[error("unavailable field_bus_transport")]
    Transport,
    #[error("invalid_frame field_bus")]
    Frame,
    #[error("wrong_response field_bus")]
    Correlation,
    #[error("crc_mismatch field_bus")]
    Crc,
    #[error("not_supported field_bus_protocol_feature")]
    Unsupported,
    #[error("modbus_exception code={0}")]
    ModbusException(u8),
    #[error("dnp3_iin status={0}")]
    Dnp3Iin(u16),
    #[error("dnp3_control status={0}")]
    Dnp3Control(u8),
    #[error("unavailable field_bus_point_quality")]
    Quality,
    #[error("invalid_state field_bus_wal")]
    Wal,
}
impl BusError {
    pub fn io(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => Self::Timeout,
            std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted => Self::Disconnected,
            _ => Self::Transport,
        }
    }
}

const MAX_BYTES: usize = 1_048_576;
const MAX_ENTRIES: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub id: String,
    pub idempotency_key: String,
    pub endpoint: String,
    pub point: String,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "cohesix-field-bus-request/v1"
            || [&self.id, &self.idempotency_key, &self.endpoint, &self.point]
                .iter()
                .any(|value| cohesix_authority::validate_id(value).is_err() || value.len() > 64)
        {
            return Err(BusError::Policy);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Prepared,
    Attempting,
    Delivered,
    Failed,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub sequence: u64,
    pub request: Request,
    pub operation_sha256: String,
    pub provider_graph_sha256: String,
    pub control: bool,
    pub state: State,
    pub attempts: u8,
    pub created_unix_ms: u64,
    pub observed_unix_ms: Option<u64>,
    pub error: Option<String>,
    pub requests_hex: Vec<String>,
    pub acknowledgements_hex: Vec<String>,
    pub values: Vec<crate::protocol::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema: String,
    entries_sha256: String,
    entries: Vec<Entry>,
}

/// This lock and journal have one host owner. No draining or terminal eviction occurs implicitly.
pub struct Spool {
    root: PathBuf,
    _lock: File,
    ledger: Ledger,
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| BusError::Wal)
}
fn now() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| BusError::Wal)?
            .as_millis(),
    )
    .map_err(|_| BusError::Wal)
}

fn private(path: &Path, directory: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| BusError::Wal)?;
    if meta.file_type().is_symlink()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.permissions().mode() & 0o077 != 0
        || (directory && !meta.is_dir())
        || (!directory && (!meta.is_file() || meta.nlink() != 1))
    {
        return Err(BusError::Wal);
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    private(path, false)?;
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| BusError::Wal)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| BusError::Wal)?;
    if raw.len() > MAX_BYTES {
        return Err(BusError::Limit);
    }
    Ok(raw)
}

impl Spool {
    pub fn open(path: &Path) -> Result<Self> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(path)
                    .map_err(|_| BusError::Wal)?;
            }
            Err(_) => return Err(BusError::Wal),
            Ok(_) => (),
        }
        private(path, true)?;
        let owner = path.join("owner");
        let fresh = match OpenOptions::new()
            .write(true)
            .read(true)
            .mode(0o600)
            .create_new(true)
            .open(&owner)
        {
            Ok(file) => {
                file.sync_all().map_err(|_| BusError::Wal)?;
                Some(file)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(_) => return Err(BusError::Wal),
        };
        let is_fresh = fresh.is_some();
        private(&owner, false)?;
        let lock = match fresh {
            Some(file) => file,
            None => OpenOptions::new()
                .read(true)
                .write(true)
                .open(&owner)
                .map_err(|_| BusError::Wal)?,
        };
        lock.try_lock_exclusive().map_err(|_| BusError::Busy)?;
        let ledger_path = path.join("ledger.json");
        let ledger = if is_fresh {
            if ledger_path.exists() {
                return Err(BusError::Wal);
            }
            Ledger {
                schema: "cohesix-field-bus-wal/v1".into(),
                entries_sha256: hash(b"[]"),
                entries: Vec::new(),
            }
        } else {
            let raw = read(&ledger_path)?;
            serde_json::from_slice::<Ledger>(&raw).map_err(|_| BusError::Wal)?
        };
        if ledger.entries_sha256 != hash(&bytes(&ledger.entries)?)
            || ledger.schema != "cohesix-field-bus-wal/v1"
            || ledger.entries.len() > MAX_ENTRIES
        {
            return Err(BusError::Wal);
        }
        let mut keys = std::collections::BTreeSet::new();
        for (index, entry) in ledger.entries.iter().enumerate() {
            entry.request.validate()?;
            if entry.sequence != index as u64 + 1
                || !keys.insert((&entry.request.id, &entry.request.idempotency_key))
                || entry.attempts > 3
                || entry.created_unix_ms == 0
                || entry.requests_hex.len() > 4
                || entry.acknowledgements_hex.len() > 2
                || entry.values.len() > 256
                || [&entry.operation_sha256, &entry.provider_graph_sha256]
                    .iter()
                    .any(|value| {
                        value.len() != 64
                            || !value
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                || entry.error.as_ref().is_some_and(|value| value.len() > 128)
                || entry
                    .requests_hex
                    .iter()
                    .chain(&entry.acknowledgements_hex)
                    .any(|wire| {
                        wire.len() > 584
                            || wire.len() % 2 != 0
                            || !wire
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
                || (entry.state == State::Delivered
                    && (entry.observed_unix_ms.is_none() || entry.acknowledgements_hex.is_empty()))
            {
                return Err(BusError::Wal);
            }
        }
        let mut spool = Self {
            root: path.to_owned(),
            _lock: lock,
            ledger,
        };
        if is_fresh {
            spool.save()?;
        }
        Ok(spool)
    }

    fn save(&mut self) -> Result<()> {
        self.ledger.entries_sha256 = hash(&bytes(&self.ledger.entries)?);
        let raw = bytes(&self.ledger)?;
        if raw.len() > MAX_BYTES {
            return Err(BusError::Limit);
        }
        let temporary = self.root.join("ledger.next");
        if temporary.exists() {
            private(&temporary, false)?;
            fs::remove_file(&temporary).map_err(|_| BusError::Wal)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| BusError::Wal)?;
        file.write_all(&raw).map_err(|_| BusError::Wal)?;
        file.sync_all().map_err(|_| BusError::Wal)?;
        fs::rename(&temporary, self.root.join("ledger.json")).map_err(|_| BusError::Wal)?;
        File::open(&self.root)
            .map_err(|_| BusError::Wal)?
            .sync_all()
            .map_err(|_| BusError::Wal)?;
        Ok(())
    }

    pub fn entries(&self) -> &[Entry] {
        &self.ledger.entries
    }

    /// Reconcile retained state without protocol I/O, rechecking the current graph and map.
    pub fn retained(&self, request: &Request) -> Result<Option<Entry>> {
        request.validate()?;
        let (endpoint, point) = cohesix_authority::bus::resolve(&request.endpoint, &request.point)
            .map_err(|_| BusError::NotEnabled)?;
        let fingerprint = hash(&bytes(&(&endpoint, &point))?);
        let registry = cohesix_authority::provider::registry().map_err(|_| BusError::Policy)?;
        let row = self.ledger.entries.iter().find(|row| {
            row.request.id == request.id && row.request.idempotency_key == request.idempotency_key
        });
        if let Some(row) = row {
            if row.request != *request
                || row.operation_sha256 != fingerprint
                || registry["graph_sha256"] != row.provider_graph_sha256
            {
                return Err(BusError::Correlation);
            }
        }
        Ok(row.cloned())
    }

    /// Read-only public dispatch. Controls require a separately verified admitted call.
    pub fn read(&mut self, request: &Request) -> Result<Entry> {
        let endpoints = cohesix_authority::bus::endpoints().map_err(|_| BusError::Policy)?;
        let endpoint = endpoints
            .iter()
            .find(|value| value.id == request.endpoint)
            .ok_or(BusError::NotEnabled)?;
        let point = endpoint
            .points
            .iter()
            .find(|value| value.id == request.point)
            .ok_or(BusError::NotEnabled)?;
        if point.operation.is_control() {
            return Err(BusError::Policy);
        }
        self.execute(request, endpoint, point)
    }

    /// Require a separately enrolled signed grant for the exact control map.
    /// No CLI boolean, arbitrary payload or queued request grants a write.
    pub fn control(
        &mut self,
        request: &Request,
        admitted: &serde_json::Value,
        authority: &cohesix_evidence::producer::Operation,
    ) -> Result<Entry> {
        request.validate()?;
        let endpoints = cohesix_authority::bus::endpoints().map_err(|_| BusError::Policy)?;
        let endpoint = endpoints
            .iter()
            .find(|value| value.id == request.endpoint)
            .ok_or(BusError::NotEnabled)?;
        let point = endpoint
            .points
            .iter()
            .find(|value| value.id == request.point)
            .ok_or(BusError::NotEnabled)?;
        require_control(request, admitted, authority, endpoint, point, now()?)?;
        self.execute(request, endpoint, point)
    }

    fn execute(&mut self, request: &Request, endpoint: &Endpoint, point: &Point) -> Result<Entry> {
        request.validate()?;
        endpoint.validate().map_err(|_| BusError::Policy)?;
        let fingerprint = hash(&bytes(&(endpoint, point))?);
        let registry = cohesix_authority::provider::registry().map_err(|_| BusError::Policy)?;
        let graph = registry["graph_sha256"].as_str().ok_or(BusError::Policy)?;
        let index = match self.ledger.entries.iter().position(|row| {
            row.request.id == request.id && row.request.idempotency_key == request.idempotency_key
        }) {
            Some(index) => {
                let row = &self.ledger.entries[index];
                if row.request != *request
                    || row.operation_sha256 != fingerprint
                    || row.provider_graph_sha256 != graph
                {
                    return Err(BusError::Correlation);
                }
                index
            }
            None => {
                if self.ledger.entries.len() >= MAX_ENTRIES {
                    return Err(BusError::Limit);
                }
                let created = now()?;
                if !point.operation.is_control() {
                    if let Some(previous) = self.ledger.entries.iter().rev().find(|row| {
                        row.request.endpoint == request.endpoint
                            && row.request.point == request.point
                    }) {
                        require_poll_interval(
                            previous.created_unix_ms,
                            created,
                            endpoint.poll_interval_ms,
                        )?;
                    }
                }
                let index = self.ledger.entries.len();
                self.ledger.entries.push(Entry {
                    sequence: index as u64 + 1,
                    request: request.clone(),
                    operation_sha256: fingerprint,
                    provider_graph_sha256: graph.into(),
                    control: point.operation.is_control(),
                    state: State::Prepared,
                    attempts: 0,
                    created_unix_ms: created,
                    observed_unix_ms: None,
                    error: None,
                    requests_hex: Vec::new(),
                    acknowledgements_hex: Vec::new(),
                    values: Vec::new(),
                });
                self.save()?;
                index
            }
        };
        let row = &mut self.ledger.entries[index];
        if matches!(
            row.state,
            State::Delivered | State::Failed | State::Ambiguous
        ) {
            return Ok(row.clone());
        }
        if row.state == State::Attempting && row.control {
            row.state = State::Ambiguous;
            row.error = Some("control_ack_unconfirmed_no_replay".into());
            self.save()?;
            return Ok(self.ledger.entries[index].clone());
        }
        let time = now()?;
        if time < row.created_unix_ms || time - row.created_unix_ms >= 60_000 || row.attempts >= 3 {
            row.state = State::Failed;
            row.error = Some("read_retry_or_lifetime_bound".into());
            self.save()?;
            return Ok(self.ledger.entries[index].clone());
        }
        row.state = State::Attempting;
        row.attempts += 1;
        let sequence = row.sequence as u16;
        self.save()?; // All intent and attempt bytes reach stable storage before any wire I/O.
        let outcome = crate::transport::exchange(endpoint, &point.operation, sequence);
        let row = &mut self.ledger.entries[index];
        match outcome {
            Ok(exchange) => {
                row.requests_hex = exchange.requests.iter().map(hex::encode).collect();
                row.acknowledgements_hex = exchange.responses.iter().map(hex::encode).collect();
                row.values = exchange.values;
                row.state = State::Delivered;
                row.observed_unix_ms = Some(now()?);
                row.error = None;
            }
            Err(error) => {
                row.error = Some(error.to_string());
                row.state = if row.control {
                    State::Ambiguous
                } else if matches!(
                    error,
                    BusError::Timeout | BusError::Disconnected | BusError::Transport
                ) && row.attempts < 3
                {
                    State::Prepared
                } else {
                    State::Failed
                };
            }
        }
        self.save()?;
        Ok(self.ledger.entries[index].clone())
    }
}

/// A MODBUS client over one generated endpoint; it is not the in-memory adapter model.
pub struct ModbusAdapter {
    endpoint: String,
    spool: Spool,
}
impl ModbusAdapter {
    pub fn open(endpoint: &str, state: &Path) -> Result<Self> {
        require_protocol(endpoint, cohesix_authority::bus::Protocol::Modbus)?;
        Ok(Self {
            endpoint: endpoint.into(),
            spool: Spool::open(state)?,
        })
    }
    pub fn read(&mut self, request: &Request) -> Result<Entry> {
        if request.endpoint != self.endpoint {
            return Err(BusError::Policy);
        }
        self.spool.read(request)
    }
}

/// A DNP3 client over one generated endpoint; unsupported object/transport features refuse.
pub struct Dnp3Adapter {
    endpoint: String,
    spool: Spool,
}
impl Dnp3Adapter {
    pub fn open(endpoint: &str, state: &Path) -> Result<Self> {
        require_protocol(endpoint, cohesix_authority::bus::Protocol::Dnp3)?;
        Ok(Self {
            endpoint: endpoint.into(),
            spool: Spool::open(state)?,
        })
    }
    pub fn read(&mut self, request: &Request) -> Result<Entry> {
        if request.endpoint != self.endpoint {
            return Err(BusError::Policy);
        }
        self.spool.read(request)
    }
}
fn require_protocol(id: &str, protocol: cohesix_authority::bus::Protocol) -> Result<()> {
    let rows = cohesix_authority::bus::endpoints().map_err(|_| BusError::Policy)?;
    if !rows
        .iter()
        .any(|endpoint| endpoint.id == id && endpoint.protocol == protocol)
    {
        return Err(BusError::NotEnabled);
    }
    Ok(())
}

fn require_control(
    request: &Request,
    admitted: &serde_json::Value,
    authority: &cohesix_evidence::producer::Operation,
    endpoint: &Endpoint,
    point: &Point,
    now: u64,
) -> Result<()> {
    use cohesix_evidence::{Kind, Outcome};
    let family = match endpoint.protocol {
        cohesix_authority::bus::Protocol::Modbus => "modbus",
        cohesix_authority::bus::Protocol::Dnp3 => "dnp3",
    };
    let action = format!("{family}.control");
    let expected = &authority.trust.expected;
    let registry = cohesix_authority::provider::registry().map_err(|_| BusError::Policy)?;
    if !point.approval_required
        || !point.operation.is_control()
        || expected.ticket_id != request.id
        || expected.idempotency_key != request.idempotency_key
        || expected.action != action
        || expected.worker.is_some()
        || registry["graph_sha256"] != expected.provider_graph_sha256
        || !authority
            .record(Kind::Grant)
            .is_some_and(|grant| grant.outcome == Outcome::Admitted)
        || authority.expires_unix_ms().saturating_sub(now) <= u64::from(endpoint.timeout_ms) + 1000
        || admitted["schema"] != "host-ticket/v1"
        || admitted["id"] != request.id
        || admitted["idempotency_key"] != request.idempotency_key
        || admitted["action"] != action
        || admitted["writer_epoch"] != expected.writer_epoch
        || admitted["target"] != endpoint.id
        || admitted["args"] != serde_json::json!({"endpoint":endpoint.id,"point":point.id})
    {
        return Err(BusError::Policy);
    }
    authority
        .require_json(Kind::Intent, admitted, false)
        .map_err(|_| BusError::Policy)
}

fn require_poll_interval(previous: u64, current: u64, interval: u32) -> Result<()> {
    if current
        .checked_sub(previous)
        .is_none_or(|elapsed| elapsed < u64::from(interval))
    {
        return Err(BusError::PollInterval);
    }
    Ok(())
}

#[cfg(test)]
#[path = "authorization_tests.rs"]
mod authorization_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::bus::{Operation, Protocol, Transport};
    use std::{net::TcpListener, thread};

    #[test]
    fn new_reads_respect_poll_interval_without_clock_regression() {
        assert!(matches!(
            require_poll_interval(1000, 1999, 1000),
            Err(BusError::PollInterval)
        ));
        assert!(matches!(
            require_poll_interval(1000, 999, 1000),
            Err(BusError::PollInterval)
        ));
        assert!(require_poll_interval(1000, 2000, 1000).is_ok());
    }

    pub(super) fn selected(address: String, control: bool) -> (Endpoint, Request) {
        let point = Point {
            id: "register".into(),
            approval_required: control,
            operation: if control {
                Operation::ModbusWrite {
                    function: 6,
                    address: 0,
                    value: 42,
                }
            } else {
                Operation::ModbusRead {
                    function: 3,
                    start: 0,
                    count: 1,
                }
            },
        };
        (
            Endpoint {
                id: "plc".into(),
                protocol: Protocol::Modbus,
                transport: Transport::Tcp { address },
                unit: 1,
                master: 0,
                outstation: 0,
                timeout_ms: 1000,
                poll_interval_ms: 1000,
                observation_ttl_ms: 2000,
                points: vec![point],
            },
            Request {
                schema: "cohesix-field-bus-request/v1".into(),
                id: "one".into(),
                idempotency_key: "once".into(),
                endpoint: "plc".into(),
                point: "register".into(),
            },
        )
    }

    #[test]
    fn acknowledged_read_recovery_retains_exact_terminal_and_attempt_wal() {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (endpoint, request) = selected(listener.local_addr().unwrap().to_string(), false);
        let spool_path = root.path().join("state");
        let server_state = spool_path.clone();
        let server = thread::spawn(move || {
            for attempt in 1..=2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 12];
                stream.read_exact(&mut request).unwrap();
                assert_eq!(request, [0, 1, 0, 0, 0, 6, 1, 3, 0, 0, 0, 1]);
                let durable: Ledger =
                    serde_json::from_slice(&fs::read(server_state.join("ledger.json")).unwrap())
                        .unwrap();
                assert_eq!(durable.entries[0].state, State::Attempting);
                assert_eq!(durable.entries[0].attempts, attempt);
                if attempt == 2 {
                    stream
                        .write_all(&[0, 1, 0, 0, 0, 5, 1, 3, 2, 0, 42])
                        .unwrap();
                }
            }
        });
        let mut spool = Spool::open(&spool_path).unwrap();
        assert!(matches!(Spool::open(&spool_path), Err(BusError::Busy)));
        let first = spool
            .execute(&request, &endpoint, &endpoint.points[0])
            .unwrap();
        assert_eq!(first.state, State::Prepared);
        assert_eq!(first.attempts, 1);
        drop(spool);
        let mut spool = Spool::open(&spool_path).unwrap();
        let second = spool
            .execute(&request, &endpoint, &endpoint.points[0])
            .unwrap();
        assert_eq!(second.state, State::Delivered);
        assert_eq!(second.attempts, 2);
        assert_eq!(second.values[0].value, 42);
        server.join().unwrap();
        drop(spool);
        let before = fs::read(spool_path.join("ledger.json")).unwrap();
        let mut spool = Spool::open(&spool_path).unwrap();
        assert_eq!(
            spool
                .execute(&request, &endpoint, &endpoint.points[0])
                .unwrap()
                .state,
            State::Delivered
        );
        assert_eq!(before, fs::read(spool_path.join("ledger.json")).unwrap());
        let mut changed = endpoint.clone();
        changed.points[0].operation = Operation::ModbusRead {
            function: 3,
            start: 1,
            count: 1,
        };
        assert!(matches!(
            spool.execute(&request, &changed, &changed.points[0]),
            Err(BusError::Correlation)
        ));
    }

    #[test]
    fn unacknowledged_control_is_ambiguous_and_never_replayed_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (endpoint, request) = selected(listener.local_addr().unwrap().to_string(), true);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 12];
            stream.read_exact(&mut bytes).unwrap();
            assert_eq!(bytes, [0, 1, 0, 0, 0, 6, 1, 6, 0, 0, 0, 42]);
            // Simulate a remote write followed by loss of the protocol response.
        });
        let path = root.path().join("state");
        let mut spool = Spool::open(&path).unwrap();
        assert_eq!(
            spool
                .execute(&request, &endpoint, &endpoint.points[0])
                .unwrap()
                .state,
            State::Ambiguous
        );
        server.join().unwrap();
        drop(spool);
        let mut spool = Spool::open(&path).unwrap();
        let entry = spool
            .execute(&request, &endpoint, &endpoint.points[0])
            .unwrap();
        assert_eq!(entry.state, State::Ambiguous);
        assert_eq!(entry.attempts, 1);
        // The prepared/attempting transition itself is also a crash boundary.
        spool.ledger.entries[0].state = State::Attempting;
        spool.save().unwrap();
        drop(spool);
        let mut spool = Spool::open(&path).unwrap();
        let entry = spool
            .execute(&request, &endpoint, &endpoint.points[0])
            .unwrap();
        assert_eq!(entry.state, State::Ambiguous);
        assert_eq!(entry.attempts, 1);
    }

    #[test]
    fn missing_or_corrupt_owned_wal_is_never_reinitialized() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state");
        drop(Spool::open(&path).unwrap());
        let ledger = path.join("ledger.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        value["entries_sha256"] = serde_json::json!("0".repeat(64));
        fs::write(&ledger, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(matches!(Spool::open(&path), Err(BusError::Wal)));
        fs::remove_file(ledger).unwrap();
        assert!(matches!(Spool::open(&path), Err(BusError::Wal)));
    }
}
