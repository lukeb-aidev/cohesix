// Author: Lukas Bower

#![no_std]
#![deny(unsafe_code)]
#![deny(missing_docs)]

//! Shared networking constants for Cohesix components.

/// Default TCP port exposed by the Cohesix console listener.
pub const COHSH_TCP_PORT: u16 = 31337;
/// Alias for the TCP console port constant.
pub const TCP_CONSOLE_PORT: u16 = COHSH_TCP_PORT;
