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
    use core::arch::asm;
    use core::mem::MaybeUninit;
    use core::ptr;

    pub type seL4_Word = u64;
    #[allow(clippy::manual_bits)]
    pub const seL4_WordBits: seL4_Word = (core::mem::size_of::<seL4_Word>() * 8) as seL4_Word;
    /// Log2 of the base page size baked into the target seL4 kernel configuration.
    ///
    /// Cohesix kernels are built for 4 KiB pages on every supported target, so we
    /// expose the value as a constant for consumers that need to reason about
    /// IPC buffer placement without depending on generated C headers.
    pub const seL4_PageBits: seL4_Word = 12;
    pub type seL4_CPtr = seL4_Word;
    pub type seL4_Uint8 = u8;
    pub type seL4_Uint16 = u16;
    pub type seL4_Uint32 = u32;
    pub type seL4_Uint64 = u64;
    pub type seL4_Bool = u8;
    pub type seL4_Error = i32;

    pub const seL4_NoError: seL4_Error = 0;
    pub const seL4_InvalidArgument: seL4_Error = 1;
    pub const seL4_InvalidCapability: seL4_Error = 2;
    pub const seL4_IllegalOperation: seL4_Error = 3;
    pub const seL4_RangeError: seL4_Error = 4;
    pub const seL4_AlignmentError: seL4_Error = 5;
    pub const seL4_TruncatedMessage: seL4_Error = 7;
    pub const seL4_DeleteFirst: seL4_Error = 8;
    pub const seL4_RevokeFirst: seL4_Error = 9;

    pub const seL4_MessageRegisterCount: usize = 4;

    const SEL4_TCB_SET_IPC_BUFFER: seL4_Word = 9;
    const SEL4_TCB_SET_SPACE: seL4_Word = 10;
    const SEL4_TCB_SUSPEND: seL4_Word = 11;
    const SEL4_CNODE_DELETE: seL4_Word = 18;
    const SEL4_CNODE_COPY: seL4_Word = 20;
    const SEL4_CNODE_MOVE: seL4_Word = 22;
    const SEL4_CNODE_MINT: seL4_Word = 21;

    /// Maximum number of bootinfo untyped caps for the configured kernel.
    /// The value is inferred from CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS in the seL4 build.
    pub const MAX_BOOTINFO_UNTYPEDS: usize = 230;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_IPCBuffer {
        pub tag: seL4_MessageInfo,
        pub msg: [seL4_Word; 64],
        pub userData: seL4_Word,
        pub capsOrBadges: [seL4_Word; 64],
        pub receiveCNode: seL4_CPtr,
        pub receiveIndex: seL4_CPtr,
        pub receiveDepth: seL4_Word,
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
            let mut value: seL4_Word = 0;
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
        pub const fn get_label(self) -> seL4_Word {
            self.label()
        }

        #[inline(always)]
        pub const fn caps_unwrapped(self) -> seL4_Word {
            (self.words[0] >> 9) & 0x7
        }

        #[inline(always)]
        pub const fn get_capsUnwrapped(self) -> seL4_Word {
            self.caps_unwrapped()
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            (self.words[0] >> 7) & 0x3
        }

        #[inline(always)]
        pub const fn length(self) -> seL4_Word {
            self.words[0] & 0x7f
        }

        #[inline(always)]
        pub const fn get_length(self) -> seL4_Word {
            self.length()
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_CapRights_t {
        words: [seL4_Word; 1],
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

    #[inline(always)]
    pub const fn seL4_CapRights_to_word(rights: seL4_CapRights) -> seL4_CapRights_t {
        rights
    }

    pub const seL4_CapRights_ReadWrite: seL4_CapRights_t = seL4_CapRights_t::new(0, 0, 1, 1);
    pub const seL4_CapRights_All: seL4_CapRights_t = seL4_CapRights_t::new(1, 1, 1, 1);
    pub const seL4_AllRights: seL4_Word = seL4_CapRights_All.raw();

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_CNode_CapData {
        words: [seL4_Word; 1],
    }

    impl seL4_CNode_CapData {
        #[inline(always)]
        pub const fn new(guard: seL4_Word, guard_size: seL4_Word) -> Self {
            let mut value: seL4_Word = 0;
            value |= (guard & 0x3fff_ffff_ffff_ffff) << 6;
            value |= guard_size & 0x3f;
            Self { words: [value] }
        }
    }

    pub type seL4_Untyped = seL4_CPtr;
    pub type seL4_CNode = seL4_CPtr;
    pub type seL4_TCB = seL4_CPtr;
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
    /// Alias mirroring the nomenclature used in Table 9.1 of the seL4 reference manual.
    pub const seL4_CapIOPort: seL4_CPtr = seL4_CapIOPortControl;
    pub const seL4_CapIOSpace: seL4_CPtr = 8;
    pub const seL4_CapBootInfoFrame: seL4_CPtr = 9;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr = 10;
    pub const seL4_CapDomain: seL4_CPtr = 11;
    pub const seL4_CapSMMUSIDControl: seL4_CPtr = 12;
    pub const seL4_CapSMMUCBControl: seL4_CPtr = 13;
    pub const seL4_CapInitThreadSC: seL4_CPtr = 14;
    pub const seL4_CapSMC: seL4_CPtr = 15;

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
        pub ipcBuffer: *mut seL4_IPCBuffer,
        pub empty: seL4_SlotRegion,
        pub sharedFrames: seL4_SlotRegion,
        pub userImageFrames: seL4_SlotRegion,
        pub userImagePaging: seL4_SlotRegion,
        pub ioSpaceCaps: seL4_SlotRegion,
        pub extraBIPages: seL4_SlotRegion,
        pub initThreadCNodeSizeBits: seL4_Uint8,
        /// Padding inserted by the kernel ABI to restore native alignment after the
        /// 8-bit `initThreadCNodeSizeBits` field. Keeping this explicit prevents the
        /// following `initThreadDomain` field from aliasing unrelated memory when
        /// the bootinfo layout changes across kernel releases.
        pub _padding_init_cnode_bits: [seL4_Uint8; core::mem::size_of::<seL4_Word>() - 1],
        pub initThreadDomain: seL4_Word,
        #[cfg(sel4_config_kernel_mcs)]
        pub schedcontrol: seL4_SlotRegion,
        pub untyped: seL4_SlotRegion,
        pub untypedList: [seL4_UntypedDesc; MAX_BOOTINFO_UNTYPEDS],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_ARM_Page_GetAddress {
        pub error: seL4_Error,
        pub paddr: seL4_Word,
    }

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

    static mut IPC_BUFFER_FALLBACK: *mut seL4_IPCBuffer = core::ptr::null_mut();

    #[inline(always)]
    unsafe fn tls_base_ptr() -> *mut TlsImage {
        let mut base: usize;
        asm!(
            "mrs {out}, TPIDR_EL0",
            out = out(reg) base,
            options(nostack, preserves_flags)
        );
        base as *mut TlsImage
    }

    #[inline(always)]
    pub unsafe fn tls_set_base(ptr: *mut TlsImage) {
        asm!(
            "msr TPIDR_EL0, {inptr}",
            inptr = in(reg) ptr,
            options(nostack, preserves_flags)
        );
    }

    #[inline(always)]
    pub unsafe fn tls_image_mut() -> Option<&'static mut TlsImage> {
        let base = tls_base_ptr();
        if base.is_null() {
            None
        } else {
            Some(&mut *base)
        }
    }

    #[inline(always)]
    unsafe fn ipc_buffer() -> *mut seL4_IPCBuffer {
        if let Some(image) = tls_image_mut() {
            let ptr = image.ipc_buffer();
            if !ptr.is_null() {
                return ptr;
            }
        }
        IPC_BUFFER_FALLBACK
    }

    #[inline(always)]
    pub unsafe fn seL4_SetIPCBuffer(ptr: *mut seL4_IPCBuffer) {
        IPC_BUFFER_FALLBACK = ptr;
        if let Some(image) = tls_image_mut() {
            image.set_ipc_buffer(ptr);
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_GetIPCBuffer() -> *mut seL4_IPCBuffer {
        ipc_buffer()
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
        (*ipc_buffer()).capsOrBadges[slot] = cptr;
    }

    #[inline(always)]
    pub unsafe fn seL4_GetCap(slot: usize) -> seL4_CPtr {
        (*ipc_buffer()).capsOrBadges[slot]
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
        let mut dummy_badge: seL4_Word = 0;
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

    pub const seL4_SysCall: seL4_Word = !0u64; // -1 in two's complement
    pub const seL4_SysReplyRecv: seL4_Word = !1u64; // -2
    pub const seL4_SysSend: seL4_Word = !2u64; // -3
    pub const seL4_SysYield: seL4_Word = !6u64; // -7
    pub const seL4_SysNBRecv: seL4_Word = !7u64; // -8

    #[inline(always)]
    pub unsafe fn seL4_Poll(dest: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut badge_out: seL4_Word = 0;
        let mut info_out: seL4_Word = 0;

        let mut mr0 = MaybeUninit::<seL4_Word>::new(0);
        let mut mr1 = MaybeUninit::<seL4_Word>::new(0);
        let mut mr2 = MaybeUninit::<seL4_Word>::new(0);
        let mut mr3 = MaybeUninit::<seL4_Word>::new(0);

        arm_sys_send_recv(
            seL4_SysNBRecv,
            dest,
            &mut badge_out,
            0,
            &mut info_out,
            mr0.as_mut_ptr(),
            mr1.as_mut_ptr(),
            mr2.as_mut_ptr(),
            mr3.as_mut_ptr(),
            0,
        );

        if !sender_badge.is_null() {
            *sender_badge = badge_out;
        }

        let info = seL4_MessageInfo { words: [info_out] };
        let length = info.length();

        if length > 0 {
            seL4_SetMR(0, mr0.assume_init());
        }
        if length > 1 {
            seL4_SetMR(1, mr1.assume_init());
        }
        if length > 2 {
            seL4_SetMR(2, mr2.assume_init());
        }
        if length > 3 {
            seL4_SetMR(3, mr3.assume_init());
        }

        info
    }

    #[inline(always)]
    pub unsafe fn seL4_Send(dest: seL4_CPtr, msg_info: seL4_MessageInfo) {
        let length = msg_info.length();

        let mut mr0_val = MaybeUninit::<seL4_Word>::uninit();
        let mut mr1_val = MaybeUninit::<seL4_Word>::uninit();
        let mut mr2_val = MaybeUninit::<seL4_Word>::uninit();
        let mut mr3_val = MaybeUninit::<seL4_Word>::uninit();

        let mut mr0_ptr: *mut seL4_Word = ptr::null_mut();
        let mut mr1_ptr: *mut seL4_Word = ptr::null_mut();
        let mut mr2_ptr: *mut seL4_Word = ptr::null_mut();
        let mut mr3_ptr: *mut seL4_Word = ptr::null_mut();

        if length > 0 {
            mr0_val.write(seL4_GetMR(0));
            mr0_ptr = mr0_val.as_mut_ptr();
        }
        if length > 1 {
            mr1_val.write(seL4_GetMR(1));
            mr1_ptr = mr1_val.as_mut_ptr();
        }
        if length > 2 {
            mr2_val.write(seL4_GetMR(2));
            mr2_ptr = mr2_val.as_mut_ptr();
        }
        if length > 3 {
            mr3_val.write(seL4_GetMR(3));
            mr3_ptr = mr3_val.as_mut_ptr();
        }

        arm_sys_send_recv(
            seL4_SysSend,
            dest,
            ptr::null_mut(),
            msg_info.words[0],
            ptr::null_mut(),
            mr0_ptr,
            mr1_ptr,
            mr2_ptr,
            mr3_ptr,
            0,
        );
    }

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
    pub fn seL4_untyped_retype(
        service: seL4_Untyped,
        objtype: seL4_ObjectType,
        size_bits: u8,
        root: seL4_CNode,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num_objects: seL4_Word,
    ) -> seL4_Error {
        unsafe {
            seL4_Untyped_Retype(
                service,
                objtype as seL4_Word,
                size_bits as seL4_Word,
                root,
                node_index,
                node_depth,
                node_offset,
                num_objects,
            )
        }
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

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        service: seL4_TCB,
        buffer: seL4_Word,
        buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_TCB_SET_IPC_BUFFER, 0, 1, 1);
        let mut mr0 = buffer;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        seL4_SetCap(0, buffer_frame);

        let info = seL4_CallWithMRs(service, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSpace(
        service: seL4_TCB,
        fault_ep: seL4_CPtr,
        cspace_root: seL4_CNode,
        cspace_root_data: seL4_Word,
        vspace_root: seL4_CPtr,
        vspace_root_data: seL4_Word,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_TCB_SET_SPACE, 0, 3, 2);
        let mut mr0 = cspace_root_data;
        let mut mr1 = vspace_root_data;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        seL4_SetCap(0, fault_ep);
        seL4_SetCap(1, cspace_root);
        seL4_SetCap(2, vspace_root);

        let info = seL4_CallWithMRs(service, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetFaultHandler(
        service: seL4_TCB,
        fault_ep: seL4_CPtr,
        cspace_root: seL4_CNode,
        vspace_root: seL4_CPtr,
    ) -> seL4_Error {
        seL4_TCB_SetSpace(service, fault_ep, cspace_root, 0, vspace_root, 0)
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_Suspend(service: seL4_TCB) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_TCB_SUSPEND, 0, 0, 0);
        let mut mr0: seL4_Word = 0;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let info = seL4_CallWithMRs(service, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Delete(
        root: seL4_CNode,
        index: seL4_CPtr,
        depth: seL4_Word,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_CNODE_DELETE, 0, 0, 2);
        let mut mr0 = index;
        let mut mr1 = depth as seL4_Word;

        let info = seL4_CallWithMRs(
            root,
            msg,
            &mut mr0,
            &mut mr1,
            ptr::null_mut(),
            ptr::null_mut(),
        );

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Move(
        dest_root: seL4_CNode,
        dest_index: seL4_CPtr,
        dest_depth: seL4_Uint8,
        src_root: seL4_CNode,
        src_index: seL4_CPtr,
        src_depth: seL4_Uint8,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_CNODE_MOVE, 0, 1, 4);
        let mut mr0 = dest_index;
        let mut mr1 = dest_depth as seL4_Word;
        let mut mr2 = src_index;
        let mut mr3 = src_depth as seL4_Word;

        seL4_SetCap(0, src_root);

        let info = seL4_CallWithMRs(dest_root, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Mint(
        dest_root: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Word,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Word,
        rights: seL4_CapRights_t,
        badge: seL4_Word,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_CNODE_MINT, 0, 1, 6);
        let mut mr0 = dest_index;
        let mut mr1 = (dest_depth & 0xff) as seL4_Word;
        let mut mr2 = src_index;
        let mut mr3 = (src_depth & 0xff) as seL4_Word;

        seL4_SetCap(0, src_root);
        seL4_SetMR(4, rights.raw());
        seL4_SetMR(5, badge);

        let info = seL4_CallWithMRs(dest_root, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Copy(
        dest_root: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Word,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Word,
        rights: seL4_CapRights_t,
    ) -> seL4_Error {
        let msg = seL4_MessageInfo::new(SEL4_CNODE_COPY, 0, 1, 5);
        let mut mr0 = dest_index;
        let mut mr1 = (dest_depth & 0xff) as seL4_Word;
        let mut mr2 = src_index;
        let mut mr3 = (src_depth & 0xff) as seL4_Word;
        seL4_SetCap(0, src_root);
        seL4_SetMR(4, rights.raw());

        let info = seL4_CallWithMRs(dest_root, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as seL4_Error
    }

    #[inline(always)]
    pub unsafe fn seL4_CapIdentify(service: seL4_CPtr) -> seL4_Word {
        seL4_DebugCapIdentify(service)
    }

    extern "C" {
        pub fn seL4_DebugPutChar(c: u8);
        pub fn seL4_DebugHalt();
        pub fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Word;
        pub fn seL4_ARM_Page_Unmap(service: seL4_ARM_Page) -> seL4_Error;
        pub fn seL4_ARM_Page_GetAddress(service: seL4_ARM_Page) -> seL4_ARM_Page_GetAddress;
        pub fn seL4_GetBootInfo() -> *const seL4_BootInfo;
    }

    #[inline(always)]
    pub unsafe fn seL4_Yield() {
        arm_sys_send_recv(
            seL4_SysYield,
            0,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
    }

    /// Yield the current thread to the seL4 scheduler.
    #[inline(always)]
    pub fn yield_now() {
        unsafe {
            seL4_Yield();
        }
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

    pub type seL4_Word = u64;
    #[allow(clippy::manual_bits)]
    pub const seL4_WordBits: seL4_Word = (size_of::<seL4_Word>() * 8) as seL4_Word;
    /// Log2 of the base page size baked into the target seL4 kernel configuration.
    ///
    /// Cohesix kernels are built for 4 KiB pages on every supported target, so we
    /// expose the value as a constant for consumers that need to reason about
    /// IPC buffer placement without depending on generated C headers.
    pub const seL4_PageBits: seL4_Word = 12;
    pub type seL4_CPtr = u64;
    pub type seL4_Error = i32;
    pub type seL4_CNode = seL4_CPtr;
    pub type seL4_TCB = seL4_CPtr;
    pub type seL4_Untyped = seL4_CPtr;
    pub type seL4_VSpace = seL4_CPtr;
    pub type seL4_ARM_Page = seL4_CPtr;
    pub type seL4_ARM_PageTable = seL4_CPtr;
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
    /// Alias mirroring the nomenclature used in Table 9.1 of the seL4 reference manual.
    pub const seL4_CapIOPort: seL4_CPtr = seL4_CapIOPortControl;
    pub const seL4_CapIOSpace: seL4_CPtr = 8;
    pub const seL4_CapBootInfoFrame: seL4_CPtr = 9;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr = 10;
    pub const seL4_CapDomain: seL4_CPtr = 11;
    pub const seL4_CapSMMUSIDControl: seL4_CPtr = 12;
    pub const seL4_CapSMMUCBControl: seL4_CPtr = 13;
    pub const seL4_CapInitThreadSC: seL4_CPtr = 14;
    pub const seL4_CapSMC: seL4_CPtr = 15;

    pub const seL4_CapRights_All: seL4_CapRights_t = seL4_CapRights_t::new(1, 1, 1, 1);
    pub const seL4_CapRights_ReadWrite: seL4_CapRights_t = seL4_CapRights_t::new(0, 0, 1, 1);
    pub const seL4_AllRights: seL4_Word = seL4_CapRights_All.raw();

    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_SmallPageObject: seL4_Word = 0;
    pub const seL4_ARM_PageTableObject: seL4_Word = 0;
    pub const seL4_EndpointBits: seL4_Word = 4;
    pub const seL4_NotificationBits: seL4_Word = 4;

    #[repr(usize)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum seL4_ObjectType {
        seL4_UntypedObject = 0,
        seL4_TCBObject = 1,
        seL4_EndpointObject = 2,
        seL4_NotificationObject = 3,
        seL4_CapTableObject = 4,
        seL4_ARM_Page = 6,
        seL4_ARM_LargePage = 7,
        seL4_ARM_PageTableObject = 8,
    }
    pub const seL4_FailedLookup: seL4_Error = 6;
    pub const seL4_NotEnoughMemory: seL4_Error = 10;

    #[inline(always)]
    pub unsafe fn seL4_SetMR(_index: usize, _value: seL4_Word) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_SetIPCBuffer(_ptr: *mut seL4_IPCBuffer) {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_GetBootInfo() -> *const seL4_BootInfo {
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
    pub unsafe fn seL4_Poll(_dest: seL4_CPtr, _sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        _service: seL4_TCB,
        _buffer: seL4_Word,
        _buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSpace(
        _service: seL4_TCB,
        _fault_ep: seL4_CPtr,
        _cspace_root: seL4_CNode,
        _cspace_root_data: seL4_Word,
        _vspace_root: seL4_CPtr,
        _vspace_root_data: seL4_Word,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetFaultHandler(
        _service: seL4_TCB,
        _fault_ep: seL4_CPtr,
        _cspace_root: seL4_CNode,
        _vspace_root: seL4_CPtr,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_Suspend(_service: seL4_TCB) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Delete(
        _root: seL4_CNode,
        _index: seL4_CPtr,
        _depth: seL4_Word,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Move(
        _dest_root: seL4_CNode,
        _dest_index: seL4_CPtr,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_CPtr,
        _src_depth: seL4_Word,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Mint(
        _dest_root: seL4_CNode,
        _dest_index: seL4_CPtr,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_CPtr,
        _src_depth: seL4_Word,
        _rights: seL4_CapRights_t,
        _badge: seL4_Word,
    ) -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Copy(
        _dest_root: seL4_CNode,
        _dest_index: seL4_CPtr,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_CPtr,
        _src_depth: seL4_Word,
        _rights: seL4_CapRights_t,
    ) -> seL4_Error {
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
    pub unsafe fn seL4_Send(_dest: seL4_CPtr, _msg_info: seL4_MessageInfo) {
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
    pub fn seL4_untyped_retype(
        service: seL4_Untyped,
        objtype: seL4_ObjectType,
        size_bits: u8,
        root: seL4_CNode,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num_objects: seL4_Word,
    ) -> seL4_Error {
        unsafe {
            seL4_Untyped_Retype(
                service,
                objtype as seL4_Word,
                size_bits as seL4_Word,
                root,
                node_index,
                node_depth,
                node_offset,
                num_objects,
            )
        }
    }

    #[inline(always)]
    pub fn seL4_DebugPutChar(_c: u8) {
        unsupported();
    }

    #[inline(always)]
    pub fn seL4_DebugCapIdentify(_cap: seL4_CPtr) -> seL4_Word {
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

    /// Yield the current thread when running under host stubs.
    ///
    /// Host executions do not model kernel scheduling, so invoking this helper
    /// indicates a logic error in the calling code.
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
pub use host_stub::*;
