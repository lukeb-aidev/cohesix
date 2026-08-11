// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the nine-door library and public module surface.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! NineDoor Secure9P server implementation for host builds.
//!
//! Target namespace parsing is implemented by the separately packaged
//! `nine-door-runtime` child. This crate intentionally has no target binary.

pub use secure9p_transport::{
    NamespaceOpcode, NamespaceRequestHeader, NamespaceResponseHeader, NamespaceStatus,
    PreparedNamespaceOperation, RequestToken, TransportError, NAMESPACE_SERVICE_ABI_VERSION,
};

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
pub use host::*;
