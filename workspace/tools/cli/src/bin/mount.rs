// CLASSIFICATION: COMMUNITY
// Filename: mount.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-01-27

use clap::Parser;
use cohesix::trace::recorder::event;
use cohesix::{CohError, coh_bail};
use ninep::client::TcpClient;
#[cfg(unix)]
use ninep::client::UnixClient;

/// Mount a 9P service and list files.
#[derive(Parser)]
#[command(about = "Mount a 9P service from /srv and list files")]
struct Args {
    /// Service name in /srv
    name: String,
    /// Mount point under /mnt
    mount: String,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    let entry = std::fs::read_to_string(format!("/srv/{}", args.name))?;
    let addr = entry.trim();
    std::fs::create_dir_all("/mnt")?;
    let mnt = format!("/mnt/{}", args.mount.trim_start_matches('/'));
    std::fs::create_dir_all(&mnt)?;
    event("mount", "start", &format!("{} -> {}", args.name, mnt));
    if let Some(tcp) = addr.strip_prefix("tcp:") {
        let mut client = TcpClient::new_tcp("cli".to_string(), tcp, "/")?;
        for stat in client.read_dir("/")? {
            println!("{}", stat.fm.name);
        }
    } else if let Some(path) = addr.strip_prefix("unix:") {
        #[cfg(unix)]
        {
            let mut client =
                UnixClient::new_unix_with_explicit_path("cli".to_string(), path.to_string(), "/")?;
            for stat in client.read_dir("/")? {
                println!("{}", stat.fm.name);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            coh_bail!("unix sockets unsupported on this platform");
        }
    } else {
        coh_bail!("unknown address: {}", addr);
    }
    Ok(())
}
