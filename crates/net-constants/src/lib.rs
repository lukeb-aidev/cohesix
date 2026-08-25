// Copyright © 2026 Lukas Bower
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

/// Maximum ordered namespace commands carried by one bounded transport activation.
///
/// This is shared by the target console ABI, direct TCP clients, and REST
/// projection so host layers cannot silently fragment one guest service
/// quantum into smaller transport turns.
pub const COHESIX_TRANSPORT_COMMAND_BATCH_MAX: usize = 8;

/// Maximum time the Hive Gateway broker may spend admitting one queued request.
pub const HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS: u64 = 5_000;
/// Default time the Hive Gateway broker may wait for one target response.
pub const HIVE_GATEWAY_DEFAULT_BROKER_RESPONSE_TIMEOUT_MS: u64 = 120_000;
/// Client-side grace after broker queue and response deadlines for HTTP delivery.
pub const HIVE_GATEWAY_REST_RESPONSE_GRACE_MS: u64 = 5_000;
/// Short HTTP timeout retained for metadata, resolution, connection, and response bodies.
pub const HIVE_GATEWAY_REST_IO_TIMEOUT_MS: u64 = 3_000;
/// Default `/v1/fs/*` receive-response timeout covering the full broker contract.
pub const HIVE_GATEWAY_REST_OPERATION_RESPONSE_TIMEOUT_MS: u64 =
    HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS
        + HIVE_GATEWAY_DEFAULT_BROKER_RESPONSE_TIMEOUT_MS
        + HIVE_GATEWAY_REST_RESPONSE_GRACE_MS;
