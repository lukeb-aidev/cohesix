// Author: Lukas Bower
// Purpose: Record bounded real native command observations for the explicit SwarmUI acceptance workflow.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
//! Opt-in native acceptance evidence records metadata, never credentials or raw command inputs.
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

/// A single application process owns a new acceptance log; existing evidence is immutable.
pub struct Recorder {
    file: File,
    sequence: u64,
}
impl Recorder {
    /// Open a fresh absolute evidence path; no implicit parent directories or overwrite.
    pub fn create(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("acceptance_path: use an absolute log path".into());
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|_| "acceptance_path: cannot create new log")?;
        Ok(Self { file, sequence: 0 })
    }
    /// Metadata is selected at command boundaries; record and byte bounds fail closed.
    pub fn record(&mut self, action: &str, observation: Value) -> Result<(), String> {
        if self.sequence >= 1024 {
            return Err("acceptance_bound: start a fresh bounded run".into());
        }
        let record = json!({"lane":"native_bridge","sequence":self.sequence,"action":action,"observation":observation});
        let mut bytes = serde_json::to_vec(&record).map_err(|_| "acceptance_format")?;
        if bytes.len() > 16384 {
            return Err("acceptance_bound: observation too large".into());
        }
        bytes.push(b'\n');
        self.file.write_all(&bytes).map_err(|_| "acceptance_io")?;
        self.file.flush().map_err(|_| "acceptance_io")?;
        self.sequence += 1;
        Ok(())
    }
}
