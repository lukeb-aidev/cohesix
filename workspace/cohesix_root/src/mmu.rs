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
}
