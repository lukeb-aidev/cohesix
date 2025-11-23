// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

#[cfg(target_os = "none")]
mod imp {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

    pub const MAX_BOOTINFO_UNTYPEDS: usize = CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS as usize;

    #[repr(C, align(16))]
    pub struct TlsImage {
        ipc_buffer: *mut seL4_IPCBuffer,
    }

    impl TlsImage {
        pub const fn new() -> Self {
            Self {
                ipc_buffer: core::ptr::null_mut(),
            }
        }

        #[inline(always)]
        pub fn ipc_buffer(&self) -> *mut seL4_IPCBuffer {
            self.ipc_buffer
        }

        #[inline(always)]
        pub unsafe fn set_ipc_buffer(&mut self, ptr: *mut seL4_IPCBuffer) {
            self.ipc_buffer = ptr;
        }
    }

    #[inline(always)]
    unsafe fn tls_base_ptr() -> *mut TlsImage {
        extern "C" {
            static mut __tls_base: usize;
        }

        let ptr = core::ptr::addr_of_mut!(__tls_base);
        let mut base: usize;
        core::arch::asm!("ldr {}, [{ptr}]", out(reg) base, ptr = in(reg) ptr, options(nostack));
        base as *mut TlsImage
    }

    pub unsafe fn tls_set_base(ptr: *mut TlsImage) {
        extern "C" {
            static mut __tls_base: usize;
        }

        let addr = ptr as usize;
        let dest_ptr = core::ptr::addr_of_mut!(__tls_base);
        core::arch::asm!("str {value}, [{dst}]", value = in(reg) addr, dst = in(reg) dest_ptr, options(nostack));
    }

    pub unsafe fn tls_image_mut() -> Option<&'static mut TlsImage> {
        let base = tls_base_ptr();
        if base.is_null() {
            return None;
        }

        Some(&mut *base)
    }
}

#[cfg(target_os = "none")]
pub use imp::*;

#[cfg(not(target_os = "none"))]
mod imp {
    use core::mem::size_of;
    use core::ptr;

    #[inline(always)]
    fn unsupported() -> ! {
        panic!("sel4-sys stubs must not be used on host targets");
    }

    pub const MAX_BOOTINFO_UNTYPEDS: usize = 0;

    pub type seL4_Word = u64;
    #[allow(clippy::manual_bits)]
    pub const seL4_WordBits: seL4_Word = (size_of::<seL4_Word>() * 8) as seL4_Word;
    pub const seL4_PageBits: seL4_Word = 12;
    pub type seL4_CPtr = u64;
    pub type seL4_Error = i32;
    pub type seL4_CNode = seL4_CPtr;
    pub type seL4_TCB = seL4_CPtr;
    pub type seL4_Untyped = seL4_CPtr;
    pub type seL4_VSpace = seL4_CPtr;
    pub type seL4_ARM_Page = seL4_CPtr;
    pub type seL4_ARM_PageTable = seL4_CPtr;

    pub const seL4_CapNull: seL4_CPtr = 0;
    pub const seL4_CapInitThreadTCB: seL4_CPtr = 1;
    pub const seL4_CapInitThreadCNode: seL4_CPtr = 2;
    pub const seL4_CapInitThreadVSpace: seL4_CPtr = 3;
    pub const seL4_CapIRQControl: seL4_CPtr = 4;
    pub const seL4_CapASIDControl: seL4_CPtr = 5;
    pub const seL4_CapInitThreadASIDPool: seL4_CPtr = 6;
    pub const seL4_CapIOPortControl: seL4_CPtr = 7;
    pub const seL4_CapIOPort: seL4_CPtr = seL4_CapIOPortControl;
    pub const seL4_CapIOSpace: seL4_CPtr = 8;
    pub const seL4_CapBootInfoFrame: seL4_CPtr = 9;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr = 10;
    pub const seL4_CapDomain: seL4_CPtr = 11;
    pub const seL4_CapSMMUSIDControl: seL4_CPtr = 12;
    pub const seL4_CapSMMUCBControl: seL4_CPtr = 13;
    pub const seL4_CapInitThreadSC: seL4_CPtr = 14;
    pub const seL4_CapSMC: seL4_CPtr = 15;

