// Author: Lukas Bower
// Purpose: Run mapped field-bus operations into a durable spool; controls require an independently signed admitted ticket and never derive authority from local output.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use clap::Parser;
use sidecar_bus::live::{Request, Spool, State};
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Bounded host MODBUS/DNP3 reads using generated exact maps"
)]
struct Args {
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long, conflicts_with = "status")]
    request: Option<PathBuf>,
    #[arg(long)]
    status: bool,
    /// Exact admitted host-ticket/v1 JSON. Controls also require independent enrollment.
    #[arg(long, requires = "evidence_enrollment_dir", conflicts_with = "status")]
    admitted_ticket: Option<PathBuf>,
    /// Private independent enrollment for this exact operation and executable.
    #[arg(long, requires = "admitted_ticket")]
    evidence_enrollment_dir: Option<PathBuf>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut spool = Spool::open(&args.state_dir)?;
    if args.status {
        println!(
            "{}",
            serde_json::json!({"schema":"cohesix-field-bus-operation-report/v1","authoritative":false,"worker_proof":false,"entries":spool.entries()})
        );
        return Ok(());
    }
    let path = args.request.ok_or("--request or --status required")?;
    let mut raw = Vec::new();
    std::fs::File::open(path)?
        .take(4097)
        .read_to_end(&mut raw)?;
    if raw.len() > 4096 {
        return Err("request_limit field_bus".into());
    }
    let request: Request = serde_json::from_slice(&raw)?;
    let entry = if let Some(path) = args.admitted_ticket {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(2049)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 2048 {
            return Err("request_limit admitted ticket".into());
        }
        let admitted: serde_json::Value = serde_json::from_slice(&bytes)?;
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?;
        let directory = args
            .evidence_enrollment_dir
            .as_deref()
            .ok_or("not_enabled independent control enrollment")?;
        let authority = cohesix_evidence::producer::Operation::open(
            directory,
            &request.id,
            &request.idempotency_key,
            cohesix_evidence::producer::Custody::NativeOperation,
            now,
        )?;
        let registry = cohesix_authority::provider::registry()?;
        authority.bind(
            admitted["action"]
                .as_str()
                .ok_or("invalid control action")?,
            admitted["writer_epoch"]
                .as_u64()
                .ok_or("invalid control epoch")?,
            registry["resolved_manifest_sha256"]
                .as_str()
                .ok_or("invalid manifest binding")?,
            "sidecar-bus",
            &cohesix_evidence::producer::executable_digest()?,
        )?;
        spool.control(&request, &admitted, &authority)?
    } else {
        spool.read(&request)?
    };
    println!(
        "{}",
        serde_json::json!({"schema":"cohesix-field-bus-operation-report/v1","mode":"live-host","authoritative":false,"worker_proof":false,"entry":entry})
    );
    if entry.state != State::Delivered {
        return Err("unconfirmed field_bus_operation; inspect retained state".into());
    }
    Ok(())
}
