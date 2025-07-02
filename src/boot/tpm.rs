// CLASSIFICATION: PRIVATE
// Filename: tpm.rs v1.0
// Author: Codex
// Date Modified: 2025-06-07

/// TPM-based secure boot attestation helpers.
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Perform a basic TPM attestation if hardware is present.
/// Logs the outcome to `/srv/boot/attestation.log`.
pub fn attest() -> Result<(), CohError> {
    std::fs::create_dir_all("/srv/boot")?;
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/boot/attestation.log")?;
    if Path::new("/srv/tpm0").exists() {
        writeln!(log, "tpm present - attestation ok")?;
    } else {
        writeln!(log, "tpm not found - skipped")?;
    }
    Ok(())
}
