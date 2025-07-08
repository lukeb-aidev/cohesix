// CLASSIFICATION: COMMUNITY
// Filename: lang_items.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-10-12

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    crate::putstr("[root] panic");
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    crate::putstr("[root] alloc_error");
    loop {
        core::hint::spin_loop();
    }
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}
