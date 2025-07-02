// CLASSIFICATION: COMMUNITY
// Filename: exportfs.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-01-27

use clap::Parser;
use cohesix::{CohError};
use cohesix::trace::recorder::event;
use cohesix_9p::{FsConfig, FsServer};

/// Serve a directory over 9P and register in /srv.
#[derive(Parser)]
#[command(about = "Export a directory via 9P")]
struct Args {
    /// Directory to export
    #[arg(long, default_value = ".")]
    root: String,
    /// Service name
    name: String,
    /// TCP port to listen on
    #[arg(long, default_value_t = 5640)]
    port: u16,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let cfg = FsConfig {
        root: args.root.clone().into(),
        port: args.port,
        readonly: false,
    };
    let mut srv = FsServer::new(cfg);
    srv.start()?;
    std::fs::create_dir_all("/srv")?;
    std::fs::write(
        format!("/srv/{}", args.name),
        format!("tcp:127.0.0.1:{}", args.port),
    )?;
    event("exportfs", "start", &args.name);
    println!("exported {} on tcp:{}", args.root, args.port);
    loop {
        std::thread::park();
    }
}
