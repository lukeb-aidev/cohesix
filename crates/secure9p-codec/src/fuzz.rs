// Author: Lukas Bower
// Purpose: Provide a fuzz corpus harness for Secure9P frame decoding.

//! Fuzz corpus harnesses for Secure9P frame decoding.

use crate::Codec;

/// Exercise decoder paths on arbitrary corpus bytes.
pub fn fuzz_decode(bytes: &[u8]) {
    let codec = Codec;
    let _ = codec.decode_request(bytes);
    let _ = codec.decode_response(bytes);
}
