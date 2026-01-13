// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task ipc_compile_guard.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use std::fs;
use std::path::Path;

use root_task::sel4::{self, IpcError};
use sel4_sys::seL4_MessageInfo;

const IPC_SRC_DIR: &str = "apps/root-task/src";
const SEL4_RS: &str = "sel4.rs";
const FORBIDDEN_SYMBOLS: &[&str] = &[
    "seL4_Send",
    "seL4_Call",
    "seL4_ReplyRecv",
    "seL4_Reply",
    "seL4_Recv",
    "seL4_NBRecv",
    "seL4_Wait",
];

#[test]
fn send_guarded_signature_is_callable() {
    fn expects_guarded<F>(func: F)
    where
        F: Fn(seL4_MessageInfo) -> Result<(), IpcError>,
    {
        let info = seL4_MessageInfo::new(0, 0, 0, 0);
        let _ = func(info);
    }

    expects_guarded(sel4::send_guarded);
}

#[test]
fn raw_ipc_symbols_absent_outside_sel4_module() {
    let mut offenders = Vec::new();
    scan_for_raw_ipc(Path::new(IPC_SRC_DIR), &mut offenders);

    if !offenders.is_empty() {
        panic!(
            "raw seL4 IPC symbols detected outside sel4.rs:\n{}",
            offenders.join("\n")
        );
    }
}

fn scan_for_raw_ipc(path: &Path, offenders: &mut Vec<String>) {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq(SEL4_RS))
        .unwrap_or(false)
    {
        return;
    }

    if path.is_dir() {
        for entry in fs::read_dir(path).expect("ipc guard scan directory") {
            let entry = entry.expect("ipc guard directory entry");
            scan_for_raw_ipc(&entry.path(), offenders);
        }
        return;
    }

    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return;
    }

    let contents = fs::read_to_string(path).expect("ipc guard read source");
    if let Some(symbol) = contains_forbidden_symbol(&contents) {
        offenders.push(format!("{} -> {}", path.display(), symbol));
    }
}

fn contains_forbidden_symbol(source: &str) -> Option<&'static str> {
    for &symbol in FORBIDDEN_SYMBOLS {
        let mut start = 0;
        while let Some(offset) = source[start..].find(symbol) {
            let absolute = start + offset;
            if has_word_boundary(source, absolute)
                && follows_call(&source[absolute + symbol.len()..])
            {
                return Some(symbol);
            }
            start = absolute + symbol.len();
        }
    }
    None
}

fn has_word_boundary(source: &str, index: usize) -> bool {
    index == 0
        || source[..index]
            .chars()
            .rev()
            .next()
            .map(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            .unwrap_or(true)
}

fn follows_call(rest: &str) -> bool {
    let trimmed = rest.trim_start();
    trimmed.starts_with('(')
}
