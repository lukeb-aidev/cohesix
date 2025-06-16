// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-27

pub mod boot {
    pub mod bootloader;
    pub mod secure_boot;
    pub mod role_hooks;
}

pub mod drivers {
    pub mod net;
    pub mod gpu;
}

pub mod fs {
    #[cfg(feature = "busybox")]
    pub mod busybox;
    pub mod initfs;
    pub mod plan9;
    #[cfg(feature = "minimal_uefi")]
    pub mod fat;
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

pub mod proc_mgr;
pub mod userland_bootstrap;
pub mod user_api;
#[cfg(not(target_os = "uefi"))]
pub mod config;
