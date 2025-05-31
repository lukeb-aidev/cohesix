use clap::Parser;

/// Command-line flags for a Cohesix worker process.
///
/// **Note:** Port any existing boot-time CLI parsing logic here
/// and delete it from kernel/boot code.
#[derive(Debug, Parser)]
pub struct WorkerArgs {
    /// Verbosity level (`error`, `warn`, `info`, `debug`, `trace`)
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

pub fn parse() -> WorkerArgs {
    WorkerArgs::parse()
}
