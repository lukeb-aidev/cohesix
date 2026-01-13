// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Exercise Secure9P batched pipelining and back-pressure handling.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::sync::Arc;
use std::time::Instant;

use cohesix_ticket::Role;
use nine_door::{Clock, NineDoor, Pipeline, PipelineConfig};
use secure9p_core::{SessionLimits, ShortWritePolicy};
use secure9p_codec::{BatchIter, Codec, OpenMode, Request, RequestBody, ResponseBody, MAX_MSIZE};

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

fn setup_session(server: &NineDoor) -> nine_door::InProcessConnection {
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, Role::Queen).expect("attach");
    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    client.walk(1, 2, &log_path).expect("walk /log/queen.log");
    client
        .open(2, OpenMode::write_append())
        .expect("open /log/queen.log");
    client
}

fn build_write_batch(
    codec: &Codec,
    fid: u32,
    tag_base: u16,
    payloads: &[&[u8]],
) -> (Vec<u8>, Vec<(u16, usize)>) {
    let mut batch = Vec::new();
    let mut tags = Vec::new();
    for (idx, payload) in payloads.iter().enumerate() {
        let tag = tag_base + idx as u16;
        tags.push((tag, payload.len()));
        let request = Request {
            tag,
            body: RequestBody::Write {
                fid,
                offset: u64::MAX,
                data: payload.to_vec(),
            },
        };
        let frame = codec.encode_request(&request).expect("encode request");
        batch.extend_from_slice(&frame);
    }
    (batch, tags)
}

fn decode_responses(codec: &Codec, response_bytes: &[u8]) -> Vec<(u16, ResponseBody)> {
    BatchIter::new(response_bytes)
        .collect::<Result<Vec<_>, _>>()
        .expect("batch decode")
        .into_iter()
        .map(|frame| {
            let response = codec
                .decode_response(frame.bytes())
                .expect("decode response");
            (response.tag, response.body)
        })
        .collect()
}

