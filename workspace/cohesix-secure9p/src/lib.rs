// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.4
// Author: Lukas Bower
// Date Modified: 2027-01-31

#![cfg_attr(not(feature = "std"), no_std)]
//! Secure 9P extensions for Cohesix.

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod secure_stream;
#[cfg(all(feature = "std", feature = "tls"))]
pub mod tls_server;
