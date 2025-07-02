// CLASSIFICATION: COMMUNITY
// Filename: srv.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-01-27

use clap::Parser;
use cohesix::{CohError};
use cohesix::trace::recorder::event;

/// Register a service address under /srv
#[derive(Parser)]
#[command(about = "Create /srv entry for a service")]
struct Args {
    /// Name of the service
    name: String,
    /// Address in format tcp:HOST:PORT or unix:/path
    address: String,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    std::fs::create_dir_all("/srv")?;
    std::fs::write(format!("/srv/{}", args.name), &args.address)?;
    event(
        "srv",
        "register",
        &format!("{} {}", args.name, args.address),
    );
    println!("registered {} => {}", args.name, args.address);
    Ok(())
}
