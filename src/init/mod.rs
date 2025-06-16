// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-16
// Worker init uses rand; the module is excluded for UEFI builds.

//! Initialization routines for Cohesix roles.

pub mod kiosk;
pub mod queen;
pub mod sensor;
#[cfg(not(feature = "uefi"))]
pub mod worker;
