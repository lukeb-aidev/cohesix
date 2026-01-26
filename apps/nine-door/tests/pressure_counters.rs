// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate /proc/pressure counters for refusals and cuts.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use cohesix_ticket::{
    BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer, TicketQuotas, TicketScope, TicketVerb,
};
use nine_door::{Clock, InProcessConnection, NineDoor, NineDoorError};
use secure9p_codec::{ErrorCode, OpenMode, Request, RequestBody, MAX_MSIZE};
use secure9p_core::{SessionLimits, ShortWritePolicy};

const QUEEN_SECRET: &str = "queen-secret";

struct FixedClock {
    now: Instant,
}

impl FixedClock {
    fn new() -> Self {
        Self { now: Instant::now() }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Instant {
        self.now
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, Role::Queen).expect("attach");
    client
}

fn attach_with_ticket(server: &NineDoor, token: &str) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client
        .attach_with_identity(1, Role::Queen, None, Some(token))
        .expect("attach ticket");
    client
}

fn read_proc_text(client: &mut InProcessConnection, fid: u32, path: &[&str]) -> String {
    let components = path.iter().map(|seg| seg.to_string()).collect::<Vec<_>>();
    client.walk(1, fid, &components).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

fn parse_kv(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == key {
                v.parse::<u64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

#[test]
fn pressure_counters_increment_on_refusals() {
    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    server.register_ticket_secret(Role::Queen, QUEEN_SECRET);

    let mut busy_client = attach_queen(&server);
    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    busy_client.walk(1, 2, &log_path).expect("walk log");
    busy_client
        .open(2, OpenMode::write_append())
        .expect("open log");
    let request_a = Request {
        tag: 10,
        body: RequestBody::Write {
            fid: 2,
            offset: u64::MAX,
            data: b"pressure-a".to_vec(),
        },
    };
    let request_b = Request {
        tag: 11,
        body: RequestBody::Write {
            fid: 2,
            offset: u64::MAX,
            data: b"pressure-b".to_vec(),
        },
    };
    let codec = secure9p_codec::Codec;
    let mut batch = Vec::new();
    batch.extend_from_slice(&codec.encode_request(&request_a).expect("encode a"));
    batch.extend_from_slice(&codec.encode_request(&request_b).expect("encode b"));
    let _ = busy_client.exchange_batch(&batch).expect("batch");
    busy_client.clunk(2).ok();

    let quotas = TicketQuotas {
        bandwidth_bytes: Some(1),
        cursor_resumes: None,
        cursor_advances: None,
    };
    let claims = TicketClaims::new(
        Role::Queen,
        BudgetSpec::unbounded(),
        None,
        MountSpec::empty(),
        unix_time_ms(),
    )
    .with_scopes(vec![TicketScope::new("/proc/boot", TicketVerb::Read, 0)])
    .with_quotas(quotas);
    let token = TicketIssuer::new(QUEEN_SECRET)
        .issue(claims)
        .expect("issue")
        .encode()
        .expect("encode");
    let mut ticket_client = attach_with_ticket(&server, &token);

    let deny_path = vec![
        "proc".to_owned(),
        "lifecycle".to_owned(),
        "state".to_owned(),
    ];
    let err = ticket_client.walk(1, 3, &deny_path).expect_err("scope deny");
    match err {
        NineDoorError::Protocol { code, .. } => {
            assert_eq!(code, ErrorCode::Permission);
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let boot_path = vec!["proc".to_owned(), "boot".to_owned()];
    ticket_client.walk(1, 4, &boot_path).expect("walk boot");
    ticket_client
        .open(4, OpenMode::read_only())
        .expect("open boot");
    let err = ticket_client.read(4, 0, 16).expect_err("quota deny");
    match err {
        NineDoorError::Protocol { code, .. } => {
            assert_eq!(code, ErrorCode::TooBig);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    ticket_client.clunk(4).ok();

    drop(busy_client);
    drop(ticket_client);

    let mut observer = attach_queen(&server);
    let busy = read_proc_text(&mut observer, 5, &["proc", "pressure", "busy"]);
    let quota = read_proc_text(&mut observer, 6, &["proc", "pressure", "quota"]);
    let cut = read_proc_text(&mut observer, 7, &["proc", "pressure", "cut"]);
    let policy = read_proc_text(&mut observer, 8, &["proc", "pressure", "policy"]);

    assert!(parse_kv(&busy, "busy") >= 1);
    assert!(parse_kv(&quota, "quota") >= 1);
    assert!(parse_kv(&policy, "policy") >= 1);
    assert!(parse_kv(&cut, "cut") >= 1);
}
