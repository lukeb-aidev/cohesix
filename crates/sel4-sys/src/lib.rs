// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the sel4-sys library and public module surface.
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

    ::core::include!(::core::concat!(::core::env!("OUT_DIR"), "/bindings.rs"));
    ::core::include!(::core::concat!(
        ::core::env!("OUT_DIR"),
        "/sel4_config_consts.rs"
    ));

    #[cfg(all(sel4_sys_config_printing, not(sel4_sys_has_debug_put_char_syscall)))]
    compile_error!(
        "selected seL4 printing profile does not expose its generated DebugPutChar syscall ID"
    );

    #[cfg(all(sel4_sys_config_debug_build, not(sel4_sys_has_debug_halt_syscall)))]
    compile_error!(
        "selected seL4 debug profile does not expose its generated DebugHalt syscall ID"
    );

    /// Number of machine words in an AArch64 `seL4_UserContext`.
    pub const SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT: seL4_Word = 36;
    const SEL4_TCB_WRITE_REGISTERS_MESSAGE_LENGTH: seL4_Word =
        2 + SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT;

    #[inline(always)]
    fn aarch64_user_context_register(regs: &seL4_UserContext, index: seL4_Word) -> seL4_Word {
        match index {
            0 => regs.pc,
            1 => regs.sp,
            2 => regs.spsr,
            3 => regs.x0,
            4 => regs.x1,
            5 => regs.x2,
            6 => regs.x3,
            7 => regs.x4,
            8 => regs.x5,
            9 => regs.x6,
            10 => regs.x7,
            11 => regs.x8,
            12 => regs.x16,
            13 => regs.x17,
            14 => regs.x18,
            15 => regs.x29,
            16 => regs.x30,
            17 => regs.x9,
            18 => regs.x10,
            19 => regs.x11,
            20 => regs.x12,
            21 => regs.x13,
            22 => regs.x14,
            23 => regs.x15,
            24 => regs.x19,
            25 => regs.x20,
            26 => regs.x21,
            27 => regs.x22,
            28 => regs.x23,
            29 => regs.x24,
            30 => regs.x25,
            31 => regs.x26,
            32 => regs.x27,
            33 => regs.x28,
            34 => regs.tpidr_el0,
            35 => regs.tpidrro_el0,
            _ => 0,
        }
    }

    extern "C" {
        #[cfg(not(sel4_sys_bindings_have_debug_cap_identify))]
        pub fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Word;

        #[cfg(not(sel4_sys_bindings_have_debug_put_char))]
        pub fn seL4_DebugPutChar(c: u8);

        #[cfg(not(sel4_sys_bindings_have_debug_dump_scheduler))]
        pub fn seL4_DebugDumpScheduler();

        #[cfg(not(sel4_sys_bindings_have_debug_dump_cpuinfo))]
        pub fn seL4_DebugDumpCPUInfo();
    }

    pub type seL4_VSpace = seL4_CPtr;

    #[no_mangle]
    pub static mut __sel4_ipc_buffer: *mut seL4_IPCBuffer = core::ptr::null_mut();

    #[no_mangle]
    pub static mut __sel4_print_error: core::ffi::c_char = 0;

    #[no_mangle]
    pub static mut bootinfo: *mut seL4_BootInfo = core::ptr::null_mut();

    #[inline(always)]
    pub unsafe fn seL4_GetIPCBuffer() -> *mut seL4_IPCBuffer {
        __sel4_ipc_buffer
    }

    #[inline(always)]
    pub unsafe fn seL4_SetIPCBuffer(buffer: *mut seL4_IPCBuffer) {
        __sel4_ipc_buffer = buffer;
    }

    #[export_name = "seL4_InitBootInfo"]
    pub unsafe extern "C" fn sel4_init_bootinfo(bi: *mut seL4_BootInfo) {
        bootinfo = bi;
        if !bi.is_null() {
            seL4_SetIPCBuffer((*bi).ipcBuffer);
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_InitBootInfo(bi: *mut seL4_BootInfo) {
        sel4_init_bootinfo(bi);
    }

    #[export_name = "seL4_GetBootInfo"]
    pub unsafe extern "C" fn sel4_get_bootinfo() -> *mut seL4_BootInfo {
        bootinfo
    }

    #[inline(always)]
    pub unsafe fn seL4_SetCap(index: i32, cptr: seL4_CPtr) {
        (*seL4_GetIPCBuffer()).caps_or_badges[index as usize] = cptr;
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(index: seL4_Word) -> seL4_Word {
        (*seL4_GetIPCBuffer()).msg[index as usize]
    }

    #[inline(always)]
    pub unsafe fn seL4_SetMR(index: seL4_Word, value: seL4_Word) {
        (*seL4_GetIPCBuffer()).msg[index as usize] = value;
    }

    /// Emits one byte using the selected kernel's generated debug syscall ABI.
    #[cfg(all(sel4_sys_has_debug_put_char_syscall, sel4_sys_config_printing))]
    #[inline(always)]
    pub fn debug_put_char(c: u8) {
        let mut unused0 = 0;
        let mut unused1 = 0;
        let mut unused2 = 0;
        let mut unused3 = 0;
        let mut unused4 = 0;
        let mut unused5 = 0;

        // SAFETY: all register operands are initialized machine words, every
        // output points to live local storage, and the selected printing
        // profile supplies the syscall ID checked by this function's cfg.
        unsafe {
            arm_sys_send_recv(
                seL4_Syscall_ID_seL4_SysDebugPutChar as seL4_Word,
                seL4_Word::from(c),
                &mut unused0,
                0,
                &mut unused1,
                &mut unused2,
                &mut unused3,
                &mut unused4,
                &mut unused5,
                0,
            );
        }
    }

    /// Requests a halt using the selected kernel's generated debug syscall ABI.
    #[cfg(all(sel4_sys_has_debug_halt_syscall, sel4_sys_config_debug_build))]
    #[inline(always)]
    pub fn debug_halt() {
        let mut unused0 = 0;
        let mut unused1 = 0;
        let mut unused2 = 0;
        let mut unused3 = 0;
        let mut unused4 = 0;
        let mut unused5 = 0;

        // SAFETY: all register operands are initialized machine words, every
        // output points to live local storage, and the selected debug profile
        // supplies the syscall ID checked by this function's cfg.
        unsafe {
            arm_sys_send_recv(
                seL4_Syscall_ID_seL4_SysDebugHalt as seL4_Word,
                0,
                &mut unused0,
                0,
                &mut unused1,
                &mut unused2,
                &mut unused3,
                &mut unused4,
                &mut unused5,
                0,
            );
        }
    }

    #[cfg(all(sel4_sys_has_debug_cap_identify_syscall, sel4_sys_config_debug_build))]
    #[export_name = "seL4_DebugCapIdentify"]
    pub unsafe extern "C" fn sel4_debug_cap_identify(cap: seL4_CPtr) -> seL4_Word {
        let mut cap_word = cap as seL4_Word;
        let mut info = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_send_recv(
            seL4_Syscall_ID_seL4_SysDebugCapIdentify as seL4_Word,
            cap_word,
            &mut cap_word,
            0,
            &mut info,
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        cap_word
    }

    #[cfg(any(
        not(sel4_sys_has_debug_cap_identify_syscall),
        not(sel4_sys_config_debug_build)
    ))]
    #[export_name = "seL4_DebugCapIdentify"]
    pub unsafe extern "C" fn sel4_debug_cap_identify_fallback(_cap: seL4_CPtr) -> seL4_Word {
        0
    }

    #[cfg(all(sel4_sys_has_debug_dump_scheduler_syscall, sel4_sys_config_debug_build))]
    #[export_name = "seL4_DebugDumpScheduler"]
    pub unsafe extern "C" fn sel4_debug_dump_scheduler() {
        let mut unused0 = 0;
        let mut unused1 = 0;
        let mut unused2 = 0;
        let mut unused3 = 0;
        let mut unused4 = 0;
        let mut unused5 = 0;

        arm_sys_send_recv(
            seL4_Syscall_ID_seL4_SysDebugDumpScheduler as seL4_Word,
            0,
            &mut unused0,
            0,
            &mut unused1,
            &mut unused2,
            &mut unused3,
            &mut unused4,
            &mut unused5,
            0,
        );
    }

    #[cfg(any(
        not(sel4_sys_has_debug_dump_scheduler_syscall),
        not(sel4_sys_config_debug_build)
    ))]
    #[export_name = "seL4_DebugDumpScheduler"]
    pub unsafe extern "C" fn sel4_debug_dump_scheduler_fallback() {}

    #[cfg(all(sel4_sys_debug_dump_cpuinfo, sel4_sys_config_debug_build))]
    #[export_name = "seL4_DebugDumpCPUInfo"]
    pub unsafe extern "C" fn sel4_debug_dump_cpu_info() {
        let mut unused0 = 0;
        let mut unused1 = 0;
        let mut unused2 = 0;
        let mut unused3 = 0;
        let mut unused4 = 0;
        let mut unused5 = 0;

        arm_sys_send_recv(
            seL4_Syscall_ID_seL4_SysDebugDumpCPUInfo as seL4_Word,
            0,
            &mut unused0,
            0,
            &mut unused1,
            &mut unused2,
            &mut unused3,
            &mut unused4,
            &mut unused5,
            0,
        );
    }

    #[cfg(any(not(sel4_sys_debug_dump_cpuinfo), not(sel4_sys_config_debug_build)))]
    #[export_name = "seL4_DebugDumpCPUInfo"]
    pub unsafe extern "C" fn sel4_debug_dump_cpu_info() {}

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
        let scno = sys;
        asm!(
            "svc #0",
            in("x0") dest,
            in("x2") mr0,
            in("x3") mr1,
            in("x4") mr2,
            in("x5") mr3,
            in("x1") info_arg,
            in("x7") scno,
            options(nostack)
        );
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    unsafe fn arm_sys_reply(
        sys: seL4_Word,
        info_arg: seL4_Word,
        mr0: seL4_Word,
        mr1: seL4_Word,
        mr2: seL4_Word,
        mr3: seL4_Word,
    ) {
        let scno = sys;

        asm!(
            "svc #0",
            in("x2") mr0,
            in("x3") mr1,
            in("x4") mr2,
            in("x5") mr3,
            in("x1") info_arg,
            in("x7") scno,
            options(nostack)
        );
    }

    #[inline(always)]
    #[cfg_attr(not(sel4_config_kernel_mcs), allow(unused_variables))]
    unsafe fn arm_sys_recv(
        sys: seL4_Word,
        src: seL4_Word,
        out_badge: *mut seL4_Word,
        out_info: *mut seL4_Word,
        out_mr0: *mut seL4_Word,
        out_mr1: *mut seL4_Word,
        out_mr2: *mut seL4_Word,
        out_mr3: *mut seL4_Word,
        reply: seL4_Word,
    ) {
        let mut badge = src;
        let mut info: seL4_Word;
        let scno = sys;
        let msg0: seL4_Word;
        let msg1: seL4_Word;
        let msg2: seL4_Word;
        let msg3: seL4_Word;

        #[cfg(sel4_config_kernel_mcs)]
        asm!(
            "svc #0",
            out("x2") msg0,
            out("x3") msg1,
            out("x4") msg2,
            out("x5") msg3,
            out("x1") info,
            inout("x0") badge,
            in("x7") scno,
            in("x6") reply,
            options(nostack)
        );

        #[cfg(not(sel4_config_kernel_mcs))]
        asm!(
            "svc #0",
            out("x2") msg0,
            out("x3") msg1,
            out("x4") msg2,
            out("x5") msg3,
            out("x1") info,
            inout("x0") badge,
            in("x7") scno,
            options(nostack)
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
        #[cfg_attr(not(sel4_config_kernel_mcs), allow(unused_variables))] reply: seL4_Word,
    ) {
        let mut destptr = dest;
        let mut info = info_arg;
        let mut msg0 = *in_out_mr0;
        let mut msg1 = *in_out_mr1;
        let mut msg2 = *in_out_mr2;
        let mut msg3 = *in_out_mr3;
        let scno = sys;

        #[cfg(sel4_config_kernel_mcs)]
        asm!(
            "svc #0",
            inout("x2") msg0,
            inout("x3") msg1,
            inout("x4") msg2,
            inout("x5") msg3,
            inout("x1") info,
            inout("x0") destptr,
            in("x7") scno,
            in("x6") reply,
            options(nostack)
        );

        #[cfg(not(sel4_config_kernel_mcs))]
        asm!(
            "svc #0",
            inout("x2") msg0,
            inout("x3") msg1,
            inout("x4") msg2,
            inout("x5") msg3,
            inout("x1") info,
            inout("x0") destptr,
            in("x7") scno,
            options(nostack)
        );

        *out_info = info;
        *out_badge = destptr;
        *in_out_mr0 = msg0;
        *in_out_mr1 = msg1;
        *in_out_mr2 = msg2;
        *in_out_mr3 = msg3;
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    unsafe fn arm_sys_nbsend_recv(
        sys: seL4_Word,
        dest: seL4_Word,
        src: seL4_Word,
        out_badge: *mut seL4_Word,
        info_arg: seL4_Word,
        out_info: *mut seL4_Word,
        in_out_mr0: *mut seL4_Word,
        in_out_mr1: *mut seL4_Word,
        in_out_mr2: *mut seL4_Word,
        in_out_mr3: *mut seL4_Word,
        reply: seL4_Word,
    ) {
        let mut src_and_badge = src;
        let mut info = info_arg;
        let mut msg0 = *in_out_mr0;
        let mut msg1 = *in_out_mr1;
        let mut msg2 = *in_out_mr2;
        let mut msg3 = *in_out_mr3;
        let reply_reg = reply;
        let dest_reg = dest;
        let scno = sys;

        asm!(
            "svc #0",
            inout("x2") msg0,
            inout("x3") msg1,
            inout("x4") msg2,
            inout("x5") msg3,
            inout("x0") src_and_badge,
            inout("x1") info,
            in("x7") scno,
            in("x6") reply_reg,
            in("x8") dest_reg,
            options(nostack)
        );

        *out_badge = src_and_badge;
        *out_info = info;
        *in_out_mr0 = msg0;
        *in_out_mr1 = msg1;
        *in_out_mr2 = msg2;
        *in_out_mr3 = msg3;
    }

    #[inline(always)]
    unsafe fn arm_sys_send_null(sys: seL4_Word, src: seL4_Word, info_arg: seL4_Word) {
        let scno = sys;

        asm!("svc #0", in("x0") src, in("x1") info_arg, in("x7") scno, options(nostack));
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
    pub unsafe fn seL4_SendWithMRs(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        mr0: *const seL4_Word,
        mr1: *const seL4_Word,
        mr2: *const seL4_Word,
        mr3: *const seL4_Word,
    ) {
        arm_sys_send(
            seL4_SysSend as seL4_Word,
            dest as seL4_Word,
            msg_info.words[0],
            if !mr0.is_null() && msg_info.length() > 0 {
                *mr0
            } else {
                0
            },
            if !mr1.is_null() && msg_info.length() > 0 {
                *mr1
            } else {
                0
            },
            if !mr2.is_null() && msg_info.length() > 0 {
                *mr2
            } else {
                0
            },
            if !mr3.is_null() && msg_info.length() > 0 {
                *mr3
            } else {
                0
            },
        );
    }

    #[inline(always)]
    pub unsafe fn seL4_NBSend(dest: seL4_CPtr, msg_info: seL4_MessageInfo) {
        arm_sys_send(
            seL4_SysNBSend as seL4_Word,
            dest as seL4_Word,
            msg_info.words[0],
            seL4_GetMR(0),
            seL4_GetMR(1),
            seL4_GetMR(2),
            seL4_GetMR(3),
        );
    }

    #[inline(always)]
    pub unsafe fn seL4_NBSendWithMRs(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        mr0: *const seL4_Word,
        mr1: *const seL4_Word,
        mr2: *const seL4_Word,
        mr3: *const seL4_Word,
    ) {
        arm_sys_send(
            seL4_SysNBSend as seL4_Word,
            dest as seL4_Word,
            msg_info.words[0],
            if !mr0.is_null() && msg_info.length() > 0 {
                *mr0
            } else {
                0
            },
            if !mr1.is_null() && msg_info.length() > 0 {
                *mr1
            } else {
                0
            },
            if !mr2.is_null() && msg_info.length() > 0 {
                *mr2
            } else {
                0
            },
            if !mr3.is_null() && msg_info.length() > 0 {
                *mr3
            } else {
                0
            },
        );
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_Reply(msg_info: seL4_MessageInfo) {
        arm_sys_reply(
            seL4_SysReply as seL4_Word,
            msg_info.words[0],
            seL4_GetMR(0),
            seL4_GetMR(1),
            seL4_GetMR(2),
            seL4_GetMR(3),
        );
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_ReplyWithMRs(
        msg_info: seL4_MessageInfo,
        mr0: *const seL4_Word,
        mr1: *const seL4_Word,
        mr2: *const seL4_Word,
        mr3: *const seL4_Word,
    ) {
        arm_sys_reply(
            seL4_SysReply as seL4_Word,
            msg_info.words[0],
            if !mr0.is_null() && msg_info.length() > 0 {
                *mr0
            } else {
                0
            },
            if !mr1.is_null() && msg_info.length() > 0 {
                *mr1
            } else {
                0
            },
            if !mr2.is_null() && msg_info.length() > 0 {
                *mr2
            } else {
                0
            },
            if !mr3.is_null() && msg_info.length() > 0 {
                *mr3
            } else {
                0
            },
        );
    }

    #[inline(always)]
    pub unsafe fn seL4_Signal(dest: seL4_CPtr) {
        arm_sys_send_null(
            seL4_SysSend as seL4_Word,
            dest as seL4_Word,
            seL4_MessageInfo_new(0, 0, 0, 0).words[0],
        );
    }

    #[inline(always)]
    pub unsafe fn seL4_Call(dest: seL4_CPtr, msg_info: seL4_MessageInfo) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut msg0 = seL4_GetMR(0);
        let mut msg1 = seL4_GetMR(1);
        let mut msg2 = seL4_GetMR(2);
        let mut msg3 = seL4_GetMR(3);
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

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        info
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

    /// Reply through an explicit MCS Reply capability.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_MCS_Reply(reply: seL4_CPtr, msg_info: seL4_MessageInfo) {
        seL4_Send(reply, msg_info);
    }

    /// Reply through an explicit MCS Reply capability using caller-owned MRs.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_MCS_ReplyWithMRs(
        reply: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        mr0: *const seL4_Word,
        mr1: *const seL4_Word,
        mr2: *const seL4_Word,
        mr3: *const seL4_Word,
    ) {
        seL4_SendWithMRs(reply, msg_info, mr0, mr1, mr2, mr3);
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_ReplyRecv(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        reply_recv_with_cap(src, msg_info, sender_badge, reply)
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_ReplyRecv(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        reply_recv_with_cap(src, msg_info, sender_badge, 0)
    }

    #[inline(always)]
    unsafe fn reply_recv_with_cap(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut badge = src as seL4_Word;
        let mut mr0 = seL4_GetMR(0);
        let mut mr1 = seL4_GetMR(1);
        let mut mr2 = seL4_GetMR(2);
        let mut mr3 = seL4_GetMR(3);

        arm_sys_send_recv(
            seL4_SysReplyRecv,
            src as seL4_Word,
            &mut badge,
            info.words[0],
            &mut info.words[0],
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
            reply as seL4_Word,
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

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_ReplyRecvWithMRs(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        reply_recv_with_cap_and_mrs(src, msg_info, sender_badge, mr0, mr1, mr2, mr3, reply)
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_ReplyRecvWithMRs(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        reply_recv_with_cap_and_mrs(src, msg_info, sender_badge, mr0, mr1, mr2, mr3, 0)
    }

    #[inline(always)]
    unsafe fn reply_recv_with_cap_and_mrs(
        src: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut badge = 0;
        let mut msg0 = load_message_register(mr0, info.length(), 0);
        let mut msg1 = load_message_register(mr1, info.length(), 1);
        let mut msg2 = load_message_register(mr2, info.length(), 2);
        let mut msg3 = load_message_register(mr3, info.length(), 3);

        arm_sys_send_recv(
            seL4_SysReplyRecv,
            src as seL4_Word,
            &mut badge,
            info.words[0],
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            reply as seL4_Word,
        );

        store_message_register(mr0, msg0);
        store_message_register(mr1, msg1);
        store_message_register(mr2, msg2);
        store_message_register(mr3, msg3);
        if !sender_badge.is_null() {
            *sender_badge = badge;
        }
        info
    }

    #[inline(always)]
    unsafe fn load_message_register(
        register: *mut seL4_Word,
        length: seL4_Word,
        index: seL4_Word,
    ) -> seL4_Word {
        if !register.is_null() && length > index {
            *register
        } else {
            0
        }
    }

    #[inline(always)]
    unsafe fn store_message_register(register: *mut seL4_Word, value: seL4_Word) {
        if !register.is_null() {
            *register = value;
        }
    }

    /// Atomically perform an MCS nonblocking send and receive with a Reply object.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBSendRecv(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut badge = 0;
        let mut mr0 = seL4_GetMR(0);
        let mut mr1 = seL4_GetMR(1);
        let mut mr2 = seL4_GetMR(2);
        let mut mr3 = seL4_GetMR(3);
        arm_sys_nbsend_recv(
            seL4_SysNBSendRecv,
            dest as seL4_Word,
            src as seL4_Word,
            &mut badge,
            info.words[0],
            &mut info.words[0],
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
            reply as seL4_Word,
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

    /// Atomically perform an MCS nonblocking send and receive using caller-owned MRs.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBSendRecvWithMRs(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        mcs_nbsend_recv_with_mrs(
            seL4_SysNBSendRecv,
            dest,
            msg_info,
            src,
            sender_badge,
            mr0,
            mr1,
            mr2,
            mr3,
            reply,
        )
    }

    /// Invoke a Reply cap nonblockingly, then wait without allocating a Reply object.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBSendWait(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut badge = 0;
        let mut mr0 = seL4_GetMR(0);
        let mut mr1 = seL4_GetMR(1);
        let mut mr2 = seL4_GetMR(2);
        let mut mr3 = seL4_GetMR(3);
        arm_sys_nbsend_recv(
            seL4_SysNBSendWait,
            0,
            src as seL4_Word,
            &mut badge,
            info.words[0],
            &mut info.words[0],
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
            dest as seL4_Word,
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

    /// Invoke a Reply cap nonblockingly, then wait using caller-owned MRs.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBSendWaitWithMRs(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        mcs_nbsend_recv_with_mrs(
            seL4_SysNBSendWait,
            0,
            msg_info,
            src,
            sender_badge,
            mr0,
            mr1,
            mr2,
            mr3,
            dest,
        )
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    unsafe fn mcs_nbsend_recv_with_mrs(
        syscall: seL4_Word,
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = msg_info;
        let mut badge = 0;
        let mut msg0 = load_message_register(mr0, info.length(), 0);
        let mut msg1 = load_message_register(mr1, info.length(), 1);
        let mut msg2 = load_message_register(mr2, info.length(), 2);
        let mut msg3 = load_message_register(mr3, info.length(), 3);
        arm_sys_nbsend_recv(
            syscall,
            dest as seL4_Word,
            src as seL4_Word,
            &mut badge,
            info.words[0],
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            reply as seL4_Word,
        );
        store_message_register(mr0, msg0);
        store_message_register(mr1, msg1);
        store_message_register(mr2, msg2);
        store_message_register(mr3, msg3);
        if !sender_badge.is_null() {
            *sender_badge = badge;
        }
        info
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_Recv(
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            reply as seL4_Word,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_Recv(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_RecvWithMRs(
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            reply as seL4_Word,
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

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_RecvWithMRs(
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        mr0: *mut seL4_Word,
        mr1: *mut seL4_Word,
        mr2: *mut seL4_Word,
        mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
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

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBRecv(
        src: seL4_CPtr,
        sender_badge: *mut seL4_Word,
        reply: seL4_CPtr,
    ) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysNBRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            reply as seL4_Word,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_NBRecv(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysNBRecv as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_Wait(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysWait as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_Wait(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        seL4_Recv(src, sender_badge)
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_NBWait(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        let mut info = seL4_MessageInfo { words: [0] };
        let mut badge = 0;
        let mut msg0 = 0;
        let mut msg1 = 0;
        let mut msg2 = 0;
        let mut msg3 = 0;

        arm_sys_recv(
            seL4_SysNBWait as seL4_Word,
            src as seL4_Word,
            &mut badge,
            &mut info.words[0],
            &mut msg0,
            &mut msg1,
            &mut msg2,
            &mut msg3,
            0,
        );

        seL4_SetMR(0, msg0);
        seL4_SetMR(1, msg1);
        seL4_SetMR(2, msg2);
        seL4_SetMR(3, msg3);

        if !sender_badge.is_null() {
            *sender_badge = badge;
        }

        info
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_NBWait(src: seL4_CPtr, sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        seL4_NBRecv(src, sender_badge)
    }

    #[inline(always)]
    fn encode_depth(depth: seL4_Uint8) -> seL4_Word {
        depth as seL4_Word
    }

    fn set_error_mrs(mr0: seL4_Word, mr1: seL4_Word, mr2: seL4_Word, mr3: seL4_Word) {
        unsafe {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_CNode_Copy(
        dest_root: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Word,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Word,
        rights: seL4_CapRights,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_CNodeCopy as seL4_Word, 0, 1, 5);

        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = encode_depth(dest_depth as seL4_Uint8);
        let mut mr2 = src_index;
        let mut mr3 = encode_depth(src_depth as seL4_Uint8);
        seL4_SetMR(4, rights.words[0]);

        let output_tag = seL4_CallWithMRs(dest_root, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }

        result
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_CNode_Mint(
        dest_root: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Word,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Word,
        rights: seL4_CapRights,
        badge: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_CNodeMint as seL4_Word, 0, 1, 6);

        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = encode_depth(dest_depth as seL4_Uint8);
        let mut mr2 = src_index;
        let mut mr3 = encode_depth(src_depth as seL4_Uint8);
        seL4_SetMR(4, rights.words[0]);
        seL4_SetMR(5, badge);

        let output_tag = seL4_CallWithMRs(dest_root, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }

        result
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_CNode_Move(
        dest_root: seL4_CNode,
        dest_index: seL4_Word,
        dest_depth: seL4_Word,
        src_root: seL4_CNode,
        src_index: seL4_Word,
        src_depth: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_CNodeMove as seL4_Word, 0, 1, 4);

        seL4_SetCap(0, src_root);

        let mut mr0 = dest_index;
        let mut mr1 = encode_depth(dest_depth as seL4_Uint8);
        let mut mr2 = src_index;
        let mut mr3 = encode_depth(src_depth as seL4_Uint8);

        let output_tag = seL4_CallWithMRs(dest_root, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }

        result
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_CNode_Delete(
        dest_root: seL4_CNode,
        index: seL4_Word,
        depth: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_CNodeDelete as seL4_Word, 0, 0, 2);

        let mut mr0 = index;
        let mut mr1 = encode_depth(depth as seL4_Uint8);
        let mut mr2 = 0;
        let mut mr3 = 0;

        let output_tag = seL4_CallWithMRs(dest_root, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }

        result
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_CNode_Revoke(
        dest_root: seL4_CNode,
        index: seL4_Word,
        depth: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_CNodeRevoke as seL4_Word, 0, 0, 2);

        let mut mr0 = index;
        let mut mr1 = encode_depth(depth as seL4_Uint8);
        let mut mr2 = 0;
        let mut mr3 = 0;

        let output_tag = seL4_CallWithMRs(dest_root, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }

        result
    }

    #[no_mangle]
    pub unsafe extern "C" fn seL4_Untyped_Retype(
        ut_cap: seL4_Untyped,
        obj_type: seL4_Word,
        size_bits: seL4_Word,
        root: seL4_CNode,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo_new(invocation_label_UntypedRetype as seL4_Word, 0, 1, 6);

        seL4_SetCap(0, root);

        let mut mr0 = obj_type;
        let mut mr1 = size_bits;
        let mut mr2 = node_index;
        let mut mr3 = node_depth;
        seL4_SetMR(4, node_offset);
        seL4_SetMR(5, num);

        let output_tag = seL4_CallWithMRs(ut_cap, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
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

        let tag = seL4_MessageInfo_new(arch_invocation_label_ARMPageTableMap as seL4_Word, 0, 1, 2);
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
    pub unsafe fn seL4_ARM_PageTable_Unmap(pt: seL4_ARM_PageTable) -> seL4_Error {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(
            arch_invocation_label_ARMPageTableUnmap as seL4_Word,
            0,
            0,
            0,
        );
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
    pub unsafe fn seL4_ARM_Page_Unmap(page: seL4_ARM_Page) -> seL4_Error {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(arch_invocation_label_ARMPageUnmap as seL4_Word, 0, 0, 0);
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
    pub unsafe fn seL4_ARM_ASIDControl_MakePool(
        service: seL4_ARM_ASIDControl,
        untyped: seL4_Untyped,
        root: seL4_CNode,
        index: seL4_Word,
        depth: u8,
    ) -> seL4_Error {
        seL4_SetCap(0, untyped);
        seL4_SetCap(1, root);

        let mut mr0 = index;
        let mut mr1 = (depth as seL4_Word) & 0xff;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(
            arch_invocation_label_ARMASIDControlMakePool as seL4_Word,
            0,
            2,
            2,
        );
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
    pub unsafe fn seL4_ARM_ASIDPool_Assign(
        service: seL4_ARM_ASIDPool,
        vspace: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, vspace);

        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;

        let tag = seL4_MessageInfo_new(
            arch_invocation_label_ARMASIDPoolAssign as seL4_Word,
            0,
            1,
            0,
        );
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
    pub unsafe fn seL4_TCB_SetSpace(
        service: seL4_TCB,
        fault_ep: seL4_CPtr,
        cspace_root: seL4_CNode,
        cspace_root_data: seL4_Word,
        vspace_root: seL4_CPtr,
        vspace_root_data: seL4_Word,
    ) -> seL4_Error {
        #[cfg(sel4_config_kernel_mcs)]
        {
            seL4_SetCap(0, fault_ep);
            seL4_SetCap(1, cspace_root);
            seL4_SetCap(2, vspace_root);

            let mut mr0: seL4_Word = cspace_root_data;
            let mut mr1: seL4_Word = vspace_root_data;
            let mut mr2: seL4_Word = 0;
            let mut mr3: seL4_Word = 0;

            let tag = seL4_MessageInfo::new(invocation_label_TCBSetSpace as seL4_Word, 0, 3, 2);
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

        #[cfg(not(sel4_config_kernel_mcs))]
        {
            seL4_SetCap(0, cspace_root);
            seL4_SetCap(1, vspace_root);

            let mut mr0: seL4_Word = fault_ep as seL4_Word;
            let mut mr1: seL4_Word = cspace_root_data;
            let mut mr2: seL4_Word = vspace_root_data;
            let mut mr3: seL4_Word = 0;

            let tag = seL4_MessageInfo::new(invocation_label_TCBSetSpace as seL4_Word, 0, 2, 3);
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
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetFaultHandler(
        tcb: seL4_TCB,
        fault_handler: seL4_CPtr,
        cspace_root: seL4_CNode,
        cspace_root_data: seL4_Word,
        vspace_root: seL4_CPtr,
        vspace_root_data: seL4_Word,
    ) -> seL4_Error {
        seL4_TCB_SetSpace(
            tcb,
            fault_handler,
            cspace_root,
            cspace_root_data,
            vspace_root,
            vspace_root_data,
        )
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        service: seL4_TCB,
        buffer: seL4_Word,
        buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, buffer_frame);

        let mut mr0: seL4_Word = buffer;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBSetIPCBuffer as seL4_Word, 0, 1, 1);
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
    pub unsafe fn seL4_TCB_WriteRegisters(
        service: seL4_TCB,
        resume_target: seL4_Bool,
        arch_flags: seL4_Uint8,
        count: seL4_Word,
        regs: *const seL4_UserContext,
    ) -> seL4_Error {
        if regs.is_null() {
            return seL4_InvalidArgument;
        }

        let regs = &*regs;
        let bounded_count = core::cmp::min(count, SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT);
        let mut mr0: seL4_Word =
            ((resume_target as seL4_Word) & 0x1) | (((arch_flags as seL4_Word) & 0xff) << 8);
        let mut mr1: seL4_Word = count;
        // seL4's AArch64 fast-message ABI always carries PC and SP in mr2/mr3.
        // The kernel uses `count` to decide how many context words to consume.
        let mut mr2: seL4_Word = aarch64_user_context_register(regs, 0);
        let mut mr3: seL4_Word = aarch64_user_context_register(regs, 1);
        let mut register_index = 2;
        while register_index < bounded_count {
            seL4_SetMR(
                register_index + 2,
                aarch64_user_context_register(regs, register_index),
            );
            register_index += 1;
        }

        let tag = seL4_MessageInfo::new(
            invocation_label_TCBWriteRegisters as seL4_Word,
            0,
            0,
            SEL4_TCB_WRITE_REGISTERS_MESSAGE_LENGTH,
        );
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
    pub unsafe fn seL4_TCB_SetPriority(
        service: seL4_TCB,
        authority: seL4_TCB,
        priority: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);

        let mut mr0: seL4_Word = priority;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBSetPriority as seL4_Word, 0, 1, 1);
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

    #[cfg(not(sel4_config_kernel_mcs))]
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSchedParams(
        service: seL4_TCB,
        authority: seL4_TCB,
        mcp: seL4_Word,
        priority: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);

        let mut mr0: seL4_Word = mcp;
        let mut mr1: seL4_Word = priority;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBSetSchedParams as seL4_Word, 0, 1, 2);
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

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSchedParams(
        service: seL4_TCB,
        authority: seL4_TCB,
        mcp: seL4_Word,
        priority: seL4_Word,
        sched_context: seL4_SchedContext,
        fault_ep: seL4_CPtr,
    ) -> seL4_Error {
        seL4_TCB_SetSchedParamsMcs(service, authority, mcp, priority, sched_context, fault_ep)
    }

    /// Invoke the seL4 16 MCS `TCB_SetSchedParams` ABI.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSchedParamsMcs(
        service: seL4_TCB,
        authority: seL4_TCB,
        mcp: seL4_Word,
        priority: seL4_Word,
        sched_context: seL4_SchedContext,
        fault_ep: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);
        seL4_SetCap(1, sched_context);
        seL4_SetCap(2, fault_ep);

        let mut mr0 = mcp;
        let mut mr1 = priority;
        let mut mr2 = 0;
        let mut mr3 = 0;
        let tag = seL4_MessageInfo::new(invocation_label_TCBSetSchedParams as seL4_Word, 0, 3, 2);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }
        result
    }

    /// Set the endpoint that receives timeout faults for an MCS TCB.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetTimeoutEndpoint(
        service: seL4_TCB,
        timeout_fault_ep: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, timeout_fault_ep);
        invoke_mcs_object(
            service,
            invocation_label_TCBSetTimeoutEndpoint as seL4_Word,
            1,
            0,
        )
    }

    /// Configure budget, period, replenishment, badge, and flags for an MCS SC.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedControl_ConfigureFlags(
        service: seL4_SchedControl,
        sched_context: seL4_SchedContext,
        budget: seL4_Time,
        period: seL4_Time,
        extra_refills: seL4_Word,
        badge: seL4_Word,
        flags: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, sched_context);
        seL4_SetMR(4, flags);
        invoke_mcs_object_with_mrs(
            service,
            invocation_label_SchedControlConfigureFlags as seL4_Word,
            1,
            5,
            budget,
            period,
            extra_refills,
            badge,
        )
    }

    /// Bind an MCS scheduling context to a TCB or notification.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Bind(service: seL4_SchedContext, cap: seL4_CPtr) -> seL4_Error {
        seL4_SetCap(0, cap);
        invoke_mcs_object(
            service,
            invocation_label_SchedContextBind as seL4_Word,
            1,
            0,
        )
    }

    /// Unbind both object associations from an MCS scheduling context.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Unbind(service: seL4_SchedContext) -> seL4_Error {
        invoke_mcs_object(
            service,
            invocation_label_SchedContextUnbind as seL4_Word,
            0,
            0,
        )
    }

    /// Unbind one TCB or notification from an MCS scheduling context.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedContext_UnbindObject(
        service: seL4_SchedContext,
        cap: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, cap);
        invoke_mcs_object(
            service,
            invocation_label_SchedContextUnbindObject as seL4_Word,
            1,
            0,
        )
    }

    /// Read and reset the consumed-time counter for an MCS scheduling context.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Consumed(
        service: seL4_SchedContext,
    ) -> seL4_SchedContext_Consumed_t {
        let (error, consumed) =
            invoke_mcs_consumed(service, invocation_label_SchedContextConsumed as seL4_Word);
        seL4_SchedContext_Consumed {
            error: error as core::ffi::c_int,
            consumed,
        }
    }

    /// Yield to the TCB currently bound to an MCS scheduling context.
    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    pub unsafe fn seL4_SchedContext_YieldTo(
        service: seL4_SchedContext,
    ) -> seL4_SchedContext_YieldTo_t {
        let (error, consumed) =
            invoke_mcs_consumed(service, invocation_label_SchedContextYieldTo as seL4_Word);
        seL4_SchedContext_YieldTo {
            error: error as core::ffi::c_int,
            consumed,
        }
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    unsafe fn invoke_mcs_object(
        service: seL4_CPtr,
        label: seL4_Word,
        extra_caps: seL4_Word,
        length: seL4_Word,
    ) -> seL4_Error {
        invoke_mcs_object_with_mrs(service, label, extra_caps, length, 0, 0, 0, 0)
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    unsafe fn invoke_mcs_object_with_mrs(
        service: seL4_CPtr,
        label: seL4_Word,
        extra_caps: seL4_Word,
        length: seL4_Word,
        mut mr0: seL4_Word,
        mut mr1: seL4_Word,
        mut mr2: seL4_Word,
        mut mr3: seL4_Word,
    ) -> seL4_Error {
        let tag = seL4_MessageInfo::new(label, 0, extra_caps, length);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
        }
        result
    }

    #[cfg(sel4_config_kernel_mcs)]
    #[inline(always)]
    unsafe fn invoke_mcs_consumed(
        service: seL4_SchedContext,
        label: seL4_Word,
    ) -> (seL4_Error, seL4_Time) {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;
        let tag = seL4_MessageInfo::new(label, 0, 0, 0);
        let output_tag = seL4_CallWithMRs(service, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = seL4_MessageInfo_get_label(output_tag) as seL4_Error;
        if result != seL4_NoError {
            set_error_mrs(mr0, mr1, mr2, mr3);
            (result, 0)
        } else {
            (result, mr0 as seL4_Time)
        }
    }

    #[inline(always)]
    #[cfg(sel4_sys_has_tcb_set_affinity)]
    pub unsafe fn seL4_TCB_SetAffinity(service: seL4_TCB, affinity: seL4_Word) -> seL4_Error {
        let mut mr0: seL4_Word = affinity;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBSetAffinity as seL4_Word, 0, 0, 1);
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
    pub unsafe fn seL4_TCB_Resume(service: seL4_TCB) -> seL4_Error {
        let mut mr0: seL4_Word = 0;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBResume as seL4_Word, 0, 0, 0);
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
    pub unsafe fn seL4_TCB_BindNotification(
        service: seL4_TCB,
        notification: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, notification);

        let mut mr0: seL4_Word = 0;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBBindNotification as seL4_Word, 0, 1, 0);
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
    pub unsafe fn seL4_TCB_UnbindNotification(service: seL4_TCB) -> seL4_Error {
        let mut mr0: seL4_Word = 0;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag =
            seL4_MessageInfo::new(invocation_label_TCBUnbindNotification as seL4_Word, 0, 0, 0);
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
    #[cfg(not(sel4_sys_has_tcb_set_affinity))]
    pub unsafe fn seL4_TCB_SetAffinity(_service: seL4_TCB, _affinity: seL4_Word) -> seL4_Error {
        seL4_IllegalOperation
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_Suspend(service: seL4_TCB) -> seL4_Error {
        let mut mr0: seL4_Word = 0;
        let mut mr1: seL4_Word = 0;
        let mut mr2: seL4_Word = 0;
        let mut mr3: seL4_Word = 0;

        let tag = seL4_MessageInfo::new(invocation_label_TCBSuspend as seL4_Word, 0, 0, 0);
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
    pub const seL4_SysNBSend: seL4_Word = seL4_Syscall_ID_seL4_SysNBSend as seL4_Word;
    pub const seL4_SysRecv: seL4_Word = seL4_Syscall_ID_seL4_SysRecv as seL4_Word;
    pub const seL4_SysNBRecv: seL4_Word = seL4_Syscall_ID_seL4_SysNBRecv as seL4_Word;
    pub const seL4_SysReplyRecv: seL4_Word = seL4_Syscall_ID_seL4_SysReplyRecv as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SysNBSendRecv: seL4_Word = seL4_Syscall_ID_seL4_SysNBSendRecv as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SysNBSendWait: seL4_Word = seL4_Syscall_ID_seL4_SysNBSendWait as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SysWait: seL4_Word = seL4_Syscall_ID_seL4_SysWait as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SysNBWait: seL4_Word = seL4_Syscall_ID_seL4_SysNBWait as seL4_Word;
    #[cfg(not(sel4_config_kernel_mcs))]
    pub const seL4_SysReply: seL4_Word = seL4_Syscall_ID_seL4_SysReply as seL4_Word;
    pub const seL4_SysCall: seL4_Word = seL4_Syscall_ID_seL4_SysCall as seL4_Word;
    pub const seL4_SysYield: seL4_Word = seL4_Syscall_ID_seL4_SysYield as seL4_Word;

    pub const seL4_UntypedObject: seL4_ObjectType = api_object_seL4_UntypedObject;
    pub const seL4_TCBObject: seL4_ObjectType = api_object_seL4_TCBObject;
    pub const seL4_EndpointObject: seL4_ObjectType = api_object_seL4_EndpointObject;
    pub const seL4_NotificationObject: seL4_ObjectType = api_object_seL4_NotificationObject;
    pub const seL4_CapTableObject: seL4_ObjectType = api_object_seL4_CapTableObject;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SchedContextObject: seL4_ObjectType = api_object_seL4_SchedContextObject;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_ReplyObject: seL4_ObjectType = api_object_seL4_ReplyObject;

    pub const seL4_ARM_Page: seL4_ObjectType = _object_seL4_ARM_SmallPageObject as seL4_ObjectType;
    pub const seL4_ARM_LargePage: seL4_ObjectType =
        _object_seL4_ARM_LargePageObject as seL4_ObjectType;
    pub const seL4_ARM_PageTableObject: seL4_ObjectType =
        _object_seL4_ARM_PageTableObject as seL4_ObjectType;
    pub const seL4_ARM_VSpaceObject: seL4_ObjectType =
        _mode_object_seL4_ARM_VSpaceObject as seL4_ObjectType;
    pub const seL4_ARM_SmallPageObject: seL4_ObjectType = seL4_ARM_Page;
    pub const seL4_UntypedObjectType: seL4_ObjectType = seL4_UntypedObject;
    pub const seL4_TCBObjectType: seL4_ObjectType = seL4_TCBObject;
    pub const seL4_EndpointObjectType: seL4_ObjectType = seL4_EndpointObject;
    pub const seL4_NotificationObjectType: seL4_ObjectType = seL4_NotificationObject;
    pub const seL4_CapTableObjectType: seL4_ObjectType = seL4_CapTableObject;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_SchedContextObjectType: seL4_ObjectType = seL4_SchedContextObject;
    #[cfg(sel4_config_kernel_mcs)]
    pub const seL4_ReplyObjectType: seL4_ObjectType = seL4_ReplyObject;
    pub const seL4_ARM_PageObjectType: seL4_ObjectType = seL4_ARM_Page;
    pub const seL4_ARM_LargePageObjectType: seL4_ObjectType = seL4_ARM_LargePage;
    pub const seL4_ARM_PageTableObjectType: seL4_ObjectType = seL4_ARM_PageTableObject;
    pub const seL4_ARM_VSpaceObjectType: seL4_ObjectType = seL4_ARM_VSpaceObject;

    pub const seL4_CapNull: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapNull as seL4_CPtr;
    pub const seL4_CapInitThreadTCB: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadTCB as seL4_CPtr;
    pub const seL4_CapInitThreadCNode: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadCNode as seL4_CPtr;
    pub const seL4_CapInitThreadVSpace: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadVSpace as seL4_CPtr;
    pub const seL4_CapIRQControl: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapIRQControl as seL4_CPtr;
    pub const seL4_CapASIDControl: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapASIDControl as seL4_CPtr;
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
    pub const seL4_CapInitThreadSC: seL4_CPtr =
        seL4_RootCNodeCapSlots_seL4_CapInitThreadSC as seL4_CPtr;
    pub const seL4_CapSMC: seL4_CPtr = seL4_RootCNodeCapSlots_seL4_CapSMC as seL4_CPtr;
    pub const seL4_NumInitialCaps: seL4_CPtr = seL4_CapSMC + 1;

    /// Fault labels emitted by the selected generated seL4 headers.
    pub const SEL4_FAULT_NULL_LABEL: seL4_Word = seL4_Fault_tag_seL4_Fault_NullFault as seL4_Word;
    pub const SEL4_FAULT_CAP_LABEL: seL4_Word = seL4_Fault_tag_seL4_Fault_CapFault as seL4_Word;
    pub const SEL4_FAULT_UNKNOWN_SYSCALL_LABEL: seL4_Word =
        seL4_Fault_tag_seL4_Fault_UnknownSyscall as seL4_Word;
    pub const SEL4_FAULT_USER_EXCEPTION_LABEL: seL4_Word =
        seL4_Fault_tag_seL4_Fault_UserException as seL4_Word;

    /// seL4 16 AArch64 MCS object and timeout-message contract.
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_MIN_SCHED_CONTEXT_BITS: seL4_Word = seL4_MinSchedContextBits as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_REPLY_BITS: seL4_Word = seL4_ReplyBits as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_NOTIFICATION_BITS: seL4_Word = seL4_NotificationBits as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_TIMEOUT_DATA: seL4_Word = seL4_Timeout_Msg_seL4_Timeout_Data as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_TIMEOUT_CONSUMED: seL4_Word =
        seL4_Timeout_Msg_seL4_Timeout_Consumed as seL4_Word;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_TIMEOUT_LENGTH: seL4_Word =
        seL4_Timeout_Msg_seL4_Timeout_Length as seL4_Word;
    #[cfg(not(sel4_config_kernel_mcs))]
    pub const SEL4_MCS_TIMEOUT_LENGTH: seL4_Word = 2;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_FAULT_TIMEOUT_LABEL: seL4_Word =
        seL4_Fault_tag_seL4_Fault_Timeout as seL4_Word;
    #[cfg(not(sel4_config_kernel_mcs))]
    pub const SEL4_MCS_FAULT_TIMEOUT_LABEL: seL4_Word = 5;
    #[cfg(sel4_config_kernel_mcs)]
    pub const SEL4_MCS_FAULT_VM_LABEL: seL4_Word = seL4_Fault_tag_seL4_Fault_VMFault as seL4_Word;
    #[cfg(not(sel4_config_kernel_mcs))]
    pub const SEL4_MCS_FAULT_VM_LABEL: seL4_Word = 6;

    pub const seL4_WordBits: seL4_Word = (core::mem::size_of::<seL4_Word>() * 8) as seL4_Word;

    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes =
        seL4_ARM_VMAttributes_seL4_ARM_Default_VMAttributes;
    pub const seL4_ARM_PageCacheable: seL4_ARM_VMAttributes =
        seL4_ARM_VMAttributes_seL4_ARM_PageCacheable;
    pub const seL4_ARM_ParityEnabled: seL4_ARM_VMAttributes =
        seL4_ARM_VMAttributes_seL4_ARM_ParityEnabled;
    pub const seL4_ARM_ExecuteNever: seL4_ARM_VMAttributes =
        seL4_ARM_VMAttributes_seL4_ARM_ExecuteNever;
    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = 0;
    pub const ARMVSpaceClean_Data: seL4_Word =
        sel4_arch_invocation_label_ARMVSpaceClean_Data as seL4_Word;
    pub const ARMVSpaceInvalidate_Data: seL4_Word =
        sel4_arch_invocation_label_ARMVSpaceInvalidate_Data as seL4_Word;
    pub const ARMVSpaceCleanInvalidate_Data: seL4_Word =
        sel4_arch_invocation_label_ARMVSpaceCleanInvalidate_Data as seL4_Word;
    pub const ARMVSpaceUnify_Instruction: seL4_Word =
        sel4_arch_invocation_label_ARMVSpaceUnify_Instruction as seL4_Word;
    pub const nSeL4ArchInvocationLabels: seL4_Word =
        sel4_arch_invocation_label_nSeL4ArchInvocationLabels as seL4_Word;
    pub use seL4_DebugCapIdentify as seL4_CapIdentify;

    #[repr(C, align(16))]
    pub struct TlsImage {
        ipc_buffer: *mut seL4_IPCBuffer,
    }

    impl Default for TlsImage {
        fn default() -> Self {
            Self::new()
        }
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
        pub const fn new(grant_reply: u8, grant: u8, read: u8, write: u8) -> Self {
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
            self.words[0] & 0x7f
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

    // libsel4 no longer exports fault message register indices. Keep the legacy
    // constants available for kernel-space consumers that decode faults using
    // message registers instead of the structured accessors.
    pub type seL4_UnknownSyscall_Msg = seL4_Word;
    pub const seL4_UnknownSyscall_X0: seL4_UnknownSyscall_Msg = 0;
    pub const seL4_UnknownSyscall_X1: seL4_UnknownSyscall_Msg = 1;
    pub const seL4_UnknownSyscall_X2: seL4_UnknownSyscall_Msg = 2;
    pub const seL4_UnknownSyscall_X3: seL4_UnknownSyscall_Msg = 3;
    pub const seL4_UnknownSyscall_X4: seL4_UnknownSyscall_Msg = 4;
    pub const seL4_UnknownSyscall_X5: seL4_UnknownSyscall_Msg = 5;
    pub const seL4_UnknownSyscall_X6: seL4_UnknownSyscall_Msg = 6;
    pub const seL4_UnknownSyscall_X7: seL4_UnknownSyscall_Msg = 7;
    pub const seL4_UnknownSyscall_FaultIP: seL4_UnknownSyscall_Msg = 8;
    pub const seL4_UnknownSyscall_SP: seL4_UnknownSyscall_Msg = 9;
    pub const seL4_UnknownSyscall_LR: seL4_UnknownSyscall_Msg = 10;
    pub const seL4_UnknownSyscall_SPSR: seL4_UnknownSyscall_Msg = 11;
    pub const seL4_UnknownSyscall_Syscall: seL4_UnknownSyscall_Msg = 12;
    pub const seL4_UnknownSyscall_Length: seL4_UnknownSyscall_Msg = 13;

    pub type seL4_UserException_Msg = seL4_Word;
    pub const seL4_UserException_FaultIP: seL4_UserException_Msg = 0;
    pub const seL4_UserException_SP: seL4_UserException_Msg = 1;
    pub const seL4_UserException_SPSR: seL4_UserException_Msg = 2;
    pub const seL4_UserException_Number: seL4_UserException_Msg = 3;
    pub const seL4_UserException_Code: seL4_UserException_Msg = 4;
    pub const seL4_UserException_Length: seL4_UserException_Msg = 5;

    pub type seL4_VMFault_Msg = seL4_Word;
    pub const seL4_VMFault_IP: seL4_VMFault_Msg = 0;
    pub const seL4_VMFault_Addr: seL4_VMFault_Msg = 1;
    pub const seL4_VMFault_PrefetchFault: seL4_VMFault_Msg = 2;
    pub const seL4_VMFault_FSR: seL4_VMFault_Msg = 3;
    pub const seL4_VMFault_Length: seL4_VMFault_Msg = 4;

    pub type seL4_CapFault_Msg = seL4_Word;
    pub const seL4_CapFault_IP: seL4_CapFault_Msg = 0;
    pub const seL4_CapFault_Addr: seL4_CapFault_Msg = 1;
    pub const seL4_CapFault_InRecvPhase: seL4_CapFault_Msg = 2;
    pub const seL4_CapFault_LookupFailureType: seL4_CapFault_Msg = 3;
    pub const seL4_CapFault_BitsLeft: seL4_CapFault_Msg = 4;
    pub const seL4_CapFault_DepthMismatch_BitsFound: seL4_CapFault_Msg = 5;
    pub const seL4_CapFault_GuardMismatch_GuardFound: seL4_CapFault_Msg = 5;
    pub const seL4_CapFault_GuardMismatch_BitsFound: seL4_CapFault_Msg = 6;
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

    pub const MAX_BOOTINFO_UNTYPEDS: usize = 230;

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
    pub type seL4_ARM_ASIDControl = seL4_CPtr;
    pub type seL4_ARM_ASIDPool = seL4_CPtr;
    pub type seL4_ARM_Page = seL4_CPtr;
    pub type seL4_ARM_PageTable = seL4_CPtr;
    pub type seL4_SchedContext = seL4_CPtr;
    pub type seL4_SchedControl = seL4_CPtr;
    pub type seL4_Time = u64;

    /// Number of machine words in an AArch64 `seL4_UserContext`.
    pub const SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT: seL4_Word = 36;
    const SEL4_TCB_WRITE_REGISTERS_MESSAGE_LENGTH: seL4_Word =
        2 + SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT;

    #[no_mangle]
    pub static mut __sel4_ipc_buffer: *mut seL4_IPCBuffer = ptr::null_mut();

    #[no_mangle]
    pub static mut bootinfo: *mut seL4_BootInfo = core::ptr::null_mut();

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
    pub const invocation_label_TCBWriteRegisters: seL4_Word = 3;
    pub const invocation_label_TCBSetPriority: seL4_Word = 6;
    pub const invocation_label_TCBSetMCPriority: seL4_Word = 7;
    pub const invocation_label_TCBSetSchedParams: seL4_Word = 8;
    pub const invocation_label_TCBSetIPCBuffer: seL4_Word = 9;
    pub const invocation_label_TCBSuspend: seL4_Word = 11;
    pub const invocation_label_TCBResume: seL4_Word = 12;
    pub const invocation_label_TCBBindNotification: seL4_Word = 13;
    pub const invocation_label_TCBUnbindNotification: seL4_Word = 14;
    pub const MCS_INVOCATION_LABEL_TCB_SET_TIMEOUT_ENDPOINT: seL4_Word = 9;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTROL_CONFIGURE_FLAGS: seL4_Word = 33;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTEXT_BIND: seL4_Word = 34;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND: seL4_Word = 35;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND_OBJECT: seL4_Word = 36;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTEXT_CONSUMED: seL4_Word = 37;
    pub const MCS_INVOCATION_LABEL_SCHED_CONTEXT_YIELD_TO: seL4_Word = 38;
    pub const MCS_INVOCATION_LABEL_COUNT: seL4_Word = 39;
    pub const SEL4_MCS_SYS_CALL: i32 = -1;
    pub const SEL4_MCS_SYS_REPLY_RECV: i32 = -2;
    pub const SEL4_MCS_SYS_NB_SEND_RECV: i32 = -3;
    pub const SEL4_MCS_SYS_NB_SEND_WAIT: i32 = -4;
    pub const SEL4_MCS_SYS_SEND: i32 = -5;
    pub const SEL4_MCS_SYS_NB_SEND: i32 = -6;
    pub const SEL4_MCS_SYS_RECV: i32 = -7;
    pub const SEL4_MCS_SYS_NB_RECV: i32 = -8;
    pub const SEL4_MCS_SYS_WAIT: i32 = -9;
    pub const SEL4_MCS_SYS_NB_WAIT: i32 = -10;
    pub const SEL4_MCS_SYS_YIELD: i32 = -11;
    pub const arch_invocation_label_ARMPageTableMap: seL4_Word = 37;
    pub const arch_invocation_label_ARMPageTableUnmap: seL4_Word = 38;
    pub const arch_invocation_label_ARMPageMap: seL4_Word = 39;
    pub const arch_invocation_label_ARMPageUnmap: seL4_Word = 40;
    pub const arch_invocation_label_ARMASIDControlMakePool: seL4_Word = 46;
    pub const arch_invocation_label_ARMASIDPoolAssign: seL4_Word = 47;
    pub const seL4_CapInitThreadSC: seL4_CPtr = 14;
    pub const seL4_CapSMC: seL4_CPtr = 15;
    pub const seL4_NumInitialCaps: seL4_CPtr = seL4_CapSMC + 1;
    pub const seL4_TCBBits: seL4_Word = 11;
    pub const seL4_SlotBits: seL4_Word = 5;

    pub const seL4_UntypedObject: seL4_Word = 0;
    pub const seL4_TCBObject: seL4_Word = 1;
    pub const seL4_EndpointObject: seL4_Word = 2;
    pub const seL4_NotificationObject: seL4_Word = 3;
    pub const seL4_CapTableObject: seL4_Word = 4;
    pub const seL4_ARM_HugePageObject: seL4_Word = 5;
    pub const seL4_ARM_VSpaceObject: seL4_Word = 6;
    pub const seL4_ARM_SmallPageObject: seL4_Word = 7;
    pub const seL4_ARM_LargePageObject: seL4_Word = 8;
    pub const seL4_ARM_PageTableObject: seL4_Word = 9;
    pub const seL4_EndpointBits: seL4_Word = 4;
    pub const seL4_NotificationBits: seL4_Word = 5;
    pub const seL4_VSpaceBits: seL4_Word = 12;
    pub const seL4_ASIDPoolBits: seL4_Word = 12;
    pub const SEL4_FAULT_NULL_LABEL: seL4_Word = 0;
    pub const SEL4_FAULT_CAP_LABEL: seL4_Word = 1;
    pub const SEL4_FAULT_UNKNOWN_SYSCALL_LABEL: seL4_Word = 2;
    pub const SEL4_FAULT_USER_EXCEPTION_LABEL: seL4_Word = 3;
    pub const SEL4_MCS_SCHED_CONTEXT_OBJECT: seL4_Word = 5;
    pub const SEL4_MCS_REPLY_OBJECT: seL4_Word = 6;
    pub const SEL4_MCS_MIN_SCHED_CONTEXT_BITS: seL4_Word = 7;
    pub const SEL4_MCS_REPLY_BITS: seL4_Word = 5;
    pub const SEL4_MCS_NOTIFICATION_BITS: seL4_Word = 6;
    pub const SEL4_MCS_TIMEOUT_DATA: seL4_Word = 0;
    pub const SEL4_MCS_TIMEOUT_CONSUMED: seL4_Word = 1;
    pub const SEL4_MCS_TIMEOUT_LENGTH: seL4_Word = 2;
    pub const SEL4_MCS_FAULT_TIMEOUT_LABEL: seL4_Word = 5;
    pub const SEL4_MCS_FAULT_VM_LABEL: seL4_Word = 6;

    #[repr(usize)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum seL4_ObjectType {
        seL4_UntypedObject = seL4_UntypedObject as usize,
        seL4_TCBObject = seL4_TCBObject as usize,
        seL4_EndpointObject = seL4_EndpointObject as usize,
        seL4_NotificationObject = seL4_NotificationObject as usize,
        seL4_CapTableObject = seL4_CapTableObject as usize,
        seL4_ARM_HugePageObject = seL4_ARM_HugePageObject as usize,
        seL4_ARM_VSpaceObject = seL4_ARM_VSpaceObject as usize,
        seL4_ARM_Page = seL4_ARM_SmallPageObject as usize,
        seL4_ARM_LargePage = seL4_ARM_LargePageObject as usize,
        seL4_ARM_PageTableObject = seL4_ARM_PageTableObject as usize,
    }

    pub const seL4_UntypedObjectType: seL4_ObjectType = seL4_ObjectType::seL4_UntypedObject;
    pub const seL4_TCBObjectType: seL4_ObjectType = seL4_ObjectType::seL4_TCBObject;
    pub const seL4_EndpointObjectType: seL4_ObjectType = seL4_ObjectType::seL4_EndpointObject;
    pub const seL4_NotificationObjectType: seL4_ObjectType =
        seL4_ObjectType::seL4_NotificationObject;
    pub const seL4_CapTableObjectType: seL4_ObjectType = seL4_ObjectType::seL4_CapTableObject;
    pub const seL4_ARM_PageObjectType: seL4_ObjectType = seL4_ObjectType::seL4_ARM_Page;
    pub const seL4_ARM_LargePageObjectType: seL4_ObjectType = seL4_ObjectType::seL4_ARM_LargePage;
    pub const seL4_ARM_PageTableObjectType: seL4_ObjectType =
        seL4_ObjectType::seL4_ARM_PageTableObject;
    pub const seL4_ARM_VSpaceObjectType: seL4_ObjectType = seL4_ObjectType::seL4_ARM_VSpaceObject;

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
    pub type seL4_Bool = i8;
    pub type seL4_Uint8 = u8;
    pub type seL4_Uint32 = u32;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct seL4_SchedContext_Consumed {
        pub error: core::ffi::c_int,
        pub consumed: seL4_Time,
    }

    pub type seL4_SchedContext_Consumed_t = seL4_SchedContext_Consumed;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct seL4_SchedContext_YieldTo {
        pub error: core::ffi::c_int,
        pub consumed: seL4_Time,
    }

    pub type seL4_SchedContext_YieldTo_t = seL4_SchedContext_YieldTo;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_UserContext_ {
        pub pc: seL4_Word,
        pub sp: seL4_Word,
        pub spsr: seL4_Word,
        pub x0: seL4_Word,
        pub x1: seL4_Word,
        pub x2: seL4_Word,
        pub x3: seL4_Word,
        pub x4: seL4_Word,
        pub x5: seL4_Word,
        pub x6: seL4_Word,
        pub x7: seL4_Word,
        pub x8: seL4_Word,
        pub x16: seL4_Word,
        pub x17: seL4_Word,
        pub x18: seL4_Word,
        pub x29: seL4_Word,
        pub x30: seL4_Word,
        pub x9: seL4_Word,
        pub x10: seL4_Word,
        pub x11: seL4_Word,
        pub x12: seL4_Word,
        pub x13: seL4_Word,
        pub x14: seL4_Word,
        pub x15: seL4_Word,
        pub x19: seL4_Word,
        pub x20: seL4_Word,
        pub x21: seL4_Word,
        pub x22: seL4_Word,
        pub x23: seL4_Word,
        pub x24: seL4_Word,
        pub x25: seL4_Word,
        pub x26: seL4_Word,
        pub x27: seL4_Word,
        pub x28: seL4_Word,
        pub tpidr_el0: seL4_Word,
        pub tpidrro_el0: seL4_Word,
    }

    impl seL4_UserContext_ {
        #[inline(always)]
        pub const fn zeroed() -> Self {
            Self {
                pc: 0,
                sp: 0,
                spsr: 0,
                x0: 0,
                x1: 0,
                x2: 0,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
                x8: 0,
                x16: 0,
                x17: 0,
                x18: 0,
                x29: 0,
                x30: 0,
                x9: 0,
                x10: 0,
                x11: 0,
                x12: 0,
                x13: 0,
                x14: 0,
                x15: 0,
                x19: 0,
                x20: 0,
                x21: 0,
                x22: 0,
                x23: 0,
                x24: 0,
                x25: 0,
                x26: 0,
                x27: 0,
                x28: 0,
                tpidr_el0: 0,
                tpidrro_el0: 0,
            }
        }
    }

    impl Default for seL4_UserContext_ {
        fn default() -> Self {
            Self::zeroed()
        }
    }

    pub type seL4_UserContext = seL4_UserContext_;

    #[inline(always)]
    fn aarch64_user_context_register(regs: &seL4_UserContext, index: seL4_Word) -> seL4_Word {
        match index {
            0 => regs.pc,
            1 => regs.sp,
            2 => regs.spsr,
            3 => regs.x0,
            4 => regs.x1,
            5 => regs.x2,
            6 => regs.x3,
            7 => regs.x4,
            8 => regs.x5,
            9 => regs.x6,
            10 => regs.x7,
            11 => regs.x8,
            12 => regs.x16,
            13 => regs.x17,
            14 => regs.x18,
            15 => regs.x29,
            16 => regs.x30,
            17 => regs.x9,
            18 => regs.x10,
            19 => regs.x11,
            20 => regs.x12,
            21 => regs.x13,
            22 => regs.x14,
            23 => regs.x15,
            24 => regs.x19,
            25 => regs.x20,
            26 => regs.x21,
            27 => regs.x22,
            28 => regs.x23,
            29 => regs.x24,
            30 => regs.x25,
            31 => regs.x26,
            32 => regs.x27,
            33 => regs.x28,
            34 => regs.tpidr_el0,
            35 => regs.tpidrro_el0,
            _ => 0,
        }
    }

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
            self.words[0] & 0x7f
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

    pub type seL4_UnknownSyscall_Msg = seL4_Word;
    pub const seL4_UnknownSyscall_X0: seL4_UnknownSyscall_Msg = 0;
    pub const seL4_UnknownSyscall_X1: seL4_UnknownSyscall_Msg = 1;
    pub const seL4_UnknownSyscall_X2: seL4_UnknownSyscall_Msg = 2;
    pub const seL4_UnknownSyscall_X3: seL4_UnknownSyscall_Msg = 3;
    pub const seL4_UnknownSyscall_X4: seL4_UnknownSyscall_Msg = 4;
    pub const seL4_UnknownSyscall_X5: seL4_UnknownSyscall_Msg = 5;
    pub const seL4_UnknownSyscall_X6: seL4_UnknownSyscall_Msg = 6;
    pub const seL4_UnknownSyscall_X7: seL4_UnknownSyscall_Msg = 7;
    pub const seL4_UnknownSyscall_FaultIP: seL4_UnknownSyscall_Msg = 8;
    pub const seL4_UnknownSyscall_SP: seL4_UnknownSyscall_Msg = 9;
    pub const seL4_UnknownSyscall_LR: seL4_UnknownSyscall_Msg = 10;
    pub const seL4_UnknownSyscall_SPSR: seL4_UnknownSyscall_Msg = 11;
    pub const seL4_UnknownSyscall_Syscall: seL4_UnknownSyscall_Msg = 12;
    pub const seL4_UnknownSyscall_Length: seL4_UnknownSyscall_Msg = 13;

    pub type seL4_UserException_Msg = seL4_Word;
    pub const seL4_UserException_FaultIP: seL4_UserException_Msg = 0;
    pub const seL4_UserException_SP: seL4_UserException_Msg = 1;
    pub const seL4_UserException_SPSR: seL4_UserException_Msg = 2;
    pub const seL4_UserException_Number: seL4_UserException_Msg = 3;
    pub const seL4_UserException_Code: seL4_UserException_Msg = 4;
    pub const seL4_UserException_Length: seL4_UserException_Msg = 5;

    pub type seL4_VMFault_Msg = seL4_Word;
    pub const seL4_VMFault_IP: seL4_VMFault_Msg = 0;
    pub const seL4_VMFault_Addr: seL4_VMFault_Msg = 1;
    pub const seL4_VMFault_PrefetchFault: seL4_VMFault_Msg = 2;
    pub const seL4_VMFault_FSR: seL4_VMFault_Msg = 3;
    pub const seL4_VMFault_Length: seL4_VMFault_Msg = 4;

    pub type seL4_CapFault_Msg = seL4_Word;
    pub const seL4_CapFault_IP: seL4_CapFault_Msg = 0;
    pub const seL4_CapFault_Addr: seL4_CapFault_Msg = 1;
    pub const seL4_CapFault_InRecvPhase: seL4_CapFault_Msg = 2;
    pub const seL4_CapFault_LookupFailureType: seL4_CapFault_Msg = 3;
    pub const seL4_CapFault_BitsLeft: seL4_CapFault_Msg = 4;
    pub const seL4_CapFault_DepthMismatch_BitsFound: seL4_CapFault_Msg = 5;
    pub const seL4_CapFault_GuardMismatch_GuardFound: seL4_CapFault_Msg = 5;
    pub const seL4_CapFault_GuardMismatch_BitsFound: seL4_CapFault_Msg = 6;

    #[derive(Clone, Copy)]
    pub struct seL4_CNode_CapData;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct seL4_IPCBuffer {
        pub tag: seL4_MessageInfo,
        pub msg: [seL4_Word; 120],
        pub userData: seL4_Word,
        pub caps_or_badges: [seL4_Word; 4],
        pub receiveCNode: seL4_CPtr,
        pub receiveIndex: seL4_CPtr,
        pub receiveDepth: seL4_Word,
    }

    impl seL4_IPCBuffer {
        pub const fn new() -> Self {
            Self {
                tag: seL4_MessageInfo::new(0, 0, 0, 0),
                msg: [0; 120],
                userData: 0,
                caps_or_badges: [0; 4],
                receiveCNode: 0,
                receiveIndex: 0,
                receiveDepth: 0,
            }
        }
    }

    impl Default for seL4_IPCBuffer {
        fn default() -> Self {
            Self::new()
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct seL4_ARM_VMAttributes(pub seL4_Word);

    pub const seL4_ARM_PageCacheable: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x01);
    pub const seL4_ARM_ParityEnabled: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x02);
    pub const seL4_ARM_ExecuteNever: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x04);
    pub const seL4_ARM_Page_Uncached: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0);
    pub const seL4_ARM_Page_Default: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(0x03);

    pub const invocation_label_nInvocationLabels: seL4_Word = 31;
    pub const ARMVSpaceClean_Data: seL4_Word = invocation_label_nInvocationLabels;
    pub const ARMVSpaceInvalidate_Data: seL4_Word = ARMVSpaceClean_Data + 1;
    pub const ARMVSpaceCleanInvalidate_Data: seL4_Word = ARMVSpaceClean_Data + 2;
    pub const ARMVSpaceUnify_Instruction: seL4_Word = ARMVSpaceClean_Data + 3;
    pub const nSeL4ArchInvocationLabels: seL4_Word = ARMVSpaceClean_Data + 5;

    pub type seL4_CapData_t = seL4_CNode_CapData;

    static mut HOST_IPC_BUFFER: seL4_IPCBuffer = seL4_IPCBuffer::new();

    #[inline(always)]
    unsafe fn ensure_ipc_buffer() -> *mut seL4_IPCBuffer {
        if __sel4_ipc_buffer.is_null() {
            __sel4_ipc_buffer = ptr::addr_of_mut!(HOST_IPC_BUFFER);
        }
        __sel4_ipc_buffer
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
        pub initThreadCNodeSizeBits: u8,
        pub _padding_init_cnode_bits: [u8; size_of::<seL4_Word>() - 1],
        pub initThreadDomain: seL4_Word,
        pub untyped: seL4_SlotRegion,
        pub untypedList: [seL4_UntypedDesc; MAX_BOOTINFO_UNTYPEDS],
    }

    #[inline(always)]
    pub unsafe fn seL4_InitBootInfo(bi: *mut seL4_BootInfo) {
        bootinfo = bi;
        if !bi.is_null() {
            seL4_SetIPCBuffer((*bi).ipcBuffer);
        } else {
            seL4_SetIPCBuffer(ptr::null_mut());
        }
    }

    #[export_name = "seL4_GetBootInfo"]
    pub unsafe extern "C" fn sel4_get_bootinfo() -> *mut seL4_BootInfo {
        bootinfo
    }

    #[inline(always)]
    pub unsafe fn seL4_GetBootInfo() -> *mut seL4_BootInfo {
        sel4_get_bootinfo()
    }

    #[inline(always)]
    fn unsupported_error() -> seL4_Error {
        unsupported();
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Copy(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Word,
        _rights: seL4_CapRights,
    ) -> seL4_Error {
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Delete(
        _root: seL4_CNode,
        _index: seL4_Word,
        _depth: seL4_Word,
    ) -> seL4_Error {
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Revoke(
        _root: seL4_CNode,
        _index: seL4_Word,
        _depth: seL4_Word,
    ) -> seL4_Error {
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Move(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Word,
    ) -> seL4_Error {
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_CNode_Mint(
        _dest_root: seL4_CNode,
        _dest_index: seL4_Word,
        _dest_depth: seL4_Word,
        _src_root: seL4_CNode,
        _src_index: seL4_Word,
        _src_depth: seL4_Word,
        _rights: seL4_CapRights_t,
        _badge: seL4_Word,
    ) -> seL4_Error {
        seL4_NoError
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
        seL4_NoError
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
        // Host tests have no kernel debug console.
    }

    #[inline(always)]
    pub fn debug_put_char(_c: u8) {
        // Host tests have no kernel debug console.
    }

    #[inline(always)]
    pub fn debug_halt() {
        // Host tests have no kernel halt syscall.
    }

    #[inline(always)]
    pub fn seL4_DebugDumpScheduler() {
        // Host tests have no kernel scheduler state.
    }

    #[inline(always)]
    pub fn seL4_DebugDumpCPUInfo() {
        // Host tests have no kernel CPU debug state.
    }

    #[inline(always)]
    pub unsafe fn seL4_CapIdentify(_cap: seL4_CPtr) -> seL4_Word {
        seL4_CapNull
    }

    #[inline(always)]
    pub fn seL4_Yield() {
        // Cooperative host tests do not model kernel scheduling.
    }

    #[inline(always)]
    pub unsafe fn seL4_Send(_dest: seL4_CPtr, _msg: seL4_MessageInfo) {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = _msg;
    }

    #[inline(always)]
    pub unsafe fn seL4_Recv(_src: seL4_CPtr, _sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        if !_sender_badge.is_null() {
            *_sender_badge = 0;
        }
        (*ensure_ipc_buffer()).tag
    }

    #[inline(always)]
    pub unsafe fn seL4_Poll(_src: seL4_CPtr, _sender_badge: *mut seL4_Word) -> seL4_MessageInfo {
        if !_sender_badge.is_null() {
            *_sender_badge = 0;
        }
        seL4_MessageInfo::new(0, 0, 0, 0)
    }

    #[inline(always)]
    pub unsafe fn seL4_CallWithMRs(
        _dest: seL4_CPtr,
        msg: seL4_MessageInfo,
        _mr0: *mut seL4_Word,
        _mr1: *mut seL4_Word,
        _mr2: *mut seL4_Word,
        _mr3: *mut seL4_Word,
    ) -> seL4_MessageInfo {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = msg;
        if !_mr0.is_null() {
            (*ipc).msg[0] = *_mr0;
        }
        if !_mr1.is_null() {
            (*ipc).msg[1] = *_mr1;
        }
        if !_mr2.is_null() {
            (*ipc).msg[2] = *_mr2;
        }
        if !_mr3.is_null() {
            (*ipc).msg[3] = *_mr3;
        }
        msg
    }

    #[inline(always)]
    pub unsafe fn seL4_SetCap(index: i32, cptr: seL4_CPtr) {
        let ipc = ensure_ipc_buffer();
        if let Some(slot) = (*ipc).caps_or_badges.get_mut(index as usize) {
            *slot = cptr;
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_SetMR(index: seL4_Word, value: seL4_Word) {
        let ipc = ensure_ipc_buffer();
        if let Some(slot) = (*ipc).msg.get_mut(index as usize) {
            *slot = value;
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_GetMR(index: seL4_Word) -> seL4_Word {
        let ipc = ensure_ipc_buffer();
        (*ipc).msg.get(index as usize).copied().unwrap_or(0)
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
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetFaultHandler(
        _tcb: seL4_TCB,
        _fault_handler: seL4_CPtr,
        _cspace_root: seL4_CNode,
        _cspace_root_data: seL4_Word,
        _vspace_root: seL4_CPtr,
        _vspace_root_data: seL4_Word,
    ) -> seL4_Error {
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetIPCBuffer(
        _tcb: seL4_TCB,
        buffer_addr: seL4_Word,
        buffer_frame: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, buffer_frame);
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBSetIPCBuffer, 0, 1, 1);
        (*ipc).msg[0] = buffer_addr;
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_WriteRegisters(
        _service: seL4_TCB,
        resume_target: seL4_Bool,
        arch_flags: seL4_Uint8,
        count: seL4_Word,
        regs: *const seL4_UserContext,
    ) -> seL4_Error {
        if regs.is_null() {
            return seL4_InvalidArgument;
        }

        let regs = &*regs;
        let bounded_count = core::cmp::min(count, SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT);
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(
            invocation_label_TCBWriteRegisters,
            0,
            0,
            SEL4_TCB_WRITE_REGISTERS_MESSAGE_LENGTH,
        );
        (*ipc).msg[0] =
            ((resume_target as seL4_Word) & 0x1) | (((arch_flags as seL4_Word) & 0xff) << 8);
        (*ipc).msg[1] = count;
        // seL4's AArch64 fast-message ABI always carries PC and SP in mr2/mr3.
        // The kernel uses `count` to decide how many context words to consume.
        (*ipc).msg[2] = aarch64_user_context_register(regs, 0);
        (*ipc).msg[3] = aarch64_user_context_register(regs, 1);
        let mut register_index = 2;
        while register_index < bounded_count {
            (*ipc).msg[(register_index + 2) as usize] =
                aarch64_user_context_register(regs, register_index);
            register_index += 1;
        }
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetPriority(
        _service: seL4_TCB,
        authority: seL4_TCB,
        priority: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBSetPriority, 0, 1, 1);
        (*ipc).msg[0] = priority;
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSchedParams(
        _service: seL4_TCB,
        authority: seL4_TCB,
        mcp: seL4_Word,
        priority: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBSetSchedParams, 0, 1, 2);
        (*ipc).msg[0] = mcp;
        (*ipc).msg[1] = priority;
        seL4_NoError
    }

    /// Host-side recorder for the seL4 16 MCS `TCB_SetSchedParams` shape.
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetSchedParamsMcs(
        _service: seL4_TCB,
        authority: seL4_TCB,
        mcp: seL4_Word,
        priority: seL4_Word,
        sched_context: seL4_SchedContext,
        fault_ep: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, authority);
        seL4_SetCap(1, sched_context);
        seL4_SetCap(2, fault_ep);
        host_record_mcs_invocation(invocation_label_TCBSetSchedParams, 3, &[mcp, priority]);
        seL4_NoError
    }

    /// Host-side recorder for MCS timeout-endpoint configuration.
    #[inline(always)]
    pub unsafe fn seL4_TCB_SetTimeoutEndpoint(
        _service: seL4_TCB,
        timeout_fault_ep: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, timeout_fault_ep);
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_TCB_SET_TIMEOUT_ENDPOINT, 1, &[]);
        seL4_NoError
    }

    /// Host-side recorder for MCS scheduling-context configuration.
    #[inline(always)]
    pub unsafe fn seL4_SchedControl_ConfigureFlags(
        _service: seL4_SchedControl,
        sched_context: seL4_SchedContext,
        budget: seL4_Time,
        period: seL4_Time,
        extra_refills: seL4_Word,
        badge: seL4_Word,
        flags: seL4_Word,
    ) -> seL4_Error {
        seL4_SetCap(0, sched_context);
        host_record_mcs_invocation(
            MCS_INVOCATION_LABEL_SCHED_CONTROL_CONFIGURE_FLAGS,
            1,
            &[budget, period, extra_refills, badge, flags],
        );
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Bind(
        _service: seL4_SchedContext,
        cap: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, cap);
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_SCHED_CONTEXT_BIND, 1, &[]);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Unbind(_service: seL4_SchedContext) -> seL4_Error {
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND, 0, &[]);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_SchedContext_UnbindObject(
        _service: seL4_SchedContext,
        cap: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, cap);
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND_OBJECT, 1, &[]);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_SchedContext_Consumed(
        _service: seL4_SchedContext,
    ) -> seL4_SchedContext_Consumed_t {
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_SCHED_CONTEXT_CONSUMED, 0, &[]);
        seL4_SchedContext_Consumed {
            error: seL4_NoError as core::ffi::c_int,
            consumed: 0,
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_SchedContext_YieldTo(
        _service: seL4_SchedContext,
    ) -> seL4_SchedContext_YieldTo_t {
        host_record_mcs_invocation(MCS_INVOCATION_LABEL_SCHED_CONTEXT_YIELD_TO, 0, &[]);
        seL4_SchedContext_YieldTo {
            error: seL4_NoError as core::ffi::c_int,
            consumed: 0,
        }
    }

    #[inline(always)]
    unsafe fn host_record_mcs_invocation(
        label: seL4_Word,
        extra_caps: seL4_Word,
        message: &[seL4_Word],
    ) {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(label, 0, extra_caps, message.len() as seL4_Word);
        for (index, value) in message.iter().copied().enumerate() {
            if let Some(slot) = (*ipc).msg.get_mut(index) {
                *slot = value;
            }
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_Suspend(_service: seL4_TCB) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBSuspend, 0, 0, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_Resume(_service: seL4_TCB) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBResume, 0, 0, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_BindNotification(
        _service: seL4_TCB,
        notification: seL4_CPtr,
    ) -> seL4_Error {
        seL4_SetCap(0, notification);
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBBindNotification, 0, 1, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_TCB_UnbindNotification(_service: seL4_TCB) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(invocation_label_TCBUnbindNotification, 0, 0, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub fn seL4_SetIPCBuffer(buf: *mut seL4_IPCBuffer) {
        unsafe {
            __sel4_ipc_buffer = if buf.is_null() {
                ptr::addr_of_mut!(HOST_IPC_BUFFER)
            } else {
                buf
            };
        }
    }

    #[inline(always)]
    pub unsafe fn seL4_GetIPCBuffer() -> *mut seL4_IPCBuffer {
        ensure_ipc_buffer()
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Map(
        _page: seL4_ARM_Page,
        vspace: seL4_VSpace,
        vaddr: seL4_Word,
        rights: seL4_CapRights_t,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMPageMap, 0, 1, 3);
        (*ipc).caps_or_badges[0] = vspace;
        (*ipc).msg[0] = vaddr;
        (*ipc).msg[1] = rights.raw();
        (*ipc).msg[2] = attr.0;
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Map(
        _pt: seL4_ARM_PageTable,
        vspace: seL4_VSpace,
        vaddr: seL4_Word,
        attr: seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMPageTableMap, 0, 1, 2);
        (*ipc).caps_or_badges[0] = vspace;
        (*ipc).msg[0] = vaddr;
        (*ipc).msg[1] = attr.0;
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_PageTable_Unmap(_pt: seL4_ARM_PageTable) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMPageTableUnmap, 0, 0, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_Page_Unmap(_page: seL4_ARM_Page) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMPageUnmap, 0, 0, 0);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_ASIDControl_MakePool(
        _service: seL4_ARM_ASIDControl,
        untyped: seL4_Untyped,
        root: seL4_CNode,
        index: seL4_Word,
        depth: u8,
    ) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMASIDControlMakePool, 0, 2, 2);
        (*ipc).caps_or_badges[0] = untyped;
        (*ipc).caps_or_badges[1] = root;
        (*ipc).msg[0] = index;
        (*ipc).msg[1] = seL4_Word::from(depth);
        seL4_NoError
    }

    #[inline(always)]
    pub unsafe fn seL4_ARM_ASIDPool_Assign(
        _service: seL4_ARM_ASIDPool,
        vspace: seL4_CPtr,
    ) -> seL4_Error {
        let ipc = ensure_ipc_buffer();
        (*ipc).tag = seL4_MessageInfo::new(arch_invocation_label_ARMASIDPoolAssign, 0, 1, 0);
        (*ipc).caps_or_badges[0] = vspace;
        seL4_NoError
    }

    #[inline(always)]
    pub fn yield_now() {
        // Cooperative host tests do not model kernel scheduling.
    }

    #[inline(always)]
    pub unsafe fn seL4_DebugHalt() {
        unsupported();
    }
}

#[cfg(not(target_os = "none"))]
pub use imp::*;

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static HOST_IPC_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn host_ipc_tag() -> seL4_MessageInfo {
        // SAFETY: Host tests use the crate-owned synthetic IPC buffer.
        let ipc = unsafe { seL4_GetIPCBuffer() };
        // SAFETY: `seL4_GetIPCBuffer` returns the synthetic host IPC buffer for host tests.
        unsafe { (*ipc).tag }
    }

    fn host_cap(index: usize) -> seL4_CPtr {
        // SAFETY: Host tests use the crate-owned synthetic IPC buffer.
        let ipc = unsafe { seL4_GetIPCBuffer() };
        // SAFETY: The synthetic IPC buffer has a fixed cap array and tests pass valid indices.
        unsafe { (*ipc).caps_or_badges[index] }
    }

    fn host_mr(index: usize) -> seL4_Word {
        // SAFETY: Host tests use the crate-owned synthetic IPC buffer.
        let ipc = unsafe { seL4_GetIPCBuffer() };
        // SAFETY: The synthetic IPC buffer has a fixed message array and tests pass valid indices.
        unsafe { (*ipc).msg[index] }
    }

    fn patterned_user_context() -> seL4_UserContext {
        seL4_UserContext {
            pc: 0x100,
            sp: 0x101,
            spsr: 0x102,
            x0: 0x103,
            x1: 0x104,
            x2: 0x105,
            x3: 0x106,
            x4: 0x107,
            x5: 0x108,
            x6: 0x109,
            x7: 0x10a,
            x8: 0x10b,
            x16: 0x10c,
            x17: 0x10d,
            x18: 0x10e,
            x29: 0x10f,
            x30: 0x110,
            x9: 0x111,
            x10: 0x112,
            x11: 0x113,
            x12: 0x114,
            x13: 0x115,
            x14: 0x116,
            x15: 0x117,
            x19: 0x118,
            x20: 0x119,
            x21: 0x11a,
            x22: 0x11b,
            x23: 0x11c,
            x24: 0x11d,
            x25: 0x11e,
            x26: 0x11f,
            x27: 0x120,
            x28: 0x121,
            tpidr_el0: 0x122,
            tpidrro_el0: 0x123,
        }
    }

    #[test]
    fn aarch64_host_object_numbers_match_generated_kernel_layout() {
        assert_eq!(seL4_ARM_HugePageObject, 5);
        assert_eq!(seL4_ARM_VSpaceObject as seL4_Word, 6);
        assert_eq!(seL4_ARM_SmallPageObject as seL4_Word, 7);
        assert_eq!(seL4_ARM_LargePageObject as seL4_Word, 8);
        assert_eq!(seL4_ARM_PageTableObject as seL4_Word, 9);
        assert_eq!(seL4_VSpaceBits, 12);
        assert_eq!(seL4_ASIDPoolBits, 12);
    }

    #[test]
    fn aarch64_user_context_register_count_matches_layout() {
        assert_eq!(
            core::mem::size_of::<seL4_UserContext>() / core::mem::size_of::<seL4_Word>(),
            SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT as usize
        );
    }

    #[test]
    fn tcb_write_registers_respects_v16_count_and_static_message_length() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        let sentinel = 0xfeed_face_cafe_beef;
        for index in 4..=37 {
            // SAFETY: Host tests use the crate-owned synthetic IPC buffer and valid MR indices.
            unsafe { seL4_SetMR(index, sentinel) };
        }
        let regs = patterned_user_context();

        // SAFETY: The context is initialized and remains live for the synthetic host invocation.
        let result = unsafe { seL4_TCB_WriteRegisters(0x44, 1, 0x5a, 3, &regs) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBWriteRegisters);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 2 + SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT);
        assert_eq!(host_mr(0), 1 | (0x5a << 8));
        assert_eq!(host_mr(1), 3);
        assert_eq!(host_mr(2), regs.pc);
        assert_eq!(host_mr(3), regs.sp);
        assert_eq!(host_mr(4), regs.spsr);
        assert_eq!(host_mr(5), sentinel);
        assert_eq!(host_mr(37), sentinel);
    }

    #[test]
    fn tcb_write_registers_bounds_invalid_count_to_aarch64_context() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        let sentinel = 0xfeed_face_cafe_beef;
        // SAFETY: Host tests use the crate-owned synthetic IPC buffer and a valid MR index.
        unsafe { seL4_SetMR(38, sentinel) };
        let regs = patterned_user_context();
        let invalid_count = SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT + 1;

        // SAFETY: The context is initialized and remains live for the synthetic host invocation.
        let result =
            unsafe { seL4_TCB_WriteRegisters(0x44, 0, 0, invalid_count, &regs as *const _) };
        assert_eq!(result, seL4_NoError);
        assert_eq!(host_mr(1), invalid_count);
        assert_eq!(host_mr(37), regs.tpidrro_el0);
        assert_eq!(host_mr(38), sentinel);
    }

    #[test]
    fn tcb_set_ipc_buffer_uses_v16_invocation_shape() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_SetIPCBuffer(0x44, 0x8000_0000, 0x55) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBSetIPCBuffer);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 1);
        assert_eq!(host_cap(0), 0x55);
        assert_eq!(host_mr(0), 0x8000_0000);
    }

    #[test]
    fn tcb_set_sched_params_uses_v16_invocation_shape() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_SetSchedParams(0x44, 0x01, 220, 200) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBSetSchedParams);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 2);
        assert_eq!(host_cap(0), 0x01);
        assert_eq!(host_mr(0), 220);
        assert_eq!(host_mr(1), 200);
    }

    #[test]
    fn mcs_contract_matches_sel4_16_aarch64_layout() {
        assert_eq!(invocation_label_TCBSetSchedParams, 8);
        assert_eq!(MCS_INVOCATION_LABEL_TCB_SET_TIMEOUT_ENDPOINT, 9);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTROL_CONFIGURE_FLAGS, 33);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTEXT_BIND, 34);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND, 35);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND_OBJECT, 36);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTEXT_CONSUMED, 37);
        assert_eq!(MCS_INVOCATION_LABEL_SCHED_CONTEXT_YIELD_TO, 38);
        assert_eq!(MCS_INVOCATION_LABEL_COUNT, 39);

        assert_eq!(SEL4_MCS_SCHED_CONTEXT_OBJECT, 5);
        assert_eq!(SEL4_MCS_REPLY_OBJECT, 6);
        assert_eq!(SEL4_MCS_MIN_SCHED_CONTEXT_BITS, 7);
        assert_eq!(SEL4_MCS_REPLY_BITS, 5);
        assert_eq!(SEL4_MCS_NOTIFICATION_BITS, 6);
        assert_eq!(SEL4_MCS_TIMEOUT_DATA, 0);
        assert_eq!(SEL4_MCS_TIMEOUT_CONSUMED, 1);
        assert_eq!(SEL4_MCS_TIMEOUT_LENGTH, 2);
        assert_eq!(SEL4_MCS_FAULT_TIMEOUT_LABEL, 5);
        assert_eq!(SEL4_MCS_FAULT_VM_LABEL, 6);

        let core_sched_context_bytes = 128usize;
        let refill_bytes = 16usize;
        assert_eq!(((1usize << 7) - core_sched_context_bytes) / refill_bytes, 0);
        assert_eq!(((1usize << 8) - core_sched_context_bytes) / refill_bytes, 8);
        assert_eq!(core::mem::size_of::<seL4_SchedContext_Consumed>(), 16);
        assert_eq!(core::mem::size_of::<seL4_SchedContext_YieldTo>(), 16);
    }

    #[test]
    fn mcs_syscall_ids_match_sel4_16_aarch64_order() {
        assert_eq!(SEL4_MCS_SYS_CALL, -1);
        assert_eq!(SEL4_MCS_SYS_REPLY_RECV, -2);
        assert_eq!(SEL4_MCS_SYS_NB_SEND_RECV, -3);
        assert_eq!(SEL4_MCS_SYS_NB_SEND_WAIT, -4);
        assert_eq!(SEL4_MCS_SYS_SEND, -5);
        assert_eq!(SEL4_MCS_SYS_NB_SEND, -6);
        assert_eq!(SEL4_MCS_SYS_RECV, -7);
        assert_eq!(SEL4_MCS_SYS_NB_RECV, -8);
        assert_eq!(SEL4_MCS_SYS_WAIT, -9);
        assert_eq!(SEL4_MCS_SYS_NB_WAIT, -10);
        assert_eq!(SEL4_MCS_SYS_YIELD, -11);
    }

    #[test]
    fn mcs_tcb_setup_uses_explicit_sched_context_and_fault_caps() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs only record the exact invocation shape.
        let result = unsafe { seL4_TCB_SetSchedParamsMcs(0x44, 0x01, 220, 200, 0x90, 0x91) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBSetSchedParams);
        assert_eq!(tag.extra_caps(), 3);
        assert_eq!(tag.length(), 2);
        assert_eq!(host_cap(0), 0x01);
        assert_eq!(host_cap(1), 0x90);
        assert_eq!(host_cap(2), 0x91);
        assert_eq!(host_mr(0), 220);
        assert_eq!(host_mr(1), 200);

        // SAFETY: Host stubs only record the exact invocation shape.
        let result = unsafe { seL4_TCB_SetTimeoutEndpoint(0x44, 0x92) };
        assert_eq!(result, seL4_NoError);
        let tag = host_ipc_tag();
        assert_eq!(tag.label(), MCS_INVOCATION_LABEL_TCB_SET_TIMEOUT_ENDPOINT);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 0);
        assert_eq!(host_cap(0), 0x92);
    }

    #[test]
    fn mcs_sched_control_configure_flags_preserves_fifth_mr() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs only record the exact invocation shape.
        let result =
            unsafe { seL4_SchedControl_ConfigureFlags(0x80, 0x90, 2_000, 10_000, 4, 0xa5, 0x5a) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(
            tag.label(),
            MCS_INVOCATION_LABEL_SCHED_CONTROL_CONFIGURE_FLAGS
        );
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 5);
        assert_eq!(host_cap(0), 0x90);
        assert_eq!(host_mr(0), 2_000);
        assert_eq!(host_mr(1), 10_000);
        assert_eq!(host_mr(2), 4);
        assert_eq!(host_mr(3), 0xa5);
        assert_eq!(host_mr(4), 0x5a);
    }

    #[test]
    fn mcs_sched_context_object_invocations_use_exact_caps_and_lengths() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();

        // SAFETY: Host stubs only record the exact invocation shape.
        assert_eq!(unsafe { seL4_SchedContext_Bind(0x90, 0x44) }, seL4_NoError);
        let tag = host_ipc_tag();
        assert_eq!(tag.label(), MCS_INVOCATION_LABEL_SCHED_CONTEXT_BIND);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 0);
        assert_eq!(host_cap(0), 0x44);

        // SAFETY: Host stubs only record the exact invocation shape.
        assert_eq!(unsafe { seL4_SchedContext_Unbind(0x90) }, seL4_NoError);
        let tag = host_ipc_tag();
        assert_eq!(tag.label(), MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);

        // SAFETY: Host stubs only record the exact invocation shape.
        assert_eq!(
            unsafe { seL4_SchedContext_UnbindObject(0x90, 0x44) },
            seL4_NoError
        );
        let tag = host_ipc_tag();
        assert_eq!(
            tag.label(),
            MCS_INVOCATION_LABEL_SCHED_CONTEXT_UNBIND_OBJECT
        );
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 0);
        assert_eq!(host_cap(0), 0x44);

        // SAFETY: Host stubs only record the exact invocation shape.
        let consumed = unsafe { seL4_SchedContext_Consumed(0x90) };
        assert_eq!(consumed.error, seL4_NoError as core::ffi::c_int);
        assert_eq!(consumed.consumed, 0);
        let tag = host_ipc_tag();
        assert_eq!(tag.label(), MCS_INVOCATION_LABEL_SCHED_CONTEXT_CONSUMED);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);

        // SAFETY: Host stubs only record the exact invocation shape.
        let yielded = unsafe { seL4_SchedContext_YieldTo(0x90) };
        assert_eq!(yielded.error, seL4_NoError as core::ffi::c_int);
        assert_eq!(yielded.consumed, 0);
        let tag = host_ipc_tag();
        assert_eq!(tag.label(), MCS_INVOCATION_LABEL_SCHED_CONTEXT_YIELD_TO);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);
    }

    #[test]
    fn tcb_set_priority_uses_v16_invocation_shape() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_SetPriority(0x44, 0x01, 240) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBSetPriority);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 1);
        assert_eq!(host_cap(0), 0x01);
        assert_eq!(host_mr(0), 240);
    }

    #[test]
    fn tcb_bind_and_unbind_notification_use_v16_invocation_shape() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_BindNotification(0x44, 0x99) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBBindNotification);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 0);
        assert_eq!(host_cap(0), 0x99);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_UnbindNotification(0x44) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBUnbindNotification);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);
    }

    #[test]
    fn tcb_suspend_resume_use_v16_invocation_shape() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_Suspend(0x44) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBSuspend);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_TCB_Resume(0x44) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), invocation_label_TCBResume);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);
    }

    #[test]
    fn arm_page_map_and_unmap_use_kernel_invocation_shapes() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        let rights = seL4_CapRights_t::new(0, 1, 1, 0);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result =
            unsafe { seL4_ARM_Page_Map(0x22, 0x33, 0x4000_0000, rights, seL4_ARM_PageCacheable) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMPageMap);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 3);
        assert_eq!(host_cap(0), 0x33);
        assert_eq!(host_mr(0), 0x4000_0000);
        assert_eq!(host_mr(1), rights.raw());
        assert_eq!(host_mr(2), seL4_ARM_PageCacheable.0);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_ARM_Page_Unmap(0x22) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMPageUnmap);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);
    }

    #[test]
    fn arm_page_table_map_and_unmap_use_kernel_invocation_shapes() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result =
            unsafe { seL4_ARM_PageTable_Map(0x44, 0x55, 0x8000_0000, seL4_ARM_Page_Default) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMPageTableMap);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 2);
        assert_eq!(host_cap(0), 0x55);
        assert_eq!(host_mr(0), 0x8000_0000);
        assert_eq!(host_mr(1), seL4_ARM_Page_Default.0);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_ARM_PageTable_Unmap(0x44) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMPageTableUnmap);
        assert_eq!(tag.extra_caps(), 0);
        assert_eq!(tag.length(), 0);
    }

    #[test]
    fn arm_asid_pool_calls_use_kernel_invocation_shapes() {
        let _guard = HOST_IPC_TEST_LOCK.lock().unwrap();
        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_ARM_ASIDControl_MakePool(0x60, 0x61, 0x62, 0x63, 64) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMASIDControlMakePool);
        assert_eq!(tag.extra_caps(), 2);
        assert_eq!(tag.length(), 2);
        assert_eq!(host_cap(0), 0x61);
        assert_eq!(host_cap(1), 0x62);
        assert_eq!(host_mr(0), 0x63);
        assert_eq!(host_mr(1), 64);

        // SAFETY: Host stubs do not cross a kernel boundary; this records the invocation shape.
        let result = unsafe { seL4_ARM_ASIDPool_Assign(0x70, 0x71) };
        assert_eq!(result, seL4_NoError);

        let tag = host_ipc_tag();
        assert_eq!(tag.label(), arch_invocation_label_ARMASIDPoolAssign);
        assert_eq!(tag.extra_caps(), 1);
        assert_eq!(tag.length(), 0);
        assert_eq!(host_cap(0), 0x71);
    }
}
