// CLASSIFICATION: COMMUNITY
// Filename: bootinfo.rs v0.3
// Author: Lukas Bower
// Date Modified: 2028-08-30

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

const EMPTY_UNTYPED: UntypedDesc = UntypedDesc {
    paddr: 0,
    size_bits: 0,
    is_device: 0,
    padding: [0; core::mem::size_of::<usize>() - 2],
};

#[link_section = ".bss.bootinfo"]
#[no_mangle]
pub static mut BOOTINFO: BootInfo = BootInfo {
    extra_len: 0,
    node_id: 0,
    num_nodes: 0,
    num_io_pt_levels: 0,
    ipc_buffer: 0,
    empty: SlotRegion { start: 0, end: 0 },
    shared_frames: SlotRegion { start: 0, end: 0 },
    user_image_frames: SlotRegion { start: 0, end: 0 },
    user_image_paging: SlotRegion { start: 0, end: 0 },
    io_space_caps: SlotRegion { start: 0, end: 0 },
    extra_bi_pages: SlotRegion { start: 0, end: 0 },
    init_thread_cnode_size_bits: 0,
    init_thread_domain: 0,
    untyped: SlotRegion { start: 0, end: 0 },
    untyped_list: [EMPTY_UNTYPED; MAX_BOOTINFO_UNTYPED_CAPS],
};

#[no_mangle]
pub unsafe extern "C" fn copy_bootinfo(ptr: *const BootInfo) {
    BOOTINFO = ptr.read();
}

pub unsafe fn bootinfo() -> &'static BootInfo {
    &BOOTINFO
}

#[no_mangle]
pub extern "C" fn seL4_GetBootInfo(_: u32) -> *const BootInfo {
    unsafe { &BOOTINFO as *const _ }
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
