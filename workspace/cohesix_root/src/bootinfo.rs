// CLASSIFICATION: COMMUNITY
// Filename: bootinfo.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-02-15

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SlotRegion {
    pub start: usize,
    pub end: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UntypedDesc {
    pub paddr: usize,
    pub size_bits: u8,
    pub is_device: u8,
    pub padding: [u8; core::mem::size_of::<usize>() - 2],
}

pub const MAX_BOOTINFO_UNTYPED_CAPS: usize = 256;

#[repr(C)]
pub struct BootInfo {
    pub extra_len: usize,
    pub node_id: usize,
    pub num_nodes: usize,
    pub num_io_pt_levels: usize,
    pub ipc_buffer: usize,
    pub empty: SlotRegion,
    pub shared_frames: SlotRegion,
    pub user_image_frames: SlotRegion,
    pub user_image_paging: SlotRegion,
    pub io_space_caps: SlotRegion,
    pub extra_bi_pages: SlotRegion,
    pub init_thread_cnode_size_bits: usize,
    pub init_thread_domain: usize,
    pub untyped: SlotRegion,
    pub untyped_list: [UntypedDesc; MAX_BOOTINFO_UNTYPED_CAPS],
}

#[link_section = ".bss"]
#[no_mangle]
pub static mut BOOTINFO_PTR: *const BootInfo = core::ptr::null();

#[no_mangle]
pub unsafe extern "C" fn set_bootinfo_ptr(ptr: *const BootInfo) {
    BOOTINFO_PTR = ptr;
}

#[no_mangle]
pub unsafe extern "C" fn get_bootinfo_ptr() -> *const BootInfo {
    BOOTINFO_PTR
}

pub unsafe fn bootinfo() -> &'static BootInfo {
    &*BOOTINFO_PTR
}

extern "C" {
    pub fn seL4_GetBootInfo() -> *const BootInfo;
}

pub unsafe fn dump_bootinfo() {
    let bi = bootinfo();
    crate::coherr!(
        "bootinfo node_id={} untyped={} first_ut_size={}",
        bi.node_id,
        bi.untyped.end - bi.untyped.start,
        bi.untyped_list[0].size_bits
    );
}
