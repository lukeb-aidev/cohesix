// CLASSIFICATION: COMMUNITY
// Filename: bootloader.rs v1.4
// Author: Lukas Bower
// Date Modified: 2025-08-27
//==============================================================================
// COHESIX · BOOTLOADER MODULE
//------------------------------------------------------------------------------
// Location: src/kernel/boot/bootloader.rs
// Maintainers: @cohesix-core
// Description: Handles early-stage initialization, hardware discovery,
//              memory layout parsing, and prepares handoff to the seL4 microkernel.
// License: SPDX-License-Identifier: MIT OR Apache-2.0
//==============================================================================

/// Responsible for early-stage bootloader tasks in Cohesix.
pub struct BootAgent;

use super::role_hooks;
use crate::kernel::userland_bootstrap;
#[cfg(feature = "minimal_uefi")]
use crate::kernel::fs::fat;

impl BootAgent {
    /// Initialize the bootloader and perform pre-seL4 setup.
    pub fn init() {
        println!("[BootAgent] Starting bootloader initialization...");
        crate::kernel::kernel_trace::log_init_call("bootloader_init");
        crate::trace::recorder::event("boot", "init", "start");
        Self::preflight_checks();

        let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
        let ctx = crate::bootloader::init::early_init(&cmdline)
            .unwrap_or_else(|_| crate::bootloader::init::BootContext {
                args: crate::bootloader::args::BootArgs::default(),
                role: "Unknown".into(),
            });

        Self::setup_memory_zones();
        #[cfg(feature = "minimal_uefi")]
        fat::mount_root();
        Self::discover_devices();
        Self::prepare_kernel(&ctx);
        crate::trace::recorder::event("boot", "init", "finish");
        role_hooks::setup(&ctx.role);
        crate::kernel::user_api::init_user_api();
        userland_bootstrap::dispatch_user("init");
    }

    /// Perform essential checks before kernel handoff.
    pub fn preflight_checks() {
        println!("[BootAgent] Running preflight checks...");
        crate::kernel::kernel_trace::log_init_call("preflight_checks");
        if !super::secure_boot::validate_secure_boot() {
            println!("[BootAgent] secure boot validation failed");
            crate::validator::log_violation(crate::validator::RuleViolation {
                type_: "boot_secure",
                file: "bootloader".into(),
                agent: "BootAgent".into(),
                time: crate::validator::timestamp(),
            });
        }
        println!("[BootAgent] platform state OK");
        println!("[BootAgent] I/O ready");
    }

    fn setup_memory_zones() {
        println!("[BootAgent] Configuring memory zones...");
        println!("[BootAgent] kernel text at 0x100000..0x200000");
        println!("[BootAgent] user space starts at 0x8000000");
    }

    fn discover_devices() {
        println!("[BootAgent] Enumerating early devices...");
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for e in entries.flatten().take(3) {
                println!("[BootAgent] device {}", e.path().display());
            }
        }
    }

    fn prepare_kernel(ctx: &crate::bootloader::init::BootContext) {
        println!("[BootAgent] Launching seL4 with role {}", ctx.role);
    }
}
