// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the net-constants library and public module surface.
// Author: Lukas Bower

#![no_std]
#![deny(unsafe_code)]
#![deny(missing_docs)]

//! Shared networking constants for Cohesix components.

/// Default TCP port exposed by the Cohesix console listener.
pub const COHESIX_TCP_CONSOLE_PORT: u16 = 31337;
/// Backwards-compatible alias for the TCP console port constant.
pub const COHSH_TCP_PORT: u16 = COHESIX_TCP_CONSOLE_PORT;
/// Alias for the TCP console port constant.
pub const TCP_CONSOLE_PORT: u16 = COHESIX_TCP_CONSOLE_PORT;
