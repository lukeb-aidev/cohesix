// CLASSIFICATION: COMMUNITY
// Filename: cohesix_shell.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-01

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const DEFAULT_BUSYBOX_PATH: &str = "/mnt/data/bin/cohbox";
const BUSYBOX_ENV_VAR: &str = "COHESIX_BUSYBOX_PATH";

fn main() {
    let code = match run_shell() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("cohesix-shell: {err}");
            err.exit_code()
        }
    };
    process::exit(code);
}

fn run_shell() -> Result<i32, ShellError> {
    let busybox = resolve_busybox_path()?;
    let args = collect_arguments();
    let status = Command::new(&busybox)
        .args(&args)
        .status()
        .map_err(|source| ShellError::Spawn {
            path: busybox.clone(),
            source,
        })?;

    match status.code() {
        Some(code) => Ok(code),
        None => Err(ShellError::Terminated(busybox)),
    }
}

fn collect_arguments() -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    args.push(OsString::from("sh"));
    args.extend(env::args_os().skip(1));
    args
}

fn resolve_busybox_path() -> Result<PathBuf, ShellError> {
    let override_path = env::var_os(BUSYBOX_ENV_VAR)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());

    let candidate = override_path.unwrap_or_else(|| PathBuf::from(DEFAULT_BUSYBOX_PATH));
    verify_executable(&candidate)?;
    Ok(candidate)
}

fn verify_executable(path: &Path) -> Result<(), ShellError> {
    let metadata = fs::metadata(path).map_err(|source| ShellError::Missing {
        path: path.to_path_buf(),
        source,
    })?;

    if !metadata.is_file() {
        return Err(ShellError::NotFile(path.to_path_buf()));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 {
            return Err(ShellError::NotExecutable(path.to_path_buf()));
        }
    }

    Ok(())
}

#[derive(Debug)]
enum ShellError {
    Missing { path: PathBuf, source: io::Error },
    NotFile(PathBuf),
    NotExecutable(PathBuf),
    Spawn { path: PathBuf, source: io::Error },
    Terminated(PathBuf),
}

impl ShellError {
    fn exit_code(&self) -> i32 {
        match self {
            ShellError::Missing { .. }
            | ShellError::NotFile(_)
            | ShellError::NotExecutable(_)
            | ShellError::Spawn { .. }
            | ShellError::Terminated(_) => 1,
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellError::Missing { path, source } => {
                write!(
                    f,
                    "failed to locate BusyBox executable at {}: {}",
                    path.display(),
                    source
                )
            }
            ShellError::NotFile(path) => {
                write!(f, "BusyBox path {} is not a regular file", path.display())
            }
            ShellError::NotExecutable(path) => {
                write!(f, "BusyBox path {} is not executable", path.display())
            }
            ShellError::Spawn { path, source } => {
                write!(f, "failed to launch {}: {}", path.display(), source)
            }
            ShellError::Terminated(path) => {
                write!(f, "BusyBox process {} terminated by signal", path.display())
            }
        }
    }
}

impl Error for ShellError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ShellError::Missing { source, .. } | ShellError::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}
