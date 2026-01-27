// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate session pooling throughput and retry idempotency.
// Author: Lukas Bower

use std::fs;
#[cfg(feature = "tcp")]
use std::io::{BufReader, Read, Write};
#[cfg(feature = "tcp")]
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(feature = "tcp")]
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cohesix_ticket::Role;
use cohsh::{PoolKind, Session, SessionPool, Transport, TransportMetrics};
use secure9p_codec::SessionId;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    secure9p: Secure9pLimits,
}

#[derive(Debug, Deserialize)]
struct Secure9pLimits {
    msize: u32,
    tags_per_session: u16,
}

fn load_secure9p_limits() -> Result<Secure9pLimits> {
    let path = format!(
        "{}/../../configs/root_task.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let contents = fs::read_to_string(path)?;
    let manifest: Manifest = toml::from_str(&contents)?;
    Ok(manifest.secure9p)
}

#[derive(Debug)]
struct SleepyTransport {
    delay: Duration,
    writes: Arc<AtomicUsize>,
}

impl SleepyTransport {
    fn new(delay: Duration, writes: Arc<AtomicUsize>) -> Self {
        Self { delay, writes }
    }
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Transport for SleepyTransport {
    fn attach(&mut self, role: Role, _ticket: Option<&str>) -> Result<Session> {
        let id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
        Ok(Session::new(SessionId::from_raw(id), role))
    }

    fn kind(&self) -> &'static str {
        "sleepy"
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        Ok("pong".to_owned())
    }

    fn tail(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        Err(anyhow!("sleepy transport does not support tail"))
    }

    fn read(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        Err(anyhow!("sleepy transport does not support read"))
    }

    fn list(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        Err(anyhow!("sleepy transport does not support list"))
    }

    fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> Result<()> {
        thread::sleep(self.delay);
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn metrics(&self) -> TransportMetrics {
        TransportMetrics::default()
    }
}

fn ops_per_s(ops: usize, elapsed: Duration) -> u64 {
    let elapsed_ms = elapsed.as_millis() as u64;
    if elapsed_ms == 0 {
        0
    } else {
        (ops as u64).saturating_mul(1000) / elapsed_ms
    }
}

#[test]
fn pooled_throughput_exceeds_baseline() {
    let limits = load_secure9p_limits().expect("secure9p limits");
    let delay = Duration::from_millis(8);
    let ops = 40usize;
    let payload = b"pool-bench";
    let path = "/log/queen.log";

    assert!(payload.len() < limits.msize as usize);
    let pool_size = 4usize.min(limits.tags_per_session as usize);

    let baseline_writes = Arc::new(AtomicUsize::new(0));
    let mut baseline_transport = SleepyTransport::new(delay, baseline_writes.clone());
    let session = baseline_transport
        .attach(Role::Queen, None)
        .expect("attach baseline session");

    let baseline_start = Instant::now();
    for _ in 0..ops {
        baseline_transport
            .write(&session, path, payload)
            .expect("baseline write");
    }
    let baseline_elapsed = baseline_start.elapsed();

    let pooled_writes = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new({
        let writes = pooled_writes.clone();
        move || {
            Ok(Box::new(SleepyTransport::new(delay, writes.clone())) as Box<dyn Transport + Send>)
        }
    });
    let pool = SessionPool::new(pool_size as u16, pool_size as u16, factory);
    pool.attach(Role::Queen, None).expect("attach pool session");

    let shared_ops = Arc::new(AtomicUsize::new(0));
    let pooled_successes = Arc::new(AtomicUsize::new(0));
    let pooled_start = Instant::now();
    let mut handles = Vec::new();

    for _ in 0..pool_size {
        let pool = pool.clone();
        let shared_ops = Arc::clone(&shared_ops);
        let pooled_successes = Arc::clone(&pooled_successes);
        handles.push(thread::spawn(move || {
            let mut lease = pool.checkout(PoolKind::Telemetry).expect("pool checkout");
            loop {
                let index = shared_ops.fetch_add(1, Ordering::SeqCst);
                if index >= ops {
                    break;
                }
                let session = lease.session().clone();
                lease
                    .transport_mut()
                    .write(&session, path, payload)
                    .expect("pooled write");
                pooled_successes.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for handle in handles {
        handle.join().expect("join worker");
    }
    let pooled_elapsed = pooled_start.elapsed();
    let pooled_ops = pooled_successes.load(Ordering::SeqCst);

    let baseline_ops_per_s = ops_per_s(ops, baseline_elapsed);
    let pooled_ops_per_s = ops_per_s(pooled_ops, pooled_elapsed);

    println!(
        "pooling benchmark baseline_ms={} pooled_ms={} baseline_ops_s={} pooled_ops_s={} msize={} tags_per_session={}",
        baseline_elapsed.as_millis(),
        pooled_elapsed.as_millis(),
        baseline_ops_per_s,
        pooled_ops_per_s,
        limits.msize,
        limits.tags_per_session
    );

    assert_eq!(pooled_ops, ops);
    assert!(pooled_ops_per_s > baseline_ops_per_s);
}

#[cfg(feature = "tcp")]
fn write_frame(stream: &mut std::net::TcpStream, line: &str) {
    let total_len = line.len().saturating_add(4) as u32;
    stream.write_all(&total_len.to_le_bytes()).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
}

#[cfg(feature = "tcp")]
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

#[cfg(feature = "tcp")]
#[test]
fn tcp_short_write_retry_is_idempotent() {
    use cohsh::{CohshRetryPolicy, TcpTransport};

    let received = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let received_clone = Arc::clone(&received);

    let handle = thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let mut stream = stream.expect("accept stream");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == "AUTH changeme" {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed.starts_with("ECHO") {
                    let mut parts = trimmed.splitn(3, ' ');
                    let _ = parts.next();
                    let path = parts.next().unwrap_or("");
                    let payload = parts.next().unwrap_or("");
                    received_clone
                        .lock()
                        .expect("lock payloads")
                        .push(payload.to_owned());
                    let ack = format!("OK ECHO path={path} bytes={}", payload.len());
                    write_frame(&mut stream, &ack);
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
        }
    });

    let retry = CohshRetryPolicy {
        max_attempts: 3,
        backoff_ms: 1,
        ceiling_ms: 4,
        timeout_ms: 200,
    };
    let mut transport = TcpTransport::new("127.0.0.1", port)
        .with_retry_policy(retry)
        .with_heartbeat_interval(Duration::from_millis(200))
        .with_auth_token("changeme");
    let session = transport
        .attach(Role::Queen, None)
        .expect("attach tcp session");
    assert!(transport.inject_short_write(8));
    transport
        .write(&session, "/log/queen.log", b"pool-retry")
        .expect("retry write");

    let payloads = received.lock().expect("lock payloads");
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0], "pool-retry");
    let metrics = transport.metrics();
    assert!(metrics.reconnects > 0);

    drop(transport);
    handle.join().expect("join server");
}
