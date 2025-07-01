// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

use crate::prelude::*;
/// IPC subsystem

pub mod p9;

pub use p9::{P9Request, P9Response, P9Server, StubP9Server};
