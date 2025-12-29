// Author: Lukas Bower
// Purpose: Audit helpers for consolidating boot-time logging without changing semantics.

#![cfg(feature = "kernel")]

/// Boot audit helpers used during early kernel bring-up.
pub mod boot;
