// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20

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
    pub mod busybox;
    pub mod initfs;
    pub mod plan9;
}

pub mod physics {
    pub mod cuda;
}

pub mod security {
    pub mod l4_verified;
}

pub mod syscalls {
    pub mod syscall;
    pub mod syscall_table;
}
pub mod upgrade;
