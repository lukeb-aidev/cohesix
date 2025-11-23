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

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
    include!(concat!(env!("OUT_DIR"), "/sel4_config_consts.rs"));

    extern "C" {
        pub fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Uint32;

        pub fn seL4_DebugPutChar(c: u8);
    }

    pub type seL4_VSpace = seL4_CPtr;

    #[inline(always)]
    pub unsafe fn seL4_GetIPCBuffer() -> *mut seL4_IPCBuffer {
        tls_image_mut()
            .map(|tls| tls.ipc_buffer())
            .unwrap_or(core::ptr::null_mut())
    }

    #[inline(always)]
    pub unsafe fn seL4_SetIPCBuffer(buffer: *mut seL4_IPCBuffer) {
        if let Some(tls) = tls_image_mut() {
            tls.set_ipc_buffer(buffer);
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_SetCap(index: i32, cptr: seL4_CPtr) {
        (*seL4_GetIPCBuffer()).caps_or_badges[index as usize] = cptr;
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(index: i32) -> seL4_Word {
        (*seL4_GetIPCBuffer()).msg[index as usize]
    }

    #[inline(always)]
    pub unsafe fn seL4_SetMR(index: i32, value: seL4_Word) {
        (*seL4_GetIPCBuffer()).msg[index as usize] = value;
    }

    #[inline(always)]
    pub const fn seL4_MessageInfo_new(
        label: seL4_Word,
        caps_unwrapped: seL4_Word,
        extra_caps: seL4_Word,
        length: seL4_Word,
    ) -> seL4_MessageInfo {
        let word = ((label & 0xfffffffffffff) << 12)
            | ((caps_unwrapped & 0x7) << 9)
            | ((extra_caps & 0x3) << 7)
            | (length & 0x7f);
        seL4_MessageInfo { words: [word] }
    }

    #[inline(always)]
    pub const fn seL4_MessageInfo_get_label(msg_info: seL4_MessageInfo) -> seL4_Word {
        (msg_info.words[0] & 0xfffffffffffff000) >> 12
    }

    #[inline(always)]
    unsafe fn arm_sys_send(
        sys: seL4_Word,
        dest: seL4_Word,
        info_arg: seL4_Word,
        mr0: seL4_Word,
        mr1: seL4_Word,
        mr2: seL4_Word,
        mr3: seL4_Word,
    ) {
        let mut destptr = dest;
        let mut info = info_arg;
        let mut msg0 = mr0;
        let mut msg1 = mr1;
        let mut msg2 = mr2;
        let mut msg3 = mr3;
        let scno = sys;
        asm!(
            "svc #0",
            inout("x0") destptr,
            inout("x2") msg0,
            inout("x3") msg1,
            inout("x4") msg2,
            inout("x5") msg3,
            inout("x1") info,
            in("x7") scno,
        );
    }

    #[inline(always)]
    unsafe fn arm_sys_recv(
        sys: seL4_Word,
        src: seL4_Word,
        out_badge: *mut seL4_Word,
        out_info: *mut seL4_Word,
        out_mr0: *mut seL4_Word,
        out_mr1: *mut seL4_Word,
        out_mr2: *mut seL4_Word,
        out_mr3: *mut seL4_Word,
    ) {
        let mut badge = src;
        let mut info = 0;
        let scno = sys;
        let mut msg0: seL4_Word;
        let mut msg1: seL4_Word;
        let mut msg2: seL4_Word;
        let mut msg3: seL4_Word;

        asm!(
            "svc #0",
            inout("x0") badge,
            lateout("x2") msg0,
            lateout("x3") msg1,
            lateout("x4") msg2,
            lateout("x5") msg3,
            lateout("x1") info,
            in("x7") scno,
            options(nostack, preserves_flags)
        );

        *out_badge = badge;
        *out_info = info;
        *out_mr0 = msg0;
        *out_mr1 = msg1;
        *out_mr2 = msg2;
        *out_mr3 = msg3;
    }

    #[inline(always)]
    unsafe fn arm_sys_send_recv(
        sys: seL4_Word,
        dest: seL4_Word,
        out_badge: *mut seL4_Word,
        info_arg: seL4_Word,
        out_info: *mut seL4_Word,
        in_out_mr0: *mut seL4_Word,
        in_out_mr1: *mut seL4_Word,
        in_out_mr2: *mut seL4_Word,
        in_out_mr3: *mut seL4_Word,
        #[allow(unused_variables)] reply: seL4_Word,
    ) {
        let mut destptr = dest;
        let mut info = info_arg;
        let mut msg0 = *in_out_mr0;
        let mut msg1 = *in_out_mr1;
        let mut msg2 = *in_out_mr2;
        let mut msg3 = *in_out_mr3;
        let scno = sys;
        asm!(
            "svc #0",
            inout("x0") destptr,
        inout("x2") msg0,
        inout("x3") msg1,
        inout("x4") msg2,
        inout("x5") msg3,
        inout("x1") info,
        in("x7") scno,
        options(nostack),
    );
        *out_info = info;
        *out_badge = destptr;
        *in_out_mr0 = msg0;
        *in_out_mr1 = msg1;
        *in_out_mr2 = msg2;
        *in_out_mr3 = msg3;
    }

    #[inline(always)]
    unsafe fn arm_sys_null(sys: seL4_Word) {
        let scno = sys;
        asm!("svc #0", in("x7") scno, options(nostack, preserves_flags));
    }

    #[inline(always)]
    pub unsafe fn seL4_Send(dest: seL4_CPtr, msg_info: seL4_MessageInfo) {
        arm_sys_send(
            seL4_SysSend as seL4_Word,
            dest as seL4_Word,
            msg_info.words[0],
            seL4_GetMR(0),
            seL4_GetMR(1),
            seL4_GetMR(2),
            seL4_GetMR(3),
        );
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
        let mut info = msg_info;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        if !mr0.is_null() && info.length() > 0 {
            msg0 = *mr0;
        }
        if !mr1.is_null() && info.length() > 1 {
            msg1 = *mr1;
        }
        if !mr2.is_null() && info.length() > 2 {
            msg2 = *mr2;
        }
        if !mr3.is_null() && info.length() > 3 {
            msg3 = *mr3;
        }

        let mut badge_dest = dest as seL4_Word;

        arm_sys_send_recv(
            seL4_SysCall as seL4_Word,
            dest as seL4_Word,
            &mut badge_dest,
            info.words[0],
            &mut info.words[0],
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

        info
    }

    #[inline(always)]
    pub unsafe fn seL4_Recv(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;

        arm_sys_recv(
            seL4_SysRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
        );

        seL4_SetMR(0, mr0);
        seL4_SetMR(1, mr1);
        seL4_SetMR(2, mr2);
        seL4_SetMR(3, mr3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[inline(always)]
    pub unsafe fn seL4_NBRecv(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;

        arm_sys_recv(
            seL4_SysNBRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
        );

        seL4_SetMR(0, mr0);
        seL4_SetMR(1, mr1);
        seL4_SetMR(2, mr2);
        seL4_SetMR(3, mr3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Copy(
        service: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Uint8,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Uint8,
        rights: seL4_CapRights_t,
    ) -> seL4_Error {
        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = dest_depth as seL4_Word & 0xff;
        let mut mr2 = src_index;
        let mut mr3 = src_depth as seL4_Word & 0xff;

        seL4_SetMR(4, rights.words[0]);

        let tag = seL4_MessageInfo_new(invocation_label_CNodeCopy as seL4_Word, 0, 1, 5);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        if result != seL4_NoError {
            seL4_SetMR(4, rights.words[0]);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Mint(
        service: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Uint8,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Uint8,
        rights: seL4_CapRights_t,
        badge: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = dest_depth as seL4_Word & 0xff;
        let mut mr2 = src_index;
        let mut mr3 = src_depth as seL4_Word & 0xff;

        seL4_SetMR(4, rights.words[0]);
        seL4_SetMR(5, badge);

        let tag = seL4_MessageInfo_new(invocation_label_CNodeMint as seL4_Word, 0, 1, 6);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        if result != seL4_NoError {
            seL4_SetMR(4, rights.words[0]);
            seL4_SetMR(5, badge);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Move(
        service: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Uint8,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Uint8,
    ) -> seL4_Error {
        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = dest_depth as seL4_Word & 0xff;
        let mut mr2 = src_index;
        let mut mr3 = src_depth as seL4_Word & 0xff;

        let tag = seL4_MessageInfo_new(invocation_label_CNodeMove as seL4_Word, 0, 1, 4);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Delete(
        service: seL4_CNode,
        index: seL4_Word,
        depth: seL4_Uint8,
    ) -> seL4_Error {
        let mut mr0 = index;
        let mut mr1 = depth as seL4_Word & 0xff;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(invocation_label_CNodeDelete as seL4_Word, 0, 0, 2);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_Untyped_Retype(
        service: seL4_Untyped,
        obj_type: seL4_Word,
        size_bits: seL4_Word,
        root: seL4_CNode,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, root);

        let mut mr0 = obj_type;
        let mut mr1 = size_bits;
        let mut mr2 = node_index;
        let mut mr3 = node_depth;

        seL4_SetMR(4, node_offset);
        seL4_SetMR(5, num);

        let tag = seL4_MessageInfo_new(invocation_label_UntypedRetype as seL4_Word, 0, 1, 6);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        if result != seL4_NoError {
            seL4_SetMR(4, node_offset);
            seL4_SetMR(5, num);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Map(
        pt: seL4_ARM_PageTable,
        vspace: seL4_VSpace,
        vaddr: seL4_Word,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        seL4_SetCap(0, vspace);

        let mut mr0 = vaddr;
        let mut mr1 = attr as seL4_Word;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag =
            seL4_MessageInfo_new(arch_invocation_label_ARMPageTableMap as seL4_Word, 0, 1, 2);
        let output_tag = seL4_CallWithMRs(pt, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Map(
        page: seL4_ARM_Page,
        vspace: seL4_VSpace,
        vaddr: seL4_Word,
        rights: seL4_CapRights_t,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        seL4_SetCap(0, vspace);

        let mut mr0 = vaddr;
        let mut mr1 = rights.words[0];
        let mut mr2 = attr as seL4_Word;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(arch_invocation_label_ARMPageMap as seL4_Word, 0, 1, 3);
        let output_tag = seL4_CallWithMRs(page, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;

        if result != seL4_NoError {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }

        result
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        _tcb_cap: seL4_TCB,
        buffer_word: seL4_Word,
        _buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetIPCBuffer(buffer_word as *mut seL4_IPCBuffer);
        seL4_NoError
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_Poll(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        seL4_NBWait(src, sender_badge)
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_Poll(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        seL4_NBRecv(src, sender_badge)
    }

    #[inline(always)]
    pub unsafe fn seL4_Yield() {
        arm_sys_null(seL4_SysYield as seL4_Word);
        asm!("", options(nostack, nomem, preserves_flags));
    }

    pub const seL4_NoError: seL4_Error = seL4_Error_seL4_NoError;
    pub const seL4_InvalidArgument: seL4_Error = seL4_Error_seL4_InvalidArgument;
    pub const seL4_InvalidCapability: seL4_Error = seL4_Error_seL4_InvalidCapability;
    pub const seL4_AlignmentError: seL4_Error = seL4_Error_seL4_AlignmentError;
    pub const seL4_TruncatedMessage: seL4_Error = seL4_Error_seL4_TruncatedMessage;
    pub const seL4_RevokeFirst: seL4_Error = seL4_Error_seL4_RevokeFirst;
    pub const seL4_IllegalOperation: seL4_Error = seL4_Error_seL4_IllegalOperation;
    pub const seL4_NotEnoughMemory: seL4_Error = seL4_Error_seL4_NotEnoughMemory;
    pub const seL4_RangeError: seL4_Error = seL4_Error_seL4_RangeError;
    pub const seL4_FailedLookup: seL4_Error = seL4_Error_seL4_FailedLookup;
    pub const seL4_DeleteFirst: seL4_Error = seL4_Error_seL4_DeleteFirst;

    pub const seL4_SysSend: seL4_Word = seL4_Syscall_ID_seL4_SysSend as seL4_Word;
    pub const seL4_SysRecv: seL4_Word = seL4_Syscall_ID_seL4_SysRecv as seL4_Word;
    pub const seL4_SysNBRecv: seL4_Word = seL4_Syscall_ID_seL4_SysNBRecv as seL4_Word;
    pub const seL4_SysCall: seL4_Word = seL4_Syscall_ID_seL4_SysCall as seL4_Word;
    pub const seL4_SysYield: seL4_Word = seL4_Syscall_ID_seL4_SysYield as seL4_Word;

    pub const seL4_UntypedObject: seL4_ObjectType = api_object_seL4_UntypedObject;
    pub const seL4_TCBObject: seL4_ObjectType = api_object_seL4_TCBObject;
    pub const seL4_EndpointObject: seL4_ObjectType = api_object_seL4_EndpointObject;
    pub const seL4_NotificationObject: seL4_ObjectType = api_object_seL4_NotificationObject;
    pub const seL4_CapTableObject: seL4_ObjectType = api_object_seL4_CapTableObject;

    pub const seL4_ARM_Page: seL4_ObjectType = _object_seL4_ARM_SmallPageObject as seL4_ObjectType;
    pub const seL4_ARM_LargePage: seL4_ObjectType = _object_seL4_ARM_LargePageObject as seL4_ObjectType;
    pub const seL4_ARM_PageTableObject: seL4_ObjectType =
        _object_seL4_ARM_PageTableObject as seL4_ObjectType;
    pub const seL4_ARM_SmallPageObject: seL4_ObjectType = seL4_ARM_Page;

    pub const seL4_CapNull: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapNull as seL4_CPtr;
    pub const seL4_CapInitThreadTCB: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadTCB as seL4_CPtr;
    pub const seL4_CapInitThreadCNode: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadCNode as seL4_CPtr;
    pub const seL4_CapInitThreadVSpace: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadVSpace as seL4_CPtr;
    pub const seL4_CapIRQControl: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapIRQControl as seL4_CPtr;
    pub const seL4_CapASIDControl: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapASIDControl as seL4_CPtr;
    pub const seL4_CapInitThreadASIDPool: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadASIDPool as seL4_CPtr;
    pub const seL4_CapIOPortControl: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapIOPortControl as seL4_CPtr;
    pub const seL4_CapIOPort: seL4_CPtr = seL4_CapIOPortControl;
    pub const seL4_CapIOSpace: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapIOSpace as seL4_CPtr;
    pub const seL4_CapBootInfoFrame: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapBootInfoFrame as seL4_CPtr;
    pub const seL4_CapInitThreadIPCBuffer: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadIPCBuffer as seL4_CPtr;
    pub const seL4_CapDomain: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapDomain as seL4_CPtr;
    pub const seL4_CapSMMUSIDControl: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapSMMUSIDControl as seL4_CPtr;
    pub const seL4_CapSMMUCBControl: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapSMMUCBControl as seL4_CPtr;
    pub const seL4_CapInitThreadSC: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapInitThreadSC as seL4_CPtr;
    pub const seL4_CapSMC: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapSMC as seL4_CPtr;

    pub const seL4_WordBits: seL4_Word = (core::mem::size_of::<seL4_Word>() * 8) as seL4_Word;

    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes =
        seL4_ARM_VMAttributes_seL4_ARM_Default_VMAttributes;
    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = 0;
    pub use seL4_DebugCapIdentify as seL4_CapIdentify;

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

    impl seL4_CapRights {
        #[inline(always)]
        pub const fn new(
            grant_reply: u8,
            grant: u8,
            read: u8,
            write: u8,
        ) -> Self {
            let mut value: seL4_Word = 0;
            value |= (grant_reply as seL4_Word & 0x1) << 3;
            value |= (grant as seL4_Word & 0x1) << 2;
            value |= (read as seL4_Word & 0x1) << 1;
            value |= write as seL4_Word & 0x1;
            Self { words: [value] }
        }

        #[inline(always)]
        pub const fn raw(self) -> seL4_Word {
            self.words[0]
        }
    }

    pub const seL4_AllRights: seL4_CapRights = seL4_CapRights::new(1, 1, 1, 1);
    pub const seL4_CapRights_All: seL4_CapRights = seL4_AllRights;
    pub const seL4_CapRights_ReadWrite: seL4_CapRights = seL4_CapRights::new(0, 0, 1, 1);

    #[inline(always)]
    pub const fn seL4_CapRights_to_word(rights: seL4_CapRights) -> seL4_CapRights_t {
        rights
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
            value |= (label & 0xfffffffffffff) << 12;
            value |= (caps_unwrapped & 0x7) << 9;
            value |= (extra_caps & 0x3) << 7;
            value |= length & 0x7f;
            Self { words: [value] }
        }

        #[inline(always)]
        pub const fn length(self) -> seL4_Word {
            (self.words[0] & 0x7f) >> 0
        }

        #[inline(always)]
        pub const fn label(self) -> seL4_Word {
            (self.words[0] & 0xfffffffffffff000) >> 12
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            (self.words[0] & 0x180) >> 7
        }

        #[inline(always)]
        pub const fn caps_unwrapped(self) -> seL4_Word {
            (self.words[0] & 0xe00) >> 9
        }
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
            caps_unwrapped: seL4_Word,
            extra_caps: seL4_Word,
            length: seL4_Word,
        ) -> Self {
            let mut value: seL4_Word = 0;
            value |= (label & 0xfffffffffffff) << 12;
            value |= (caps_unwrapped & 0x7) << 9;
            value |= (extra_caps & 0x3) << 7;
            value |= length & 0x7f;
            Self { words: [value] }
        }

        #[inline(always)]
        pub const fn label(self) -> seL4_Word {
            (self.words[0] & 0xfffffffffffff000) >> 12
        }

        #[inline(always)]
        pub const fn get_label(self) -> seL4_Word {
            self.label()
        }

        #[inline(always)]
        pub const fn caps_unwrapped(self) -> seL4_Word {
            (self.words[0] & 0xe00) >> 9
        }

        #[inline(always)]
        pub const fn get_capsUnwrapped(self) -> seL4_Word {
            self.caps_unwrapped()
        }

        #[inline(always)]
        pub const fn length(self) -> seL4_Word {
            (self.words[0] & 0x7f) >> 0
        }

        #[inline(always)]
        pub const fn extra_caps(self) -> seL4_Word {
            (self.words[0] & 0x180) >> 7
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
