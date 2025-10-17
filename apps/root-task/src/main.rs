// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![cfg_attr(feature = "kernel", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Root task entry points for host and seL4 builds."]

#[cfg(all(target_os = "none", not(feature = "kernel")))]
compile_error!("enable the `kernel` feature when building root-task for seL4 targets");

#[cfg(feature = "kernel")]
use root_task::sel4::BootInfo;
#[cfg(feature = "kernel")]
use sel4_panicking as _;
#[cfg(feature = "kernel")]
use sel4_runtime as _;

#[cfg(feature = "kernel")]
mod sel4_entry {
    #![doc = "seL4 runtime entry shim exposed with an unmangled symbol."]
    #![allow(unsafe_code)]

    use super::*;

    /// seL4 entry point invoked by `sel4_runtime`.
    ///
    /// The symbol must remain unmangled because `sel4_runtime`'s `_start`
    /// trampoline performs a raw C call into this function. We explicitly
    /// allow the linted `no_mangle` attribute here to keep the rest of the
    /// crate `#![deny(unsafe_code)]`.
    #[no_mangle]
    pub extern "C" fn sel4_start(bootinfo: &'static BootInfo) -> ! {
        use root_task::platform::SeL4Platform;

        let platform = SeL4Platform::new(bootinfo as *const _ as *const core::ffi::c_void);
        root_task::kernel::start(bootinfo, &platform)
    }
}

#[cfg(feature = "kernel")]
#[doc(hidden)]
pub use sel4_entry::sel4_start;

#[cfg(all(not(feature = "kernel"), not(target_os = "none")))]
fn main() -> root_task::host::Result<()> {
    root_task::host::main()
}
