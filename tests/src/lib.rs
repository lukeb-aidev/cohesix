// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Shared helpers for Cohesix integration tests.

use std::time::Duration;

/// Helper constant mirroring the short timeout used by the TCP integration tests.
pub const TEST_TIMEOUT: Duration = Duration::from_millis(200);
