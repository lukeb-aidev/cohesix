// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-12-31

#![cfg_attr(not(feature = "posix"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "posix")]
extern crate std;
use alloc::{boxed::Box, string::String};

/// Common Cohesix 9P error type.
pub type CohError = Box<dyn core::error::Error + Send + Sync>;

#[derive(Debug)]
struct StringError(String);

impl core::fmt::Display for StringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for StringError {}

pub(crate) fn new_err(msg: impl Into<String>) -> CohError {
    Box::new(StringError(msg.into()))
}

#[macro_export]
macro_rules! coh_bail {
    ($($arg:tt)+) => { return Err($crate::new_err(format!($($arg)+))); };
}

#[macro_export]
macro_rules! coh_error {
    ($($arg:tt)+) => { $crate::new_err(format!($($arg)+)) };
}

#[derive(Debug, Clone)]
pub struct FsConfig {
    pub root: String,
    pub port: u16,
    pub readonly: bool,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self { root: String::from("/"), port: 564, readonly: false }
    }
}

pub mod fs;
pub mod policy;

#[cfg(feature = "inprocess")]
pub mod inprocess;
#[cfg(feature = "inprocess")]
pub use inprocess::InProcessStream;

#[cfg(feature = "posix")]
mod server;
#[cfg(feature = "posix")]
pub use server::FsServer;

/// Convenience helper when `posix` feature is enabled.
#[cfg(feature = "posix")]
pub fn start_server() -> Result<FsServer, CohError> {
    let mut srv = FsServer::new(Default::default());
    srv.start()?;
    Ok(srv)
}
