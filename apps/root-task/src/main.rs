// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![cfg_attr(feature = "kernel", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Root task entry points for host and seL4 builds."]

#[cfg(all(target_os = "none", not(feature = "kernel")))]
compile_error!("enable the `kernel` feature when building root-task for seL4 targets");

#[cfg(feature = "kernel")]
use sel4::BootInfo;
#[cfg(feature = "kernel")]
use sel4_panicking as _;
#[cfg(feature = "kernel")]
use sel4_runtime as _;

#[cfg(feature = "kernel")]
/// seL4 entry point invoked by `sel4_runtime`.
#[no_mangle]
pub extern "C" fn sel4_start(bootinfo: &'static BootInfo) -> ! {
    use root_task::platform::SeL4Platform;

    let platform = SeL4Platform::new(bootinfo as *const _ as *const core::ffi::c_void);
    root_task::kernel::start(bootinfo, &platform)
}

#[cfg(all(not(feature = "kernel"), not(target_os = "none")))]
fn main() -> root_task::host::Result<()> {
    root_task::host::main()
}
