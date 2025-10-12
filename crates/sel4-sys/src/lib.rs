#![cfg_attr(target_os = "none", no_std)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

#[cfg(target_os = "none")]
mod imp {
    use core::arch::asm;
    use core::ptr;

    pub type seL4_Word = usize;
    pub type seL4_CPtr = seL4_Word;
    pub type seL4_Uint8 = u8;
    pub type seL4_Uint16 = u16;
    pub type seL4_Uint32 = u32;
    pub type seL4_Uint64 = u64;
    pub type seL4_Bool = u8;
    pub type seL4_Error = isize;

    pub const seL4_NoError: seL4_Error = 0;

    pub const seL4_MessageRegisterCount: usize = 4;

    /// Maximum number of bootinfo untyped caps for the configured kernel.
    /// The value is inferred from CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS in the seL4 build.
    pub const MAX_BOOTINFO_UNTYPEDS: usize = 230;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_IPCBuffer {
        pub tag: seL4_MessageInfo,
        pub msg: [seL4_Word; 64],
        pub user_data: seL4_Word,
        pub caps_or_badges: [seL4_Word; 64],
        pub receive_cnode: seL4_CPtr,
        pub receive_index: seL4_CPtr,
        pub receive_depth: seL4_Word,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_MessageInfo {
        pub words: [seL4_Word; 1],
    }

    impl seL4_MessageInfo {
        #[inline(always)]
        pub const fn new(
            label: seL4_Word,
            caps_unwrapped: seL4_Word,
            extra_caps: seL4_Word,
            length: seL4_Word,
        ) -> Self {
            let mut value = 0usize;
            value |= (label & 0x0fff_ffff_ffff_ffff) << 12;
            value |= (caps_unwrapped & 0x7) << 9;
            value |= (extra_caps & 0x3) << 7;
            value |= length & 0x7f;
            Self { words: [value] }
        }

        #[inline(always)]
        pub const fn label(self) -> seL4_Word {
            (self.words[0] >> 12) & 0x0fff_ffff_ffff_ffff
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            (self.words[0] >> 7) & 0x3
        }

        #[inline(always)]
        pub const fn length(self) -> seL4_Word {
            self.words[0] & 0x7f
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_CapRights {
        words: [seL4_Word; 1],
    }

    impl seL4_CapRights {
        #[inline(always)]
        pub const fn new(
            grant_reply: seL4_Word,
            grant: seL4_Word,
            read: seL4_Word,
            write: seL4_Word,
        ) -> Self {
            let mut value = 0usize;
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

    pub const seL4_CapRights_ReadWrite: seL4_CapRights = seL4_CapRights::new(0, 0, 1, 1);
    pub const seL4_CapRights_All: seL4_CapRights = seL4_CapRights::new(1, 1, 1, 1);

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_CNode_CapData {
        words: [seL4_Word; 1],
    }

    impl seL4_CNode_CapData {
        #[inline(always)]
        pub const fn new(guard: seL4_Word, guard_size: seL4_Word) -> Self {
            let mut value = 0usize;
            value |= (guard & 0x3fff_ffff_ffff_ffff) << 6;
            value |= guard_size & 0x3f;
            Self { words: [value] }
        }
    }

    pub type seL4_Untyped = seL4_CPtr;
    pub type seL4_CNode = seL4_CPtr;
    pub type seL4_VSpace = seL4_CPtr;
    pub type seL4_ARM_Page = seL4_CPtr;
    pub type seL4_ARM_PageTable = seL4_CPtr;

    pub const seL4_UntypedObject: seL4_Word = 0;
    pub const seL4_TCBObject: seL4_Word = 1;
    pub const seL4_EndpointObject: seL4_Word = 2;
    pub const seL4_NotificationObject: seL4_Word = 3;
    pub const seL4_CapTableObject: seL4_Word = 4;
    pub const seL4_ARM_SmallPageObject: seL4_Word = 6;
    pub const seL4_ARM_LargePageObject: seL4_Word = 7;
    pub const seL4_ARM_PageTableObject: seL4_Word = 8;

    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x03);
    pub const seL4_FailedLookup: seL4_Error = 6;
    pub const seL4_NotEnoughMemory: seL4_Error = 10;

    pub const seL4_CapNull: seL4_CPtr = 0;
    pub const seL4_CapInitThreadTCB: seL4_CPtr = 1;
    pub const seL4_CapInitThreadCNode: seL4_CPtr = 2;
    pub const seL4_CapInitThreadVSpace: seL4_CPtr = 3;
    pub const seL4_CapIRQControl: seL4_CPtr = 4;
    pub const seL4_CapASIDControl: seL4_CPtr = 5;
    pub const seL4_CapInitThreadASIDPool: seL4_CPtr = 6;
    pub const seL4_CapIOPortControl: seL4_CPtr = 7;
    pub const seL4_CapIOSpace: seL4_CPtr = 8;
    pub const seL4_CapBootInfoFrame: seL4_CPtr = 9;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr = 10;

    #[repr(transparent)]
    #[derive(Clone, Copy)]
    pub struct seL4_ARM_VMAttributes(pub seL4_Word);

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
        pub sizeBits: seL4_Uint8,
        pub isDevice: seL4_Uint8,
        pub padding: [seL4_Uint8; core::mem::size_of::<seL4_Word>() - 2],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_BootInfo {
        pub extraLen: seL4_Word,
        pub nodeId: seL4_Word,
        pub numNodes: seL4_Word,
        pub numIOPTLevels: seL4_Word,
        pub ipcBuffer: seL4_Word,
        pub empty: seL4_SlotRegion,
        pub sharedFrames: seL4_SlotRegion,
        pub userImageFrames: seL4_SlotRegion,
        pub userImagePaging: seL4_SlotRegion,
        pub ioSpaceCaps: seL4_SlotRegion,
        pub extraBIPages: seL4_SlotRegion,
        pub initThreadCNodeSizeBits: seL4_Word,
        pub initThreadDomain: seL4_Word,
        pub untyped: seL4_SlotRegion,
        pub untypedList: [seL4_UntypedDesc; MAX_BOOTINFO_UNTYPEDS],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_ARM_Page_GetAddress {
        pub error: seL4_Error,
        pub paddr: seL4_Word,
    }

    extern "C" {
        static mut __sel4_ipc_buffer: *mut seL4_IPCBuffer;
    }

    #[inline(always)]
    unsafe fn ipc_buffer() -> *mut seL4_IPCBuffer {
        __sel4_ipc_buffer
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(index: usize) -> seL4_Word {
        (*ipc_buffer()).msg[index]
    }

    #[inline(always)]
    pub unsafe fn seL4_SetMR(index: usize, value: seL4_Word) {
        (*ipc_buffer()).msg[index] = value;
    }

    #[inline(always)]
    pub unsafe fn seL4_SetCap(slot: usize, cptr: seL4_CPtr) {
        (*ipc_buffer()).caps_or_badges[slot] = cptr;
    }

    #[inline(always)]
    pub unsafe fn seL4_GetCap(slot: usize) -> seL4_CPtr {
        (*ipc_buffer()).caps_or_badges[slot]
    }

    #[inline(always)]
    fn read_mut(ptr: *mut seL4_Word) -> seL4_Word {
        if ptr.is_null() {
            0
        } else {
            unsafe { *ptr }
        }
    }

    #[inline(always)]
    fn write_mut(ptr: *mut seL4_Word, value: seL4_Word) {
        if !ptr.is_null() {
            unsafe {
                *ptr = value;
            }
        }
    }

    unsafe fn arm_sys_send_recv(
        sys: seL4_Word,
        dest_in: seL4_Word,
        out_badge: *mut seL4_Word,
        info_arg: seL4_Word,
        out_info: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
        reply: seL4_Word,
    ) {
        let mut dest = dest_in;
        let mut info = info_arg;
        let mut msg0 = read_mut(mr0);
        let mut msg1 = read_mut(mr1);
        let mut msg2 = read_mut(mr2);
        let mut msg3 = read_mut(mr3);

        asm!(
            "svc #0",
            inout("x0") dest,
            inout("x1") info,
            inout("x2") msg0,
            inout("x3") msg1,
            inout("x4") msg2,
            inout("x5") msg3,
            in("x6") reply,
            in("x7") sys,
            options(nostack, preserves_flags)
        );

        write_mut(out_badge, dest);
        write_mut(out_info, info);
        write_mut(mr0, msg0);
        write_mut(mr1, msg1);
        write_mut(mr2, msg2);
        write_mut(mr3, msg3);
    }

    #[inline(always)]
    pub unsafe fn seL4_CallWithMRs(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        let mut info_out = msg_info.words[0];
        let mut dummy_badge = 0usize;
        let mut msg0 = if !mr0.is_null() && msg_info.length() > 0 {
            *mr0
        } else {
            0
        };
        let mut msg1 = if !mr1.is_null() && msg_info.length() > 1 {
            *mr1
        } else {
            0
        };
        let mut msg2 = if !mr2.is_null() && msg_info.length() > 2 {
            *mr2
        } else {
            0
        };
        let mut msg3 = if !mr3.is_null() && msg_info.length() > 3 {
            *mr3
        } else {
            0
        };

        arm_sys_send_recv(
            seL4_SysCall,
            dest,
            &mut dummy_badge,
            msg_info.words[0],
            &mut info_out,
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        if !mr0.is_null() {
            *mr0 = msg0;
        }
        if !mr1.is_null() {
            *mr1 = msg1;
        }
        if !mr2.is_null() {
            *mr2 = msg2;
        }
        if !mr3.is_null() {
            *mr3 = msg3;
        }

        seL4_MessageInfo { words: [info_out] }
    }

    pub const seL4_SysCall: seL4_Word = !0usize; // -1 in two's complement
    pub const seL4_SysReplyRecv: seL4_Word = !1usize; // -2

    /// seL4_Untyped_Retype syscall.
    #[inline(always)]
    pub unsafe fn seL4_Untyped_Retype(
        service: seL4_Untyped,
        objtype: seL4_Word,
        size_bits: seL4_Word,
        root: seL4_CNode,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num_objects: seL4_Word,
    ) -> seL4_Error {
        let label_untyped_retype: seL4_Word = 1;
        let msg = seL4_MessageInfo::new(label_untyped_retype, 0, 1, 6);
        let mut mr0 = objtype;
        let mut mr1 = size_bits;
        let mut mr2 = node_index;
        let mut mr3 = node_depth;

        seL4_SetCap(0, root);
        seL4_SetMR(4, node_offset);
        seL4_SetMR(5, num_objects);

        let info = seL4_CallWithMRs(service, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Map(
        service: seL4_ARM_PageTable,
        vspace: seL4_CPtr,
        vaddr: seL4_Word,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        let label: seL4_Word = 9;
        let msg = seL4_MessageInfo::new(label, 0, 1, 2);
        let mut mr0 = vaddr;
        let mut mr1 = attr.0;

        seL4_SetCap(0, vspace);

        let info = seL4_CallWithMRs(
            service,
            msg,
            &mut mr0,
            &mut mr1,
            ptr::null_mut(),
            ptr::null_mut(),
        );
        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Map(
        service: seL4_ARM_Page,
        vspace: seL4_CPtr,
        vaddr: seL4_Word,
        rights: seL4_CapRights,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        let label: seL4_Word = 10;
        let msg = seL4_MessageInfo::new(label, 0, 1, 3);
        let mut mr0 = vaddr;
        let mut mr1 = rights.raw();
        let mut mr2 = attr.0;

        seL4_SetCap(0, vspace);

        let info = seL4_CallWithMRs(service, msg, &mut mr0, &mut mr1, &mut mr2, ptr::null_mut());

        info.label() as seL4_Error
    }

    extern "C" {
        pub fn seL4_DebugPutChar(c: u8);
        pub fn seL4_Yield();
        pub fn seL4_ARM_Page_Unmap(service: seL4_ARM_Page) -> seL4_Error;
        pub fn seL4_CNode_Delete(
            root: seL4_CNode,
            index: seL4_CPtr,
            depth: seL4_Word,
        ) -> seL4_Error;
        pub fn seL4_ARM_Page_GetAddress(service: seL4_ARM_Page) -> seL4_ARM_Page_GetAddress;
    }

    pub use seL4_ARM_Page_GetAddress as ARMPageGetAddressResult;
    pub use seL4_BootInfo as BootInfo;
    pub use seL4_BootInfoHeader as BootInfoHeader;
    pub use seL4_SlotRegion as SlotRegion;
    pub use seL4_UntypedDesc as UntypedDesc;
}

#[cfg(target_os = "none")]
pub use imp::*;

#[cfg(not(target_os = "none"))]
mod host_stub {
    use core::mem::size_of;

    #[inline(always)]
    fn unsupported() -> ! {
        panic!("sel4-sys stubs must not be used on host targets");
    }

    pub type seL4_Word = usize;
    pub type seL4_CPtr = usize;
    pub type seL4_Error = isize;
    pub type seL4_CNode = seL4_CPtr;
    pub type seL4_Untyped = seL4_CPtr;
    pub type seL4_VSpace = seL4_CPtr;
    pub type seL4_ARM_Page = seL4_CPtr;
    pub type seL4_ARM_PageTable = seL4_CPtr;
    pub type seL4_CapRights = usize;

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
        pub const fn length(self) -> seL4_Word {
            0
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            0
        }
    }

    #[derive(Clone, Copy)]
    pub struct seL4_CNode_CapData;

    #[derive(Clone, Copy)]
    pub struct seL4_ARM_VMAttributes(pub seL4_Word);

    pub type seL4_CapData_t = seL4_CNode_CapData;

    #[derive(Clone, Copy)]
    pub struct BootInfo;

    #[derive(Clone, Copy)]
    pub struct BootInfoHeader {
        pub id: seL4_Word,
        pub len: seL4_Word,
    }

    #[derive(Clone, Copy)]
    pub struct SlotRegion {
        pub start: seL4_CPtr,
        pub end: seL4_CPtr,
    }

    #[derive(Clone, Copy)]
    pub struct UntypedDesc {
        pub paddr: seL4_Word,
        pub sizeBits: u8,
        pub isDevice: u8,
        pub padding: [u8; size_of::<seL4_Word>() - 2],
    }

    #[derive(Clone, Copy)]
    pub struct ARMPageGetAddress {
        pub error: seL4_Error,
        pub paddr: seL4_Word,
    }

    pub const seL4_NoError: seL4_Error = 0;
    pub const MAX_BOOTINFO_UNTYPEDS: usize = 0;
    pub const seL4_MessageRegisterCount: usize = 4;

    pub const seL4_CapNull: seL4_CPtr = 0;
    pub const seL4_CapInitThreadTCB: seL4_CPtr = 1;
    pub const seL4_CapInitThreadCNode: seL4_CPtr = 2;
    pub const seL4_CapInitThreadVSpace: seL4_CPtr = 3;
    pub const seL4_CapIRQControl: seL4_CPtr = 4;
    pub const seL4_CapASIDControl: seL4_CPtr = 5;
    pub const seL4_CapInitThreadASIDPool: seL4_CPtr = 6;
    pub const seL4_CapIOPortControl: seL4_CPtr = 7;
    pub const seL4_CapIOSpace: seL4_CPtr = 8;
    pub const seL4_CapBootInfoFrame: seL4_CPtr = 9;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr = 10;

    pub const seL4_CapRights_All: seL4_CapRights = 0;
    pub const seL4_CapRights_ReadWrite: seL4_CapRights = 0;

    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_SmallPageObject: seL4_Word = 0;
    pub const seL4_ARM_PageTableObject: seL4_Word = 0;
    pub const seL4_FailedLookup: seL4_Error = 6;
    pub const seL4_NotEnoughMemory: seL4_Error = 10;

    #[inline(always)]
    pub unsafe fn seL4_SetMR(_index: usize, _value: seL4_Word) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(_index: usize) -> seL4_Word {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_SetCap(_slot: usize, _cptr: seL4_CPtr) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetCap(_slot: usize) -> seL4_CPtr {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CallWithMRs(
        _dest: seL4_CPtr,
        _msg_info: seL4_MessageInfo,
        _mr0: *mut seL4_Word,
        _mr1: *mut seL4_Word,
        _mr2: *mut seL4_Word,
        _mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Map(
        _service: seL4_ARM_Page,
        _vspace: seL4_CPtr,
        _vaddr: seL4_Word,
        _rights: seL4_CapRights,
        _attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Map(
        _service: seL4_ARM_PageTable,
        _vspace: seL4_CPtr,
        _vaddr: seL4_Word,
        _attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Unmap(_service: seL4_ARM_Page) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_GetAddress(_service: seL4_ARM_Page) -> ARMPageGetAddress {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_Untyped_Retype(
        _service: seL4_Untyped,
        _objtype: seL4_Word,
        _size_bits: seL4_Word,
        _root: seL4_CNode,
        _node_index: seL4_Word,
        _node_depth: seL4_Word,
        _node_offset: seL4_Word,
        _num_objects: seL4_Word,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub fn seL4_DebugPutChar(_c: u8) {
        unsupported();
    }

    #[inline(always)]
    pub fn seL4_Yield() {
        unsupported();
    }
}

#[cfg(not(target_os = "none"))]
pub use host_stub::*;
