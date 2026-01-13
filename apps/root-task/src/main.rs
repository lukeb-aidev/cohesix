// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the root-task binary entrypoint.
// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![cfg_attr(feature = "kernel", no_main)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
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
pub use sel4_runtime::_start;

#[cfg(feature = "kernel")]
mod sel4_entry {
    #![doc = "seL4 entry shim exposed with an unmangled symbol."]
    #![allow(unsafe_code)]

    use super::*;

    /// seL4 entry point invoked by the assembly `_start` trampoline provided by `sel4_runtime`.
    ///
    /// The symbol must remain unmangled because the `_start` shim branches into
    /// this function before the broader kernel startup sequence executes.
    #[no_mangle]
    pub extern "C" fn sel4_start(bootinfo: &'static BootInfo) -> ! {
        use root_task::platform::SeL4Platform;

        let _ = sel4_runtime::bootinfo();

        let platform = SeL4Platform::new(core::ptr::from_ref(bootinfo).cast());
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
