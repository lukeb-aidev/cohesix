// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the drivers module for root-task.
// Author: Lukas Bower

//! Leaf device drivers used by the root task.

#[cfg(all(feature = "kernel", feature = "net-console"))]
pub(crate) mod cyw43_host_eapol;
#[cfg(all(feature = "kernel", feature = "net-console"))]
pub(crate) mod driver_task_net;
pub(crate) mod rtl8139;
#[cfg(feature = "kernel")]
pub(crate) mod virtio;
