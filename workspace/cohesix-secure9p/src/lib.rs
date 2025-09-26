// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.5
// Author: Lukas Bower
// Date Modified: 2029-09-26

#![cfg_attr(not(feature = "std"), no_std)]
//! Secure 9P extensions for Cohesix.

extern crate alloc;
#[cfg(test)]
extern crate std;

#[cfg(feature = "std")]
pub mod config;
#[cfg(feature = "std")]
pub mod manifest;
#[cfg(feature = "std")]
pub mod reconcile;
pub mod secure_stream;
#[cfg(all(feature = "std", feature = "tls"))]
pub mod tls_server;
