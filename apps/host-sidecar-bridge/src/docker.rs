// Author: Lukas Bower
// Purpose: Bind bounded Docker Engine operations to immutable container/image identities and observed terminal state.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const API_VERSION: &str = "1.40";
const MAX_RESPONSE_BYTES: u64 = 65_536;

/// Native observation; labels and environment variables are never returned.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContainerObservation {
    /// Full immutable Engine container ID.
    pub container_id: String,
    /// Immutable sha256 image configuration ID.
    pub image_id: String,
    /// Observed running flag.
    pub running: bool,
    /// Pending restart state excludes terminal success.
    pub restarting: bool,
    /// Paused containers cannot satisfy running postconditions.
    pub paused: bool,
    /// Native dead state excludes success.
    pub dead: bool,
    /// Observed OOM flag excludes successful execution.
    pub oom_killed: bool,
    /// Native terminal process exit code.
    pub exit_code: i64,
    /// Native runtime start identity.
    pub started_at: String,
    /// Native runtime finish observation.
    pub finished_at: String,
    /// Configured health check result, absent when no check exists.
    pub health: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Inspect {
    id: String,
    image: String,
    state: State,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct State {
    running: bool,
    restarting: bool,
    paused: bool,
    dead: bool,
    #[serde(rename = "OOMKilled")]
    oom_killed: bool,
    exit_code: i64,
    started_at: String,
    finished_at: String,
    health: Option<Health>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Health {
    status: String,
}

/// Bounded native event cursor, independent of mutable correlation labels.
#[derive(Debug, Clone, Serialize)]
pub struct ContainerEvent {
    /// Native operation or event action.
    pub action: String,
    /// Full immutable Engine container ID.
    pub container_id: String,
    /// Engine event cursor timestamp in nanoseconds.
    pub time_nano: u64,
}

/// Verified native outcome, not by itself a Cohesix admitted receipt.
#[derive(Debug, Serialize)]
pub struct ContainerTransition {
    /// Negotiated supported Engine API version.
    pub api_version: &'static str,
    /// Native operation or event action.
    pub action: String,
    /// Immutable identity and state bound before dispatch.
    pub before: ContainerObservation,
    /// Matching identity with verified postcondition.
    pub after: ContainerObservation,
    /// Bounded filtered native event cursor range.
    pub events: Vec<ContainerEvent>,
    /// Observed log export size in bytes.
    pub log_bytes: u64,
    /// SHA-256 of the bounded native log export.
    pub log_sha256: String,
}

/// A client restricted to the explicitly selected local Engine socket.
pub struct DockerEngine {
    client: Client,
    deadline: Instant,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn version(value: &str) -> Result<(u16, u16)> {
    let (major, minor) = value
        .split_once('.')
        .ok_or_else(|| anyhow!("invalid_observation docker-api-version"))?;
    if major.is_empty()
        || minor.is_empty()
        || !major
            .bytes()
            .chain(minor.bytes())
            .all(|b| b.is_ascii_digit())
    {
        bail!("invalid_observation docker-api-version");
    }
    Ok((major.parse()?, minor.parse()?))
}

/// Use the selected API only when the daemon explicitly supports its interval.
pub fn negotiate_version(minimum: &str, maximum: &str) -> Result<&'static str> {
    let minimum = version(minimum)?;
    let maximum = version(maximum)?;
    let selected = version(API_VERSION)?;
    if minimum > maximum || selected < minimum || selected > maximum {
        bail!("not_supported docker-api-version");
    }
    Ok(API_VERSION)
}

/// Parse only the native fields needed for identity and terminal verification.
pub fn parse_observation(raw: &[u8]) -> Result<ContainerObservation> {
    if raw.len() as u64 > MAX_RESPONSE_BYTES {
        bail!("response_limit docker-inspect");
    }
    let value: Inspect =
        serde_json::from_slice(raw).context("invalid_observation docker-inspect")?;
    if !digest(&value.id) || !value.image.strip_prefix("sha256:").is_some_and(digest) {
        bail!("invalid_observation docker-identity");
    }
    let state = value.state;
    for timestamp in [&state.started_at, &state.finished_at] {
        if timestamp.is_empty()
            || timestamp.len() > 40
            || !timestamp
                .bytes()
                .all(|b| b.is_ascii_digit() || b"-:TZ.+".contains(&b))
        {
            bail!("invalid_observation docker-timestamp");
        }
    }
    if state.health.as_ref().is_some_and(|health| {
        !matches!(health.status.as_str(), "starting" | "healthy" | "unhealthy")
    }) {
        bail!("invalid_observation docker-health");
    }
    Ok(ContainerObservation {
        container_id: value.id,
        image_id: value.image,
        running: state.running,
        restarting: state.restarting,
        paused: state.paused,
        dead: state.dead,
        oom_killed: state.oom_killed,
        exit_code: state.exit_code,
        started_at: state.started_at,
        finished_at: state.finished_at,
        health: state.health.map(|value| value.status),
    })
}

/// Exact container/image and observed runtime state determine terminal success.
pub fn postcondition(
    action: &str,
    before: &ContainerObservation,
    after: &ContainerObservation,
) -> bool {
    if before.container_id != after.container_id
        || before.image_id != after.image_id
        || after.restarting
        || after.paused
        || after.dead
        || after.oom_killed
    {
        return false;
    }
    match action {
        "restart" => {
            after.running
                && after.started_at != before.started_at
                && !after.started_at.starts_with("0001-")
                && after
                    .health
                    .as_deref()
                    .is_none_or(|state| state == "healthy")
        }
        "stop" => !after.running && (!before.running || after.finished_at != before.finished_at),
        _ => false,
    }
}

impl DockerEngine {
    /// Negotiate an API over a local socket with one shared operation deadline.
    #[cfg(unix)]
    pub fn connect(socket: &Path, deadline: Instant) -> Result<Self> {
        if !socket.is_absolute() {
            bail!("invalid_target docker-socket");
        }
        let client = Client::builder()
            .unix_socket(socket)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;
        let engine = Self { client, deadline };
        let raw = engine.request(Method::GET, "/version", StatusCode::OK)?;
        let value: Value = serde_json::from_slice(&raw)?;
        negotiate_version(
            value["MinAPIVersion"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid_observation docker-version"))?,
            value["ApiVersion"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid_observation docker-version"))?,
        )?;
        Ok(engine)
    }

    fn request(&self, method: Method, path: &str, expected: StatusCode) -> Result<Vec<u8>> {
        let timeout = self.deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            bail!("timeout docker-api");
        }
        let response = self
            .client
            .request(method, format!("http://localhost{path}"))
            .timeout(timeout)
            .send()
            .map_err(|_| anyhow!("unavailable docker-api"))?;
        if response.status() != expected {
            bail!(
                "provider_error docker-api status={}",
                response.status().as_u16()
            );
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES)
        {
            bail!("response_limit docker-api");
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow!("unavailable docker-body"))?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            bail!("response_limit docker-api");
        }
        Ok(bytes)
    }

    /// Resolve a name once; mutation subsequently addresses only the returned ID.
    pub fn inspect(&self, target: &str) -> Result<ContainerObservation> {
        crate::native::validate_native_id(target)?;
        parse_observation(&self.request(
            Method::GET,
            &format!("/v{API_VERSION}/containers/{target}/json"),
            StatusCode::OK,
        )?)
    }

    /// Bounded server counters without raw daemon configuration or credentials.
    pub fn status(&self) -> Result<crate::providers::DockerStatus> {
        let raw = self.request(
            Method::GET,
            &format!("/v{API_VERSION}/info"),
            StatusCode::OK,
        )?;
        let value: Value = serde_json::from_slice(&raw)?;
        let counter = |field: &str| -> Result<u32> {
            value[field]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| anyhow!("invalid_observation docker-counter"))
        };
        let version = value["ServerVersion"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid_observation docker-version"))?;
        crate::native::validate_native_id(version)?;
        Ok(crate::providers::DockerStatus {
            version: version.into(),
            containers: counter("Containers")?.to_string(),
            running: counter("ContainersRunning")?.to_string(),
            paused: counter("ContainersPaused")?.to_string(),
            stopped: counter("ContainersStopped")?.to_string(),
        })
    }

    /// Observe matching immutable-ID events in a finite native cursor interval.
    fn events(&self, id: &str, since: u64) -> Result<Vec<ContainerEvent>> {
        let until = unix_seconds()? + 1;
        let filters = serde_json::json!({"container":[id],"type":["container"],"event":["start","die","stop","restart","kill","oom"]}).to_string();
        let mut url = reqwest::Url::parse("http://localhost")?;
        url.query_pairs_mut()
            .append_pair("since", &since.to_string())
            .append_pair("until", &until.to_string())
            .append_pair("filters", &filters);
        let raw = self.request(
            Method::GET,
            &format!("/v{API_VERSION}/events?{}", url.query().unwrap_or("")),
            StatusCode::OK,
        )?;
        let mut events = Vec::new();
        for line in raw
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            if events.len() >= 128 {
                bail!("response_limit docker-events");
            }
            let value: Value = serde_json::from_slice(line)?;
            let action = value["Action"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid_observation docker-event"))?;
            let native_id = value["Actor"]["ID"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid_observation docker-event"))?;
            let time_nano = value["timeNano"]
                .as_u64()
                .ok_or_else(|| anyhow!("invalid_observation docker-event"))?;
            if native_id != id
                || value["Type"] != "container"
                || time_nano / 1_000_000_000 < since
                || time_nano / 1_000_000_000 > until
            {
                bail!("invalid_observation docker-event-correlation");
            }
            crate::native::validate_native_id(action)?;
            events.push(ContainerEvent {
                action: action.into(),
                container_id: native_id.into(),
                time_nano,
            });
        }
        Ok(events)
    }

    /// Dispatch once and verify final state, health, events and bounded log digest.
    pub fn transition(&self, target: &str, action: &str) -> Result<ContainerTransition> {
        if !matches!(action, "restart" | "stop") {
            bail!("not_implemented docker-action");
        }
        let before = self.inspect(target)?;
        let since = unix_seconds()?;
        self.request(
            Method::POST,
            &format!(
                "/v{API_VERSION}/containers/{}/{action}?t=5",
                before.container_id
            ),
            StatusCode::NO_CONTENT,
        )?;
        // Any failure after dispatch is ambiguous to the caller and must not be retried.
        loop {
            let after = self.inspect(&before.container_id)?;
            if postcondition(action, &before, &after) {
                let events = self.events(&before.container_id, since)?;
                if !events.iter().any(|event| event.action == action) {
                    bail!("unverified docker-terminal-event");
                }
                let logs = self.request(Method::GET, &format!("/v{API_VERSION}/containers/{}/logs?stdout=1&stderr=1&tail=64&since={since}", before.container_id), StatusCode::OK)?;
                use sha2::{Digest, Sha256};
                return Ok(ContainerTransition {
                    api_version: API_VERSION,
                    action: action.into(),
                    before,
                    after,
                    events,
                    log_bytes: logs.len() as u64,
                    log_sha256: Sha256::digest(&logs)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                });
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("timeout docker-postcondition");
            }
            std::thread::sleep(remaining.min(Duration::from_millis(25)));
        }
    }
}

fn unix_seconds() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"Id":"a".repeat(64), "Image":format!("sha256:{}", "b".repeat(64)),
            "State":{"Running":true,"Paused":false,"Restarting":false,"Dead":false,"OOMKilled":false,"ExitCode":0,
                     "StartedAt":"2026-09-14T00:00:00Z","FinishedAt":"0001-01-01T00:00:00Z"}})).unwrap()
    }
    #[test]
    fn version_negotiation_requires_the_complete_supported_interval() {
        assert_eq!(negotiate_version("1.24", "1.54").unwrap(), "1.40");
        for (min, max) in [
            ("1.41", "1.54"),
            ("1.24", "1.39"),
            ("1.54", "1.40"),
            ("1.x", "1.54"),
        ] {
            assert!(negotiate_version(min, max).is_err());
        }
    }
    #[test]
    fn container_replacement_old_start_pending_health_and_oom_cannot_verify() {
        let before = parse_observation(&sample()).unwrap();
        assert!(!postcondition("restart", &before, &before));
        let mut after = before.clone();
        after.started_at = "2026-09-14T00:00:01Z".into();
        assert!(postcondition("restart", &before, &after));
        for field in ["identity", "image", "oom", "health", "restarting"] {
            let mut invalid = after.clone();
            match field {
                "identity" => invalid.container_id = "c".repeat(64),
                "image" => invalid.image_id = "c".repeat(64),
                "oom" => invalid.oom_killed = true,
                "health" => invalid.health = Some("starting".into()),
                _ => invalid.restarting = true,
            }
            assert!(!postcondition("restart", &before, &invalid));
        }
        after.running = false;
        after.finished_at = "2026-09-14T00:00:02Z".into();
        assert!(postcondition("stop", &before, &after));
    }
}
