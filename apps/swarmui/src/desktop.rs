// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide testable SwarmUI desktop CLI and environment helpers outside the Tauri binary.
// Author: Lukas Bower

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use cohsh::ticket_mint::{mint_ticket_from_config, mint_ticket_from_secret, TicketMintRequest};

use crate::parse_role_label;

/// Parsed command-line arguments for SwarmUI ticket minting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintArgs {
    /// Requested Cohesix role label.
    pub role: Option<String>,
    /// Optional ticket subject.
    pub subject: Option<String>,
    /// Optional ticket configuration path.
    pub config: Option<PathBuf>,
    /// Optional ticket signing secret.
    pub secret: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct TicketConfigFile {
    #[serde(default)]
    tickets: Vec<TicketConfigEntry>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct TicketConfigEntry {
    role: Option<String>,
    secret: Option<String>,
}

/// Parse the hive replay path from SwarmUI command-line arguments.
pub fn parse_replay_path(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--replay" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(value) = arg.strip_prefix("--replay=") {
            return Some(PathBuf::from(value));
        }
    }
    None
}

/// Parse the trace replay path from SwarmUI command-line arguments.
pub fn parse_trace_replay_path(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--replay-trace" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(value) = arg.strip_prefix("--replay-trace=") {
            return Some(PathBuf::from(value));
        }
    }
    None
}

/// Resolve a CLI replay path against the current directory and SwarmUI data directory.
pub fn resolve_replay_path(path: &Path, data_dir: &Path, subdir: &str) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if let Ok(cwd) = env::current_dir() {
        let candidate = cwd.join(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    let data_candidate = data_dir.join(subdir).join(path);
    if data_candidate.is_file() {
        return data_candidate;
    }
    data_candidate
}

/// Parse SwarmUI ticket minting arguments.
pub fn parse_mint_args(args: &[String]) -> Option<MintArgs> {
    let mut mint = false;
    let mut role = None;
    let mut subject = None;
    let mut config = None;
    let mut secret = None;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--mint-ticket" {
            mint = true;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--role=") {
            role = Some(value.to_owned());
            continue;
        }
        if arg == "--role" {
            if let Some(value) = iter.next() {
                role = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-subject=") {
            subject = Some(value.to_owned());
            continue;
        }
        if arg == "--ticket-subject" {
            if let Some(value) = iter.next() {
                subject = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-config=") {
            config = Some(PathBuf::from(value));
            continue;
        }
        if arg == "--ticket-config" {
            if let Some(value) = iter.next() {
                config = Some(PathBuf::from(value));
            }
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ticket-secret=") {
            secret = Some(value.to_owned());
            continue;
        }
        if arg == "--ticket-secret" {
            if let Some(value) = iter.next() {
                secret = Some(value.to_owned());
            }
        }
    }
    if mint {
        Some(MintArgs {
            role,
            subject,
            config,
            secret,
        })
    } else {
        None
    }
}

fn resolve_ticket_config(cli_path: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = cli_path {
        return Ok(path);
    }
    if let Ok(value) =
        env::var("SWARMUI_TICKET_CONFIG").or_else(|_| env::var("COHSH_TICKET_CONFIG"))
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(PathBuf::from("configs/root_task.toml"))
}

fn resolve_ticket_secret(cli_secret: Option<String>) -> Result<Option<String>, String> {
    if cli_secret.is_some() {
        return Ok(cli_secret);
    }
    match env::var("SWARMUI_TICKET_SECRET").or_else(|_| env::var("COHSH_TICKET_SECRET")) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(format!("failed to read SWARMUI_TICKET_SECRET: {err}")),
    }
}

fn resolve_ticket_secret_from_config(config_path: &Path) -> Result<Option<String>, String> {
    let contents = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read {}: {err}", config_path.display()))?;
    let parsed: TicketConfigFile = toml::from_str(&contents)
        .map_err(|err| format!("failed to parse {}: {err}", config_path.display()))?;
    for ticket in parsed.tickets {
        let Some(role) = ticket.role.as_deref() else {
            continue;
        };
        if role.trim() != "queen" {
            continue;
        }
        let Some(secret) = ticket.secret.as_deref() else {
            continue;
        };
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_owned()));
        }
    }
    Ok(None)
}

