// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Console parser and interactive shell for the root task.
// Author: Lukas Bower

#![allow(unsafe_code)]

//! Shared console command parser and rate limiter for the root task console.

pub mod proto;

#[cfg(feature = "kernel")]
mod io;
#[cfg(feature = "kernel")]
pub use io::Console;

pub use cohsh_core::{
    Command, CommandParser, ConsoleError, MAX_LINE_LEN, MAX_ROLE_LEN, MAX_TICKET_LEN,
};

use crate::platform::Platform;
#[cfg(feature = "kernel")]
use crate::sel4::BootInfoExt;

#[cfg(feature = "canonical_cspace")]
use crate::sel4;

#[cfg(feature = "kernel")]
use core::fmt::{self, Write as FmtWrite};
#[cfg(feature = "kernel")]
use heapless::String;
#[cfg(feature = "kernel")]
use sel4_sys::seL4_CPtr;

#[cfg(feature = "canonical_cspace")]
pub fn start(ep_slot: u32, _bi: &sel4_sys::seL4_BootInfo) {
    log::info!("[console] ready on endpoint slot=0x{:04x}", ep_slot);
    loop {
        let msg = sel4::recv(ep_slot as sel4_sys::seL4_CPtr, core::ptr::null_mut());
        match msg {
            0 => log::info!("[console] recv: help | bi | ls-caps | echo"),
            _ => log::info!("[console] unknown verb id={}", msg),
        }
    }
}

