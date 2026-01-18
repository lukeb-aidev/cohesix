// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Exercise sharded worker namespace scaling and attach latency.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use nine_door::{NineDoor, ShardLayout};
use secure9p_codec::{OpenMode, MAX_MSIZE};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const WORKER_COUNT: usize = 1_000;
const ATTACH_THREADS: usize = 32;

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[test]
fn sharded_attach_scales_and_exports_metrics() {
    let server = NineDoor::new();
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");

    let mut queen = server.connect().expect("queen session");
    queen.version(MAX_MSIZE).expect("version");
    queen.attach(1, Role::Queen).expect("attach queen");
    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen.open(2, OpenMode::write_append()).expect("open ctl");
    for _ in 0..WORKER_COUNT {
        queen
            .write(2, b"{\"spawn\":\"heartbeat\",\"ticks\":5}\n")
            .expect("spawn worker");
    }
    queen.clunk(2).expect("clunk ctl");

    let issuer = TicketIssuer::new("worker-secret");
    let workers: Vec<(String, String)> = (1..=WORKER_COUNT)
        .map(|idx| {
            let worker_id = format!("worker-{idx}");
            let claims = TicketClaims::new(
                Role::WorkerHeartbeat,
                BudgetSpec::default_heartbeat(),
                Some(worker_id.clone()),
                MountSpec::empty(),
                unix_time_ms(),
            );
            let token = issuer.issue(claims).unwrap().encode().unwrap();
            (worker_id, token)
        })
        .collect();
    let workers = Arc::new(workers);

    let next_index = Arc::new(AtomicUsize::new(0));
    let durations = Arc::new(Mutex::new(Vec::with_capacity(WORKER_COUNT)));
    let threads = ATTACH_THREADS.min(WORKER_COUNT);

    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        let server = server.clone();
        let workers = Arc::clone(&workers);
        let next_index = Arc::clone(&next_index);
        let durations = Arc::clone(&durations);
        handles.push(std::thread::spawn(move || {
            let mut local = Vec::new();
            loop {
                let idx = next_index.fetch_add(1, Ordering::SeqCst);
                if idx >= workers.len() {
                    break;
                }
                let (worker_id, token) = &workers[idx];
                let start = Instant::now();
                let mut client = server.connect().expect("worker session");
                client.version(MAX_MSIZE).expect("version");
                client
                    .attach_with_identity(
                        1,
                        Role::WorkerHeartbeat,
                        Some(worker_id.as_str()),
                        Some(token.as_str()),
                    )
                    .expect("attach worker");
                local.push(start.elapsed());
            }
            durations.lock().unwrap().extend(local);
        }));
    }
    for handle in handles {
        handle.join().expect("attach thread");
    }

    let durations = durations.lock().unwrap();
    assert_eq!(durations.len(), WORKER_COUNT);
    let total_ms: f64 = durations.iter().map(|d| d.as_secs_f64() * 1_000.0).sum();
    let max_ms = durations
        .iter()
        .map(|d| d.as_secs_f64() * 1_000.0)
        .fold(0.0, f64::max);
    let avg_ms = total_ms / WORKER_COUNT as f64;
    println!(
        "attach latency: workers={} avg_ms={avg_ms:.3} max_ms={max_ms:.3}",
        WORKER_COUNT
    );

    let sessions_path = vec!["proc".to_owned(), "9p".to_owned(), "sessions".to_owned()];
    queen.walk(1, 3, &sessions_path).expect("walk sessions");
    queen.open(3, OpenMode::read_only()).expect("open sessions");
    let data = queen.read(3, 0, MAX_MSIZE).expect("read sessions");
    let sessions = String::from_utf8(data).expect("sessions utf8");
    let mut total_sessions = None;
    let mut worker_sessions = None;
    let mut shard_count = None;
    let mut shard_lines = 0usize;
    let mut shard_total = 0usize;
    for line in sessions.lines() {
        if let Some(rest) = line.strip_prefix("sessions ") {
            for part in rest.split_whitespace() {
                if let Some((key, value)) = part.split_once('=') {
                    if let Ok(parsed) = value.parse::<usize>() {
                        match key {
                            "total" => total_sessions = Some(parsed),
                            "worker" => worker_sessions = Some(parsed),
                            "shard_count" => shard_count = Some(parsed),
                            _ => {}
                        }
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("shard ") {
            let mut parts = rest.split_whitespace();
            let _label = parts.next();
            let count = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            shard_total = shard_total.saturating_add(count);
            shard_lines += 1;
        }
    }
    assert_eq!(worker_sessions, Some(WORKER_COUNT));
    assert!(total_sessions.unwrap_or(0) >= WORKER_COUNT + 1);
    assert_eq!(shard_total, WORKER_COUNT);

    let shards = ShardLayout::default();
    assert!(shards.worker_telemetry_path("worker-1").len() <= 8);
    if shards.legacy_worker_alias_enabled() {
        assert!(vec!["worker", "worker-1", "telemetry"].len() <= 8);
    }
    assert_eq!(shard_count, Some(shards.shard_count()));
    assert!(shard_lines >= shards.shard_count());
}
