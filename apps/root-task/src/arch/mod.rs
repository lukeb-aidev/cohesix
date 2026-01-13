// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the arch module for root-task.
// Author: Lukas Bower

#![cfg(target_arch = "aarch64")]

//! Architecture-specific helpers for the root task.

/// AArch64-specific helpers used by the root task runtime.
pub mod aarch64;
