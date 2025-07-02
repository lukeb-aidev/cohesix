// CLASSIFICATION: COMMUNITY
// Filename: schema.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-18

use crate::prelude::*;
use crate::CohError;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct IRArg {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IROp {
    pub op: String,
    pub dst: String,
    pub src: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IRFunction {
    pub schema_version: String,
    pub name: String,
    pub args: Vec<IRArg>,
    pub body: Vec<IROp>,
}

fn append_log(line: &str) -> std::io::Result<()> {
    fs::create_dir_all("/log")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/cohcc_ir.log")?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    f.flush()?;
    Ok(())
}

pub fn load_ir_from_file(path: &Path) -> Result<IRFunction, CohError> {
    let mut data = String::new();
    fs::File::open(path)?.read_to_string(&mut data)?;
    let ir: IRFunction = serde_json::from_str(&data)?;
    append_log(&format!("parsed {}", path.display()))?;
    Ok(ir)
}
