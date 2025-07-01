// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-17
// Worker init uses rand; the module is excluded for UEFI builds.

use crate::prelude::*;
//! Initialization routines for Cohesix roles.

pub mod kiosk;
pub mod queen;
pub mod sensor;

pub mod worker;
