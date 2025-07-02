// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-27

use crate::prelude::*;
pub mod boot {
    pub mod bootloader;
    pub mod role_hooks;
    pub mod secure_boot;
}

pub mod drivers {
    pub mod gpu;
    pub mod net;
}

pub mod fs {
    #[cfg(feature = "busybox_client")]
    pub mod busybox;
    #[cfg(feature = "minimal_uefi")]
    pub mod fat;
    pub mod initfs;
    pub mod plan9;
}

pub mod physics {
    pub mod cuda;
}

pub mod security {
    pub mod l4_verified;
}

pub mod kernel_trace;

pub mod syscalls {
    pub mod syscall;
    pub mod syscall_table;
}
pub mod upgrade;

pub mod loader;
pub mod proc_mgr;
pub mod user_api;
pub mod userland_bootstrap;

pub mod config;
