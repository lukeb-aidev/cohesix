// CLASSIFICATION: COMMUNITY
// Filename: bootinfo.rs v0.3
// Author: Lukas Bower
// Date Modified: 2028-08-31
#![allow(static_mut_refs)]

include!(concat!(env!("OUT_DIR"), "/sel4_config.rs"));

pub const MAX_BOOTINFO_UNTYPED_CAPS: usize = CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS;

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

/// Size of the bootinfo frame in bytes.
pub const BOOTINFO_FRAME_SIZE: usize = 1 << 12;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootInfoHeader {
    pub id: usize,
    pub len: usize,
}

pub const SEL4_BOOTINFO_HEADER_FDT: usize = 6;

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

const BOOTINFO_SIZE: usize = core::mem::size_of::<BootInfo>();
const _: [u8; BOOTINFO_FRAME_SIZE - BOOTINFO_SIZE] = [0; BOOTINFO_FRAME_SIZE - BOOTINFO_SIZE];

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
    &*core::ptr::addr_of!(BOOTINFO)
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

pub unsafe fn dtb_slice() -> Option<&'static [u8]> {
    let bi_ptr = core::ptr::addr_of!(BOOTINFO) as usize;
    if (*core::ptr::addr_of!(BOOTINFO)).extra_len == 0 {
        return None;
    }
    let mut cur = bi_ptr + BOOTINFO_FRAME_SIZE;
    let end = cur + (*core::ptr::addr_of!(BOOTINFO)).extra_len;
    while cur < end {
        let hdr = &*(cur as *const BootInfoHeader);
        if hdr.id == SEL4_BOOTINFO_HEADER_FDT {
            let data = cur + core::mem::size_of::<BootInfoHeader>();
            let len = hdr.len - core::mem::size_of::<BootInfoHeader>();
            return Some(core::slice::from_raw_parts(data as *const u8, len));
        }
        cur += hdr.len;
    }
    None
}
