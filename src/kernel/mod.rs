pub mod boot {
    pub mod bootloader;
    pub mod secure_boot;
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
