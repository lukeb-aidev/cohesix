// CLASSIFICATION: COMMUNITY
// Filename: loader.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-30

use anyhow::{Context, Result};
use std::fs::File;
use std::io::Read;

const MAGIC: &[u8; 4] = b"COHB";
const VERSION: u8 = 1;

/// Load a `cohcc` binary and execute the embedded program.
pub fn load_and_run(path: &str) -> Result<()> {
    let mut f = File::open(path).with_context(|| format!("open {path}"))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).context("read file")?;
    if data.len() < 5 {
        anyhow::bail!("file too small");
    }
    if &data[0..4] != MAGIC {
        anyhow::bail!("invalid magic header");
    }
    if data[4] != VERSION {
        anyhow::bail!("unsupported version {}", data[4]);
    }
    use std::fs;
    use std::process::Command;

    let exe_bytes = &data[5..];
    let tmp_path = "/tmp/coh_exec.bin";
    fs::write(tmp_path, exe_bytes).context("write temp exe")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(tmp_path)?.permissions();
        perm.set_mode(0o755);
        fs::set_permissions(tmp_path, perm)?;
    }
    let status = Command::new(tmp_path).status().context("exec")?;
    fs::remove_file(tmp_path).ok();
    if !status.success() {
        anyhow::bail!("program exited with {:?}", status.code());
    }
    Ok(())
}