    pub const seL4_UntypedObject: seL4_Word = 0;
    pub const seL4_TCBObject: seL4_Word = 1;
    pub const seL4_EndpointObject: seL4_Word = 2;
    pub const seL4_NotificationObject: seL4_Word = 3;
    pub const seL4_CapTableObject: seL4_Word = 4;
    pub const seL4_ARM_SmallPageObject: seL4_Word = 6;
    pub const seL4_ARM_LargePageObject: seL4_Word = 7;
    pub const seL4_ARM_PageTableObject: seL4_Word = 8;
    pub const seL4_EndpointBits: seL4_Word = 4;
    pub const seL4_NotificationBits: seL4_Word = 4;

    #[repr(usize)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum seL4_ObjectType {
        seL4_UntypedObject = seL4_UntypedObject as usize,
        seL4_TCBObject = seL4_TCBObject as usize,
        seL4_EndpointObject = seL4_EndpointObject as usize,
        seL4_NotificationObject = seL4_NotificationObject as usize,
        seL4_CapTableObject = seL4_CapTableObject as usize,
        seL4_ARM_Page = seL4_ARM_SmallPageObject as usize,
        seL4_ARM_LargePage = seL4_ARM_LargePageObject as usize,
        seL4_ARM_PageTableObject = seL4_ARM_PageTableObject as usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_CapRights_t {
        pub words: [seL4_Word; 1],
    }

    impl seL4_CapRights_t {
        #[inline(always)]
        pub const fn new(
            grant_reply: seL4_Word,
            grant: seL4_Word,
            read: seL4_Word,
            write: seL4_Word,
        ) -> Self {
            let mut value: seL4_Word = 0;
            value |= (grant_reply & 0x1) << 3;
            value |= (grant & 0x1) << 2;
            value |= (read & 0x1) << 1;
            value |= write & 0x1;
            Self { words: [value] }
        }

        #[inline(always)]
        pub const fn raw(self) -> seL4_Word {
            self.words[0]
        }
    }

    pub type seL4_CapRights = seL4_CapRights_t;
    pub type seL4_Uint8 = u8;
    pub type seL4_Uint32 = u32;

    #[derive(Clone, Copy)]
    pub struct seL4_MessageInfo {
        pub words: [seL4_Word; 1],
    }

    impl seL4_MessageInfo {
        #[inline(always)]
        pub const fn new(
            label: seL4_Word,
            _caps_unwrapped: seL4_Word,
            _extra_caps: seL4_Word,
            _length: seL4_Word,
        ) -> Self {
            Self { words: [label] }
        }

        #[inline(always)]
        pub const fn label(self) -> seL4_Word {
            self.words[0]
        }

        #[inline(always)]
        pub const fn get_label(self) -> seL4_Word {
            self.label()
        }

        #[inline(always)]
        pub const fn caps_unwrapped(self) -> seL4_Word {
            0
        }

        #[inline(always)]
        pub const fn get_capsUnwrapped(self) -> seL4_Word {
            self.caps_unwrapped()
        }

        #[inline(always)]
        pub const fn length(self) -> seL4_Word {
            0
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            0
        }

        #[inline(always)]
        pub const fn get_length(self) -> seL4_Word {
            self.length()
        }
    }

    #[derive(Clone, Copy)]
    pub struct seL4_CNode_CapData;

    #[derive(Clone, Copy)]
    pub struct seL4_IPCBuffer;

