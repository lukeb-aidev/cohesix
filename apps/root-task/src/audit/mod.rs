// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Audit helpers for consolidating boot-time logging without changing semantics.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

/// Boot audit helpers used during early kernel bring-up.
pub mod boot;
