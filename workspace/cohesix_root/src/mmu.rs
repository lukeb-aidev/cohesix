// CLASSIFICATION: COMMUNITY
// Filename: mmu.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-10-06
#![allow(static_mut_refs)]

pub unsafe fn init(
    _text_start: usize,
    _image_end: usize,
    dtb: usize,
    dtb_end: usize,
    bootinfo: usize,
    bootinfo_end: usize,
) {
    let _ = (dtb, dtb_end, bootinfo, bootinfo_end);
    crate::coherr!("mmu:skipping_ttbr_setup dtb={:#x} bootinfo={:#x}", dtb, bootinfo);
    // The seL4 rootserver maps the UART frame lazily. Once the boot path reaches
    // the MMU initialisation stage we can safely toggle MMIO access for the UART
    // driver so buffered logging resumes on the hardware console.
    crate::drivers::uart::enable_mmio();
}

#[cfg(test)]
mod tests {
    use super::init;

    #[test]
    fn init_is_noop_under_sel4() {
        unsafe {
            init(0, 0, 0, 0, 0, 0);
        }
    }

    #[test]
    fn init_enables_uart_mmio() {
        crate::drivers::uart::test_reset_mmio_state();
        unsafe {
            init(0, 0, 0, 0, 0, 0);
        }
        assert!(crate::drivers::uart::test_mmio_enabled());
    }
}