    #[derive(Clone, Copy)]
    pub struct seL4_ARM_VMAttributes(pub seL4_Word);

    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x03);

    pub type seL4_CapData_t = seL4_CNode_CapData;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_BootInfo {
        pub extraLen: seL4_Word,
        pub nodeId: seL4_Word,
        pub numNodes: seL4_Word,
        pub numIOPTLevels: seL4_Word,
        pub ipcBuffer: *mut seL4_IPCBuffer,
        pub empty: seL4_SlotRegion,
        pub sharedFrames: seL4_SlotRegion,
        pub userImageFrames: seL4_SlotRegion,
        pub userImagePaging: seL4_SlotRegion,
        pub ioSpaceCaps: seL4_SlotRegion,
        pub extraBIPages: seL4_SlotRegion,
        pub initThreadCNodeSizeBits: u8,
        pub _padding_init_cnode_bits: [u8; size_of::<seL4_Word>() - 1],
        pub initThreadDomain: seL4_Word,
        pub untyped: seL4_SlotRegion,
        pub untypedList: [seL4_UntypedDesc; MAX_BOOTINFO_UNTYPEDS],
    }

    #[inline(always)]
    fn unsupported_error() -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetBootInfo() -> *const seL4_BootInfo {
        ptr::null()
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Copy(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Uint8,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Uint8,
        _rights: seL4_CapRights_t,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Delete(
        _root: seL4_CNode,
        _index: seL4_Word,
        _depth: seL4_Uint8,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Move(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Uint8,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Uint8,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Mint(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Uint8,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Uint8,
        _rights: seL4_CapRights_t,
        _badge: seL4_Word,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_Untyped_Retype(
        _ut_cap: seL4_Untyped,
        _obj_type: seL4_Word,
        _size_bits: seL4_Word,
        _root: seL4_CNode,
        _node_index: seL4_Word,
        _node_depth: seL4_Word,
        _node_offset: seL4_Word,
        _num: seL4_Word,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_BootInfoHeader {
        pub id: seL4_Word,
        pub len: seL4_Word,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_SlotRegion {
        pub start: seL4_CPtr,
        pub end: seL4_CPtr,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_UntypedDesc {
        pub paddr: seL4_Word,
        pub sizeBits: u8,
        pub isDevice: u8,
        pub padding: [u8; size_of::<seL4_Word>() - 2],
    }

    #[derive(Clone, Copy)]
    pub struct seL4_ARM_Page_GetAddress {
        pub error: seL4_Error,
        pub paddr: seL4_Word,
    }

    pub type BootInfo = seL4_BootInfo;
    pub type BootInfoHeader = seL4_BootInfoHeader;
    pub type SlotRegion = seL4_SlotRegion;
    pub type UntypedDesc = seL4_UntypedDesc;
    pub type ARMPageGetAddress = seL4_ARM_Page_GetAddress;

    pub const seL4_NoError: seL4_Error = 0;
    pub const seL4_InvalidArgument: seL4_Error = 1;
    pub const seL4_InvalidCapability: seL4_Error = 2;
    pub const seL4_IllegalOperation: seL4_Error = 3;
    pub const seL4_RangeError: seL4_Error = 4;
    pub const seL4_AlignmentError: seL4_Error = 5;
    pub const seL4_TruncatedMessage: seL4_Error = 7;
    pub const seL4_DeleteFirst: seL4_Error = 8;
    pub const seL4_RevokeFirst: seL4_Error = 9;
    pub const seL4_FailedLookup: seL4_Error = 6;
    pub const seL4_NotEnoughMemory: seL4_Error = 10;

    #[inline(always)]
    pub fn seL4_CapRights_to_word(rights: seL4_CapRights) -> seL4_CapRights_t {
        rights
    }

    pub const seL4_CapRights_ReadWrite: seL4_CapRights_t = seL4_CapRights_t::new(0, 0, 1, 1);
    pub const seL4_CapRights_All: seL4_CapRights_t = seL4_CapRights_t::new(1, 1, 1, 1);
    pub const seL4_AllRights: seL4_Word = seL4_CapRights_All.raw();

    #[inline(always)]
    pub fn seL4_DebugPutChar(_c: u8) {
        unsupported();
    }

    #[inline(always)]
    pub fn seL4_DebugCapIdentify(_cap: seL4_CPtr) -> seL4_Uint32 {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CapIdentify(_cap: seL4_CPtr) -> seL4_Word {
        unsupported();
    }

    #[inline(always)]
    pub fn seL4_Yield() {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_Send(_dest: seL4_CPtr, _msg: seL4_MessageInfo) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_Recv(_src: seL4_CPtr, _sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_Poll(_src: seL4_CPtr, _sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CallWithMRs(
        _dest: seL4_CPtr,
        _msg: seL4_MessageInfo,
        _mr0: *mut seL4_Word,
        _mr1: *mut seL4_Word,
        _mr2: *mut seL4_Word,
        _mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_SetMR(_index: seL4_Word, _value: seL4_Word) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(_index: seL4_Word) -> seL4_Word {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetFaultHandler(
        _tcb: seL4_TCB,
        _fault_handler: seL4_CPtr,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        _tcb: seL4_TCB,
        _buffer_addr: seL4_Word,
        _buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub fn seL4_SetIPCBuffer(_buf: *mut seL4_IPCBuffer) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetIPCBuffer() -> *mut seL4_IPCBuffer {
        ptr::null_mut()
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Map(
        _page: seL4_ARM_Page,
        _vspace: seL4_VSpace,
        _vaddr: seL4_Word,
        _rights: seL4_CapRights_t,
        _attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Map(
        _pt: seL4_ARM_PageTable,
        _vspace: seL4_VSpace,
        _vaddr: seL4_Word,
        _attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        unsupported_error()
    }

    #[inline(always)]
    pub fn yield_now() {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_DebugHalt() {
        unsupported();
    }
}

#[cfg(not(target_os = "none"))]
pub use imp::*;