#[test]
fn batched_sessions_handle_out_of_order_responses() {
    let limits = SessionLimits {
        tags_per_session: 8,
        batch_frames: 4,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut sessions = (0..4).map(|_| setup_session(&server)).collect::<Vec<_>>();
    let codec = Codec;

    for (idx, session) in sessions.iter_mut().enumerate() {
        let payloads = [
            format!("batch-{idx}-alpha").into_bytes(),
            format!("batch-{idx}-beta").into_bytes(),
        ];
        let payload_refs: Vec<&[u8]> = payloads.iter().map(|p| p.as_slice()).collect();
        let (batch, expected) = build_write_batch(&codec, 2, 100, &payload_refs);
        let response_bytes = session.exchange_batch(&batch).expect("batch exchange");
        let mut frames = BatchIter::new(&response_bytes)
            .collect::<Result<Vec<_>, _>>()
            .expect("batch decode");
        frames.reverse();

        let mut seen = Vec::new();
        for frame in frames {
            let response = codec
                .decode_response(frame.bytes())
                .expect("decode response");
            let ResponseBody::Write { count } = response.body else {
                panic!("unexpected response body: {:?}", response.body);
            };
            seen.push((response.tag, count as usize));
        }
        seen.sort_by_key(|(tag, _)| *tag);
        assert_eq!(seen, expected);
    }
}

#[test]
fn synthetic_load_interleaves_sessions() {
    let limits = SessionLimits {
        tags_per_session: 8,
        batch_frames: 4,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut sessions = (0..4).map(|_| setup_session(&server)).collect::<Vec<_>>();
    let codec = Codec;
    let mut tag_seed: u16 = 1000;
    let mut ops = 0usize;
    let mut batch_idx = 0usize;

    while ops < 10_000 {
        let remaining = 10_000 - ops;
        let batch_size = if remaining >= 2 { 2 } else { 1 };
        let payloads = (0..batch_size)
            .map(|offset| format!("load-{ops}-{offset}").into_bytes())
            .collect::<Vec<_>>();
        let payload_refs = payloads.iter().map(|p| p.as_slice()).collect::<Vec<_>>();
        let (batch, expected) = build_write_batch(&codec, 2, tag_seed, &payload_refs);
        let session_idx = batch_idx % sessions.len();
        let response_bytes = sessions[session_idx]
            .exchange_batch(&batch)
            .expect("batch exchange");
        let mut seen = Vec::new();
        for (tag, body) in decode_responses(&codec, &response_bytes) {
            let ResponseBody::Write { count } = body else {
                panic!("unexpected response body: {body:?}");
            };
            seen.push((tag, count as usize));
        }
        seen.sort_by_key(|(tag, _)| *tag);
        assert_eq!(seen, expected);

        ops = ops.saturating_add(batch_size);
        tag_seed = tag_seed.wrapping_add(batch_size as u16);
        batch_idx = batch_idx.saturating_add(1);
    }

    let metrics = server.pipeline_metrics();
    assert_eq!(metrics.queue_limit, limits.queue_depth_limit());
    assert_eq!(metrics.queue_depth, 0);
}

#[test]
fn single_frame_round_trip_with_batching_disabled() {
    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);
    let codec = Codec;

    let payloads = [b"single-frame".as_slice()];
    let (batch, expected) = build_write_batch(&codec, 2, 300, &payloads);
    let response_bytes = client.exchange_batch(&batch).expect("batch exchange");
    let mut seen = Vec::new();
    for (tag, body) in decode_responses(&codec, &response_bytes) {
        let ResponseBody::Write { count } = body else {
            panic!("unexpected response body: {body:?}");
        };
        seen.push((tag, count as usize));
    }
    seen.sort_by_key(|(tag, _)| *tag);
    assert_eq!(seen, expected);
}

struct FlakyWriter {
    fail_writes: usize,
    attempts: usize,
}

impl FlakyWriter {
    fn new(fail_writes: usize) -> Self {
        Self {
            fail_writes,
            attempts: 0,
        }
    }
}

impl Write for FlakyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.attempts += 1;
        if self.attempts <= self.fail_writes {
            return Ok(0);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn backpressure_and_short_write_retries_are_bounded() {
    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);
    let codec = Codec;

    let payloads = [b"overflow-a".as_slice(), b"overflow-b".as_slice()];
    let (batch, _) = build_write_batch(&codec, 2, 200, &payloads);
    let response_bytes = client.exchange_batch(&batch).expect("batch exchange");
    let responses = BatchIter::new(&response_bytes)
        .collect::<Result<Vec<_>, _>>()
        .expect("batch decode");
    for frame in responses {
        let response = codec
            .decode_response(frame.bytes())
            .expect("decode response");
        let ResponseBody::Error { code, message } = response.body else {
            panic!("expected error response");
        };
        assert_eq!(code, secure9p_codec::ErrorCode::Busy);
        assert_eq!(message, "queue depth exceeded");
    }
    assert!(server.pipeline_metrics().backpressure_events > 0);

    let mut pipeline = Pipeline::new(PipelineConfig {
        batch_frames: 1,
        queue_depth: 1,
        short_write_policy: ShortWritePolicy::Retry,
    });
    let mut writer = FlakyWriter::new(4);
    let err = pipeline
        .write_batch(&mut writer, &[vec![0_u8; 8]])
        .expect_err("short write should fail");
    assert_eq!(err.kind(), io::ErrorKind::WriteZero);
    let metrics = pipeline.metrics();
    assert_eq!(metrics.short_write_retries, 3);
    assert!(metrics.short_writes >= 3);
}

#[test]
fn tag_window_overflow_is_deterministic() {
    let limits = SessionLimits {
        tags_per_session: 1,
        batch_frames: 2,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);
    let codec = Codec;

    let payloads = [b"tag-a".as_slice(), b"tag-b".as_slice()];
    let (batch, expected) = build_write_batch(&codec, 2, 400, &payloads);
    let response_bytes = client.exchange_batch(&batch).expect("batch exchange");
    let mut seen = Vec::new();
    for (tag, body) in decode_responses(&codec, &response_bytes) {
        seen.push((tag, body));
    }
    seen.sort_by_key(|(tag, _)| *tag);

    let mut expected_sorted = expected;
    expected_sorted.sort_by_key(|(tag, _)| *tag);
    assert_eq!(seen.len(), expected_sorted.len());

    for (idx, (tag, body)) in seen.into_iter().enumerate() {
        if idx == 0 {
            let ResponseBody::Write { count } = body else {
                panic!("expected write response for tag={tag}");
            };
            assert_eq!(count as usize, payloads[0].len());
        } else {
            let ResponseBody::Error { code, message } = body else {
                panic!("expected error response for tag={tag}");
            };
            assert_eq!(code, secure9p_codec::ErrorCode::Busy);
            assert_eq!(message, "tag window exceeded");
        }
    }
}
