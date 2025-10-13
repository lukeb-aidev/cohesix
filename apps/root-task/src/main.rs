// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Root task entry points for host and seL4 builds."]

#[cfg(target_os = "none")]
use core::panic::PanicInfo;

#[cfg(target_os = "none")]
use root_task::kernel;

#[cfg(target_os = "none")]
/// Ensures the kernel entry points remain linked when building for seL4.
#[used]
static FORCE_KERNEL_LINK: extern "C" fn(*const kernel::BootInfoHeader) -> ! = kernel::kernel_start;

#[cfg(not(target_os = "none"))]
use root_task::host;

#[cfg(not(target_os = "none"))]
fn main() -> host::Result<()> {
    host::main()
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel::panic_handler(info)
}
