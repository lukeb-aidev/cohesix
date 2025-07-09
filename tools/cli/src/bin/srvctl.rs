// CLASSIFICATION: COMMUNITY
// Filename: srvctl.rs v0.2
// Author: Lukas Bower
// Date Modified: 2027-11-05

use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Plan9 service control tool")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Announce a service under /srv/services
    Announce {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "0")]
        version: String,
        path: PathBuf,
    },
}

fn announce(name: &str, version: &str, path: &PathBuf) -> std::io::Result<()> {
    let dir = PathBuf::from("/srv/services").join(name);
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("info"),
        format!("name={}\nversion={}\npath={}\n", name, version, path.display()),
    )?;
    fs::write(dir.join("ctl"), b"")?;
    println!("registered {} => {}", name, path.display());
    Ok(())
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Announce { name, version, path } => announce(&name, &version, &path)?,
    }
    Ok(())
}
