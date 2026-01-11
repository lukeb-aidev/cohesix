// Author: Lukas Bower
// Purpose: Provide Secure9P wire types and codec primitives for host and VM code.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Secure9P wire types and codec primitives shared across Cohesix crates,
//! aligned with `docs/ARCHITECTURE.md` §2-§3 and the policy requirements in
//! `docs/SECURE9P.md`.

extern crate alloc;

#[cfg(test)]
extern crate std;

mod batch;
mod codec;
mod fuzz;
mod types;

pub use batch::{BatchFrame, BatchIter};
pub use codec::{decode_request, decode_response, encode_request, encode_response, Codec};
pub use fuzz::fuzz_decode;
pub use types::*;