fn resolve_console_auth_token_from_sources(
    swarmui_auth_token: Option<&str>,
    cohsh_auth_token: Option<&str>,
    coh_auth_token: Option<&str>,
    config_path: &Path,
) -> Result<String, String> {
    for candidate in [swarmui_auth_token, cohsh_auth_token, coh_auth_token] {
        let Some(value) = candidate else {
            continue;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    if let Some(secret) = resolve_ticket_secret_from_config(config_path)? {
        return Ok(secret);
    }
    Err(format!(
        "console auth token is not configured; set SWARMUI_AUTH_TOKEN/COHSH_AUTH_TOKEN/COH_AUTH_TOKEN or define a queen ticket secret in {}",
        config_path.display()
    ))
}

/// Resolve the console auth token from environment variables or queen ticket config.
pub fn resolve_console_auth_token() -> Result<String, String> {
    let config_path = resolve_ticket_config(None)?;
    let swarmui_auth_token = env::var("SWARMUI_AUTH_TOKEN").ok();
    let cohsh_auth_token = env::var("COHSH_AUTH_TOKEN").ok();
    let coh_auth_token = env::var("COH_AUTH_TOKEN").ok();
    resolve_console_auth_token_from_sources(
        swarmui_auth_token.as_deref(),
        cohsh_auth_token.as_deref(),
        coh_auth_token.as_deref(),
        config_path.as_path(),
    )
}

/// Resolve the REST transport auth token from supported SwarmUI and Cohesix variables.
pub fn resolve_rest_auth_token() -> Option<String> {
    for key in [
        "SWARMUI_REST_AUTH_TOKEN",
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN",
        "COH_REST_AUTH_TOKEN",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// Mint a Cohesix ticket for a role using an explicit secret or ticket config.
pub fn mint_ticket_for_role(
    role_label: &str,
    subject: Option<&str>,
    config: Option<PathBuf>,
    secret: Option<String>,
) -> Result<String, String> {
    let role = parse_role_label(role_label).map_err(|err| err.to_string())?;
    let request = TicketMintRequest::new(role, subject, None).map_err(|err| err.to_string())?;
    if let Some(secret) = resolve_ticket_secret(secret)? {
        return mint_ticket_from_secret(&request, secret.as_str()).map_err(|err| err.to_string());
    }
    let config_path = resolve_ticket_config(config)?;
    mint_ticket_from_config(&request, config_path.as_path()).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_console_auth_token_prefers_explicit_env_sources() {
        let config_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("root_task.toml");
        fs::write(
            &config_path,
            "[[tickets]]\nrole = \"queen\"\nsecret = \"bootstrap\"\n",
        )
        .expect("write config");

        let token = resolve_console_auth_token_from_sources(
            Some("swarmui-token"),
            Some("cohsh-token"),
            Some("coh-token"),
            config_path.as_path(),
        )
        .expect("resolve auth token");

        assert_eq!(token, "swarmui-token");
    }

    #[test]
    fn resolve_console_auth_token_falls_back_to_queen_ticket_secret() {
        let config_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("root_task.toml");
        fs::write(
            &config_path,
            "[[tickets]]\nrole = \"queen\"\nsecret = \"bootstrap\"\n",
        )
        .expect("write config");

        let token =
            resolve_console_auth_token_from_sources(None, None, None, config_path.as_path())
                .expect("resolve auth token");

        assert_eq!(token, "bootstrap");
    }

    #[test]
    fn resolve_console_auth_token_errors_without_env_or_ticket_secret() {
        let config_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("root_task.toml");
        fs::write(
            &config_path,
            "[[tickets]]\nrole = \"worker-heartbeat\"\nsecret = \"x\"\n",
        )
        .expect("write config");

        let err = resolve_console_auth_token_from_sources(None, None, None, config_path.as_path())
            .expect_err("missing auth token should fail");

        assert!(err.contains("console auth token is not configured"));
    }
}