/// Minimal console loop used during early kernel bring-up.
pub fn run<P: Platform>(platform: &P) -> ! {
    use core::fmt::Write as _;

    struct Adapter<'a, P: Platform>(&'a P);

    impl<'a, P: Platform> core::fmt::Write for Adapter<'a, P> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.as_bytes() {
                self.0.putc(*byte);
            }
            Ok(())
        }
    }

    let mut writer = Adapter(platform);
    let _ = writeln!(writer, "[cohesix:root-task] online");

    loop {
        if let Some(byte) = platform.getc_nonblock() {
            platform.putc(byte);
            if byte == b'\r' || byte == b'\n' {
                let _ = writeln!(writer);
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(feature = "kernel")]
pub struct CohesixConsole {
    console: Console,
    ep_slot: seL4_CPtr,
    uart_slot: seL4_CPtr,
    parser: CommandParser,
}

#[cfg(feature = "kernel")]
impl CohesixConsole {
    #[must_use]
    pub fn with_console(console: Console, ep_slot: seL4_CPtr, uart_slot: seL4_CPtr) -> Self {
        log::trace!(
            "[console] CohesixConsole::with_console created (ep_slot=0x{ep:04x}, uart_slot=0x{uart:04x})",
            ep = ep_slot,
            uart = uart_slot,
        );
        Self {
            console,
            ep_slot,
            uart_slot,
            parser: CommandParser::new(),
        }
    }

    fn emit(&mut self, text: &str) {
        let _ = self.console.write_str(text);
        self.console.flush();
    }

    fn emit_line(&mut self, text: &str) {
        self.emit(text);
        self.emit("\r\n");
    }

    fn prompt(&mut self) {
        log::info!("[console] writing prompt 'cohesix>'");
        self.emit("cohesix> ");
    }

    fn bootinfo(&self) -> &'static sel4_sys::seL4_BootInfo {
        unsafe { &*sel4_sys::seL4_GetBootInfo() }
    }

    fn print_help(&mut self) {
        self.emit_line("Commands:");
        for line in cohsh_core::help::ROOT_CONSOLE_HELP_LINES {
            self.emit_line(line);
        }
    }

    fn print_bootinfo(&mut self) {
        let bi = self.bootinfo();
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[bi] node_bits={} empty=[0x{:04x}..0x{:04x}) ",
            bi.initThreadCNodeSizeBits, bi.empty.start, bi.empty.end,
        );
        if let Some(ptr) = bi.ipc_buffer_ptr() {
            let addr = ptr.as_ptr() as usize;
            let width = core::mem::size_of::<usize>() * 2;
            let _ = write!(line, "ipc=0x{addr:0width$x}");
        } else {
            let _ = line.push_str("ipc=<none>");
        }
        self.emit_line(line.as_str());
    }

    fn print_caps(&mut self) {
        let bi = self.bootinfo();
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[caps] root=0x{:04x} ep=0x{:04x} uart=0x{:04x}",
            bi.init_cnode_cap(),
            self.ep_slot,
            self.uart_slot,
        );
        self.emit_line(line.as_str());
    }

    fn print_mem(&mut self) {
        let bi = self.bootinfo();
        let count = (bi.untyped.end - bi.untyped.start) as usize;
        let mut ram_ut = 0usize;
        for desc in bi.untypedList.iter().take(count) {
            if desc.isDevice == 0 {
                ram_ut += 1;
            }
        }
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[mem] untyped caps={} ram_ut={} device_ut={}",
            count,
            ram_ut,
            count.saturating_sub(ram_ut),
        );
        self.emit_line(line.as_str());
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Help => self.print_help(),
            Command::BootInfo => self.print_bootinfo(),
            Command::Caps => self.print_caps(),
            Command::Mem => self.print_mem(),
            Command::Ping => self.emit_line("pong"),
            Command::Test => self.emit_line("test not supported on root console"),
            Command::Quit => self.emit_line("quit not supported on root console"),
            Command::Log => self.emit_line("log streaming unavailable"),
            Command::NetTest | Command::NetStats => {
                self.emit_line("network commands not available on root console")
            }
            Command::CacheLog { count } => {
                let count = usize::from(count.unwrap_or(64));
                #[cfg(all(feature = "kernel", target_os = "none"))]
                {
                    crate::hal::cache::write_recent_ops(self, count);
                    self.console.flush();
                }
                #[cfg(not(target_os = "none"))]
                {
                    self.emit_line("cachelog unavailable on host targets");
                }
            }
            Command::Attach { .. }
            | Command::Tail { .. }
            | Command::Cat { .. }
            | Command::Ls { .. }
            | Command::Echo { .. }
            | Command::Spawn(_)
            | Command::Kill(_) => self.emit_line("command not implemented"),
        }
    }

    fn reset_parser(&mut self) {
        self.parser = CommandParser::new();
    }

    pub fn run(&mut self) -> ! {
        log::info!("[console] task entry: root console online, about to write prompt");
        log::info!("[console] root shell loop starting");
        log::info!(
            "[console] starting root shell ep=0x{ep:04x} uart=0x{uart:04x}",
            ep = self.ep_slot,
            uart = self.uart_slot,
        );
        self.emit_line("Cohesix console ready");
        log::info!("[console] writing initial prompt 'cohesix>' to serial");
        self.prompt();

        loop {
            let mut buffer = [0u8; MAX_LINE_LEN];
            let count = self.console.read_line(&mut buffer);
            let line = core::str::from_utf8(&buffer[..count])
                .unwrap_or("")
                .trim_matches(char::from(0))
                .trim();

            log::trace!("[console] received line bytes={} line=<{line}>", count);

            if let Err(err) = self.feed_line(line.as_bytes()) {
                let mut message = String::<128>::new();
                let _ = write!(message, "error: {err}");
                self.emit_line(message.as_str());
                self.reset_parser();
                continue;
            }

            match self.parser.push_byte(b'\n') {
                Ok(Some(command)) => {
                    self.handle_command(command);
                    self.reset_parser();
                }
                Ok(None) => {}
                Err(err) => {
                    let mut message = String::<128>::new();
                    let _ = write!(message, "error: {err}");
                    self.emit_line(message.as_str());
                    self.reset_parser();
                }
            }
        }
    }

    fn feed_line(&mut self, line: &[u8]) -> Result<(), ConsoleError> {
        for &byte in line {
            self.parser.push_byte(byte)?;
        }
        Ok(())
    }
}

#[cfg(feature = "kernel")]
impl FmtWrite for CohesixConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _ = self.console.write_str(s);
        Ok(())
    }
}
