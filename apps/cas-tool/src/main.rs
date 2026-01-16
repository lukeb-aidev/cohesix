// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for CAS bundle packaging and upload.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the Cohesix CAS host tool.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use cas_tool::{
    bundle_paths, build_bundle, load_delta_base, load_signing_key, load_template_config,
    write_bundle,
};
use clap::{Parser, Subcommand};
use cohsh::{Transport, TcpTransport};
use cohesix_cas::CasManifest;
use cohesix_ticket::Role;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::str;

// Keep in sync with crates/cohsh-core/src/command.rs MAX_ECHO_LEN.
const MAX_ECHO_PAYLOAD_BYTES: usize = 128;
const B64_PREFIX: &str = "b64:";
const MAX_B64_CHUNK_LEN: usize =
    (MAX_ECHO_PAYLOAD_BYTES - B64_PREFIX.len()) / 4 * 4;

#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix CAS packaging tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Package a payload into CAS chunks and manifest.
    Pack(PackArgs),
    /// Upload a CAS bundle via the TCP console.
    Upload(UploadArgs),
}

#[derive(Debug, Parser)]
struct PackArgs {
    /// Update epoch label.
    #[arg(long)]
    epoch: String,
    /// Input payload file to package.
    #[arg(long)]
    input: PathBuf,
    /// Output directory for the bundle (defaults to out/cas/<epoch>).
    #[arg(long)]
    out_dir: Option<PathBuf>,
    /// CAS manifest template JSON (defaults to out/cas_manifest_template.json).
    #[arg(long, default_value = "out/cas_manifest_template.json")]
    template: PathBuf,
    /// Override chunk_bytes from the template.
    #[arg(long)]
    chunk_bytes: Option<usize>,
    /// Optional base bundle directory for delta manifests.
    #[arg(long)]
    delta_base: Option<PathBuf>,
    /// Path to an Ed25519 signing key (hex).
    #[arg(long)]
    signing_key: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct UploadArgs {
    /// Bundle directory containing manifest.cbor and chunks/.
    #[arg(long)]
    bundle: PathBuf,
    /// TCP host for the Cohesix console.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// TCP port for the Cohesix console.
    #[arg(long, default_value_t = 31337)]
    port: u16,
    /// Authentication token for the TCP console.
    #[arg(long, default_value = "changeme")]
    auth_token: String,
    /// Optional queen ticket payload.
    #[arg(long)]
    ticket: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Pack(args) => pack_bundle(args),
        Command::Upload(args) => upload_bundle(args),
    }
}

fn pack_bundle(args: PackArgs) -> Result<()> {
    let payload = fs::read(&args.input)
        .with_context(|| format!("read payload {}", args.input.display()))?;
    let mut template = load_template_config(&args.template).with_context(|| {
        format!("load cas template {}", args.template.display())
    })?;
    if let Some(chunk_bytes) = args.chunk_bytes {
        template.chunk_bytes = chunk_bytes;
    }
    let delta_base = if let Some(base_dir) = args.delta_base.as_ref() {
        Some(load_delta_base(base_dir)?)
    } else {
        None
    };
    let signing_key = if let Some(path) = args.signing_key.as_ref() {
        Some(load_signing_key(path)?)
    } else {
        None
    };
    let bundle = build_bundle(
        args.epoch.as_str(),
        &payload,
        &template,
        delta_base,
        signing_key,
    )?;
    let out_dir = args
        .out_dir
        .unwrap_or_else(|| PathBuf::from("out").join("cas").join(args.epoch.as_str()));
    write_bundle(&bundle, &out_dir)?;
    println!("cas-tool: wrote bundle {}", out_dir.display());
    Ok(())
}

fn upload_bundle(args: UploadArgs) -> Result<()> {
    let (manifest_path, chunks_dir) = bundle_paths(&args.bundle)?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read manifest {}", manifest_path.display()))?;
    let manifest = CasManifest::decode(&manifest_bytes).map_err(|err| {
        anyhow::anyhow!("decode manifest {}: {err}", manifest_path.display())
    })?;
    let chunk_bytes = manifest.chunk_bytes as usize;

    let mut chunks = Vec::new();
    for digest in &manifest.chunks {
        let name = hex::encode(digest);
        let chunk_path = chunks_dir.join(&name);
        let payload = fs::read(&chunk_path)
            .with_context(|| format!("read chunk {}", chunk_path.display()))?;
        if payload.len() != chunk_bytes {
            anyhow::bail!(
                "chunk {} has len {} (expected {})",
                chunk_path.display(),
                payload.len(),
                chunk_bytes
            );
        }
        let actual = Sha256::digest(&payload);
        if actual.as_slice() != digest {
            anyhow::bail!("chunk {} hash mismatch", chunk_path.display());
        }
        chunks.push((name, payload));
    }

    let mut transport = TcpTransport::new(args.host, args.port).with_auth_token(args.auth_token);
    let session = transport.attach(Role::Queen, args.ticket.as_deref())?;

    for (digest_hex, payload) in chunks {
        let path = format!("/updates/{}/chunks/{}", manifest.epoch, digest_hex);
        upload_b64_segments(&mut transport, &session, path.as_str(), &payload)?;
    }
    let manifest_path = format!("/updates/{}/manifest.cbor", manifest.epoch);
    upload_b64_segments(
        &mut transport,
        &session,
        manifest_path.as_str(),
        &manifest_bytes,
    )?;

    println!("cas-tool: uploaded update epoch={}", manifest.epoch);
    Ok(())
}

fn upload_b64_segments(
    transport: &mut TcpTransport,
    session: &cohsh::Session,
    path: &str,
    payload: &[u8],
) -> Result<()> {
    let encoded = BASE64_STANDARD.encode(payload);
    for chunk in encoded.as_bytes().chunks(MAX_B64_CHUNK_LEN) {
        let chunk_str = str::from_utf8(chunk).context("base64 chunk must be utf-8")?;
        let mut line = String::with_capacity(B64_PREFIX.len() + chunk_str.len());
        line.push_str(B64_PREFIX);
        line.push_str(chunk_str);
        if line.len() > MAX_ECHO_PAYLOAD_BYTES {
            anyhow::bail!(
                "payload segment exceeds console limit (len={} max={})",
                line.len(),
                MAX_ECHO_PAYLOAD_BYTES
            );
        }
        transport.write(session, path, line.as_bytes())?;
    }
    Ok(())
}
