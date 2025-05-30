// CLASSIFICATION: COMMUNITY
// Filename: args.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

use clap::{Arg, Command};

/// Builds and returns the CLI argument parser for the Coh_CC compiler.
pub fn build_cli() -> Command {
    Command::new("cohcc")
        .version("0.1")
        .about("Cohesix Compiler CLI")
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("FILE")
                .help("Input IR file to compile")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Output file path")
                .required(false)
                .takes_value(true)
                .default_value("a.out"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .value_name("MS")
                .help("Request timeout in milliseconds")
                .required(false)
                .takes_value(true)
                .default_value("5000"),
        )
}
