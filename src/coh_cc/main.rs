// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-18

use clap::Parser;
use cohesix::coh_cc::{
    backend::registry::get_backend,
    config::{Cli, Command, Config},
    guard,
    parser::input_type::CohInput,
    toolchain::Toolchain,
};
use cohesix::{cohcc_error, cohcc_info};
use std::path::Path;

/// Entry point for the cohcc binary.
pub fn main_entry() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.sandbox_info {
        println!(
            "sandbox role: {}",
            std::env::var("COHROLE").unwrap_or_default()
        );
        return Ok(());
    }
    let cfg = Config::from_cli(&cli)?;
    let backend_name = if cli.backend.is_empty() {
        "tcc"
    } else {
        &cli.backend
    };
    let backend = get_backend(backend_name)?;
    match cli.command {
        Command::Build { source, out, flags, .. } => {
            guard::validate_output_path(Path::new(&out))?;
            let input = CohInput::new(Path::new(&source).to_path_buf(), flags);
            let tc = Toolchain::new(cfg.toolchain_dir.clone())?;
            cohcc_info!(
                backend_name,
                Path::new(&source),
                Path::new(&out),
                &input.flags,
                &format!("detected {:?}", input.ty)
            );
            backend.compile(&input, Path::new(&out), &cfg.target, &cfg.sysroot, &tc)?;
            guard::verify_static_binary(Path::new(&out))?;
            let hash = guard::hash_output(Path::new(&out))?;
            guard::log_build(
                &hash,
                backend_name,
                Path::new(&source),
                Path::new(&out),
                &input.flags,
            )?;
            Ok(())
        }
    }
}

fn main() {
    if let Err(e) = main_entry() {
        cohcc_error!("tcc", Path::new(""), Path::new(""), &[], &format!("{e}"));
        eprintln!("cohcc: {e}");
        std::process::exit(1);
    }
}
