// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21

use clap::{Parser, Subcommand};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use chrono::Utc;

#[derive(Parser)]
#[command(about = "Trace inspection utilities")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected workers
    List,
    /// Push a trace file to the queen
    PushTrace { worker_id: String, path: PathBuf },
}

fn append_summary(entry: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("VALIDATION_SUMMARY.md")
    {
        let ts = Utc::now().to_rfc3339();
        let _ = writeln!(f, "- {ts} {entry}");
    }
}

fn cmd_list() -> anyhow::Result<()> {
    let base = Path::new("/srv/workers");
    if base.exists() {
        for ent in fs::read_dir(base)? {
            let name = ent?.file_name();
            println!("worker: {}", name.to_string_lossy());
        }
    } else {
        println!("no workers directory");
    }
    append_summary("cohtrace list ok");
    Ok(())
}

fn cmd_push(worker_id: String, path: PathBuf) -> anyhow::Result<()> {
    let dest = Path::new("/trace").join(&worker_id);
    fs::create_dir_all(&dest)?;
    fs::copy(&path, dest.join("sim.json"))?;
    println!("trace stored for {}", worker_id);
    append_summary(&format!("cohtrace push_trace {} {}", worker_id, path.display()));
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => cmd_list()?,
        Cmd::PushTrace { worker_id, path } => cmd_push(worker_id, path)?,
    }
    Ok(())
}

