// CLASSIFICATION: COMMUNITY
// Filename: import.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-01-27

use clap::Parser;
use cohesix::{CohError};
use cohesix::trace::recorder::event;
use ninep::client::TcpClient;

/// Import a remote 9P service and register it under /srv
#[derive(Parser)]
#[command(about = "Import remote 9P service")]
struct Args {
    /// Remote tcp address host:port
    address: String,
    /// Service name
    name: String,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let _ = TcpClient::new_tcp("import".to_string(), &args.address, "/")?;
    std::fs::create_dir_all("/srv")?;
    std::fs::write(
        format!("/srv/{}", args.name),
        format!("tcp:{}", args.address),
    )?;
    event(
        "import",
        "register",
        &format!("{} as {}", args.address, args.name),
    );
    println!("imported {} as {}", args.address, args.name);
    Ok(())
}
