// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate batch iterator framing and size enforcement.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use secure9p_codec::{BatchIter, Codec, CodecError, Request, RequestBody, VERSION, MAX_MSIZE};

#[test]
fn batch_iter_splits_frames() {
    let codec = Codec;
    let request_a = Request {
        tag: 1,
        body: RequestBody::Version {
            msize: MAX_MSIZE,
            version: VERSION.to_string(),
        },
    };
    let request_b = Request {
        tag: 2,
        body: RequestBody::Attach {
            fid: 1,
            afid: 0,
            uname: "queen".to_owned(),
            aname: "".to_owned(),
            n_uname: 0,
        },
    };
    let frame_a = codec.encode_request(&request_a).expect("encode frame a");
    let frame_b = codec.encode_request(&request_b).expect("encode frame b");
    let mut batch = Vec::new();
    batch.extend_from_slice(&frame_a);
    batch.extend_from_slice(&frame_b);

    let frames = BatchIter::new(&batch)
        .collect::<Result<Vec<_>, _>>()
        .expect("batch iter ok");
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].bytes(), frame_a.as_slice());
    assert_eq!(frames[1].bytes(), frame_b.as_slice());
}

#[test]
fn batch_iter_rejects_oversized_frame() {
    let mut frame = vec![0u8; 9];
    let declared = (MAX_MSIZE + 1).to_le_bytes();
    frame[..4].copy_from_slice(&declared);
    frame[4] = 100;
    let mut iter = BatchIter::with_max_frame(&frame, MAX_MSIZE);
    let err = iter.next().expect("frame expected").expect_err("oversize");
    assert!(matches!(err, CodecError::FrameTooLarge { .. }));
}
