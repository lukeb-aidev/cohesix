// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod secure_stream;
#[cfg(all(feature = "std", feature = "tls"))]
pub mod tls_server;
