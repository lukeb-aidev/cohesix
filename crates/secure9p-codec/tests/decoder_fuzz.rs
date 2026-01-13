// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Fuzz-style regression tests for Secure9P codec framing.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use secure9p_codec::{Codec, OpenMode, Qid, QidType, Request, RequestBody, Response, ResponseBody};

#[test]
fn fuzz_decode_round_trips() {
    let iterations = std::env::var("SECURE9P_FUZZ_ITERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(512);
    let mut rng = StdRng::seed_from_u64(0xC0DEC0DE_u64);
    let codec = Codec;

    for _ in 0..iterations {
        let mut frame = codec.encode_request(&random_request(&mut rng)).unwrap();
        mutate_frame(&mut rng, &mut frame);
        let result = catch_unwind(AssertUnwindSafe(|| codec.decode_request(&frame)));
        assert!(result.is_ok(), "request decoder panicked on mutated frame");
    }

    for _ in 0..iterations {
        let mut frame = codec.encode_response(&random_response(&mut rng)).unwrap();
        mutate_frame(&mut rng, &mut frame);
        let result = catch_unwind(AssertUnwindSafe(|| codec.decode_response(&frame)));
        assert!(result.is_ok(), "response decoder panicked on mutated frame");
    }
}

fn mutate_frame<R: Rng>(rng: &mut R, frame: &mut Vec<u8>) {
    if frame.len() < 5 {
        return;
    }
    match rng.random_range(0..3) {
        0 => {
            let declared: u32 = rng.random();
            frame[0..4].copy_from_slice(&declared.to_le_bytes());
        }
        1 => {
            if frame.len() > 6 {
                let new_len = rng.random_range(5..frame.len());
                frame.truncate(new_len);
                if rng.random_bool(0.5) {
                    frame[0..4].copy_from_slice(&(new_len as u32).to_le_bytes());
                }
            }
        }
        _ => {
            let tail_len = rng.random_range(1..16);
            let mut tail = vec![0u8; tail_len];
            rng.fill_bytes(&mut tail);
            frame.extend_from_slice(&tail);
            if rng.random_bool(0.5) {
                let declared = frame.len() as u32;
                frame[0..4].copy_from_slice(&declared.to_le_bytes());
            }
        }
    }

    frame[4] ^= rng.random_range(1..=0x7F);
}

fn random_request<R: Rng>(rng: &mut R) -> Request {
    let tag = rng.random();
    match rng.random_range(0..6) {
        0 => Request {
            tag,
            body: RequestBody::Version {
                msize: rng.random_range(256..=secure9p_codec::MAX_MSIZE),
                version: "9P2000.L".to_owned(),
            },
        },
        1 => Request {
            tag,
            body: RequestBody::Attach {
                fid: rng.random(),
                afid: 0,
                uname: random_atom(rng, 6),
                aname: random_atom(rng, 4),
                n_uname: rng.random(),
            },
        },
        2 => Request {
            tag,
            body: RequestBody::Walk {
                fid: rng.random(),
                newfid: rng.random(),
                wnames: (0..rng.random_range(0..4))
                    .map(|_| random_atom(rng, 5))
                    .collect(),
            },
        },
        3 => Request {
            tag,
            body: RequestBody::Open {
                fid: rng.random(),
                mode: if rng.random_bool(0.5) {
                    OpenMode::read_only()
                } else {
                    OpenMode::write_append()
                },
            },
        },
        4 => Request {
            tag,
            body: RequestBody::Read {
                fid: rng.random(),
                offset: rng.random(),
                count: rng.random_range(0..512),
            },
        },
        _ => {
            let mut data = vec![0u8; rng.random_range(0..64)];
            rng.fill_bytes(&mut data);
            Request {
                tag,
                body: RequestBody::Write {
                    fid: rng.random(),
                    offset: rng.random(),
                    data,
                },
            }
        }
    }
}

fn random_response<R: Rng>(rng: &mut R) -> Response {
    let tag = rng.random();
    match rng.random_range(0..6) {
        0 => Response {
            tag,
            body: ResponseBody::Version {
                msize: rng.random_range(256..=secure9p_codec::MAX_MSIZE),
                version: "9P2000.L".to_owned(),
            },
        },
        1 => Response {
            tag,
            body: ResponseBody::Attach {
                qid: random_qid(rng),
            },
        },
        2 => Response {
            tag,
            body: ResponseBody::Walk {
                qids: (0..rng.random_range(0..4))
                    .map(|_| random_qid(rng))
                    .collect(),
            },
        },
        3 => Response {
            tag,
            body: ResponseBody::Open {
                qid: random_qid(rng),
                iounit: rng.random_range(1..=secure9p_codec::MAX_MSIZE),
            },
        },
        4 => Response {
            tag,
            body: ResponseBody::Read {
                data: {
                    let mut buf = vec![0u8; rng.random_range(0..64)];
                    rng.fill_bytes(&mut buf);
                    buf
                },
            },
        },
        _ => Response {
            tag,
            body: ResponseBody::Error {
                code: secure9p_codec::ErrorCode::Permission,
                message: random_atom(rng, 12),
            },
        },
    }
}

fn random_atom<R: Rng>(rng: &mut R, max_len: usize) -> String {
    let len = rng.random_range(1..=max_len.max(1));
    (0..len)
        .map(|_| {
            const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
            let idx = rng.random_range(0..ALPHABET.len());
            ALPHABET[idx] as char
        })
        .collect()
}

fn random_qid<R: Rng>(rng: &mut R) -> Qid {
    let ty = if rng.random_bool(0.5) {
        QidType::DIRECTORY
    } else {
        QidType::FILE
    };
    Qid::new(ty, rng.random(), rng.random())
}
