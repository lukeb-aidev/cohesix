// Author: Lukas Bower
// Purpose: Keep desktop forms, connection inputs, Apple Keychain enrollment and host execution inside existing Cohesix authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
//! Desktop-only validation and bounded adapters; the owning command still admits every action.
#![allow(missing_docs)]

use clap::{CommandFactory, Parser};
use cohesix_authority::secret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const HOST_TIMEOUT: Duration = Duration::from_secs(120);

/// One explicit, private local Metal operation. It has no deployment authority.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMlxRequest {
    pub python: String,
    pub selection_path: String,
    pub operation: String,
    pub prompt: String,
    pub max_tokens: u16,
}

impl LocalMlxRequest {
    /// Reject ambiguous executables and input before starting the pinned helper.
    pub fn validate(&self) -> Result<(), String> {
        let executable = Path::new(&self.python);
        let name = executable
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !executable.is_absolute()
            || self.python.len() > 4096
            || !name.starts_with("python")
            || !executable.is_file()
            || self.python.bytes().any(|b| b.is_ascii_control())
        {
            return Err("mlx_runtime: select an installed Python executable".into());
        }
        let selected = Path::new(&self.selection_path);
        if !selected.is_absolute()
            || !selected.is_file()
            || self.selection_path.len() > 4096
            || self.selection_path.bytes().any(|b| b.is_ascii_control())
        {
            return Err("mlx_selection: choose one absolute private selection file".into());
        }
        match self.operation.as_str() {
            "infer"
                if (1..=2048).contains(&self.prompt.len())
                    && (1..=64).contains(&self.max_tokens) =>
            {
                Ok(())
            }
            "evaluate" if self.prompt.is_empty() && self.max_tokens == 0 => Ok(()),
            _ => Err("mlx_operation: choose bounded inference or held-out evaluation".into()),
        }
    }
}

/// Run the installed Cohesix Python MLX helper without a shell or network credentials.
pub fn run_local_mlx(request: &LocalMlxRequest) -> Result<Value, String> {
    request.validate()?;
    let payload = json!({"selection_path":request.selection_path,
        "operation":request.operation,"prompt":request.prompt,
        "max_tokens":request.max_tokens});
    let mut child = Command::new(&request.python)
        .args(["-I", "-m", "cohesix.mlx_workbench"])
        .current_dir("/")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "mlx_runtime: failed to launch installed Python")?;
    let serialized = serde_json::to_vec(&payload).map_err(|_| "mlx_request: invalid JSON")?;
    if let Some(mut input) = child.stdin.take() {
        input
            .write_all(&serialized)
            .map_err(|_| "mlx_request: helper input failed")?;
    }
    let stdout = child.stdout.take().ok_or("mlx_runtime: missing output")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("mlx_runtime: missing diagnostics")?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&exceeded);
    let out = std::thread::spawn(move || read_output(stdout, flag));
    let flag = Arc::clone(&exceeded);
    let err = std::thread::spawn(move || read_output(stderr, flag));
    let started = Instant::now();
    let status = loop {
        if exceeded.load(Ordering::Acquire) || started.elapsed() >= HOST_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("mlx_timeout: local operation exceeded its output or time bound".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("mlx_runtime: helper process failed".into());
            }
        }
    };
    let output = out
        .join()
        .map_err(|_| "mlx_runtime: output reader failed")?;
    let _ = err
        .join()
        .map_err(|_| "mlx_runtime: diagnostics reader failed")?;
    if !status.success() {
        return Err(
            "mlx_runtime: install the matching Cohesix Python package and pinned MLX extras".into(),
        );
    }
    let report: Value =
        serde_json::from_slice(&output).map_err(|_| "mlx_runtime: helper returned invalid JSON")?;
    if report["schema"] != "cohesix-local-mlx/v1"
        || !matches!(
            report["proof_class"].as_str(),
            Some("local_metal_observation" | "local_refusal")
        )
    {
        return Err("mlx_runtime: helper returned an unsupported schema".into());
    }
    Ok(report)
}
const APPLE_KEYCHAIN_SERVICE: &str = "com.cohesix.swarmui.gateway";
const APPLE_KEYCHAIN_ACCOUNT: &str = "selected-delegation";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequest {
    pub transport: String,
    pub endpoint: String,
    pub credential: String,
    pub role: String,
    pub ticket: Option<String>,
}

#[derive(Clone)]
pub struct Connection {
    pub transport: String,
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    pub credential: String,
    pub role: String,
    pub ticket: Option<String>,
}

impl ConnectionRequest {
    pub fn resolve(&self) -> Result<Connection, String> {
        crate::parse_role_label(&self.role).map_err(|_| "invalid_role: select a supported role")?;
        if self.transport == "rest" && self.role != "queen" {
            return Err("unsupported_role: gateway transport supports queen role with scoped delegation; use direct Queen TCP for Worker roles".into());
        }
        let endpoint = self.endpoint.trim();
        if endpoint.len() > 2048 || endpoint.bytes().any(|b| b.is_ascii_control()) {
            return Err(
                "invalid_endpoint: endpoint is oversized or contains control characters".into(),
            );
        }
        let parsed = url::Url::parse(endpoint)
            .map_err(|_| "invalid_endpoint: use tcp://host:port or https://gateway")?;
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(
                "invalid_endpoint: credentials, queries and fragments do not belong in an endpoint"
                    .into(),
            );
        }
        let host = parsed
            .host_str()
            .ok_or("invalid_endpoint: missing host")?
            .to_owned();
        let port = match self.transport.as_str() {
            "console" if parsed.scheme() == "tcp" && matches!(parsed.path(), "" | "/") => parsed
                .port()
                .ok_or("invalid_endpoint: include the Queen port")?,
            "rest" if matches!(parsed.scheme(), "http" | "https") && parsed.path() == "/" => parsed
                .port_or_known_default()
                .ok_or("invalid_endpoint: missing port")?,
            _ => {
                return Err(
                    "invalid_endpoint: choose a Queen TCP endpoint or a gateway base HTTP(S) URL"
                        .into(),
                )
            }
        };
        if port == 0 {
            return Err("invalid_endpoint: port must be nonzero".into());
        }
        let credential =
            secret::resolve_value(&self.credential).map_err(|e| format!("authentication: {e}"))?;
        let ticket = self
            .ticket
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(secret::resolve_value)
            .transpose()
            .map_err(|e| format!("ticket: {e}"))?;
        Ok(Connection {
            transport: self.transport.clone(),
            endpoint: endpoint.into(),
            host,
            port,
            credential,
            role: self.role.clone(),
            ticket,
        })
    }
}

/// Freeze only the already connected delegated gateway identity for native App Intents.
/// Keychain storage grants no new Cohesix authority; the gateway rechecks every use.
pub fn apple_delegation_payload(connection: &Connection) -> Result<Vec<u8>, String> {
    let endpoint = url::Url::parse(&connection.endpoint)
        .map_err(|_| "invalid_endpoint: Apple actions require a gateway URL")?;
    let loopback = matches!(
        endpoint.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    );
    if connection.transport != "rest"
        || connection.role != "queen"
        || !(endpoint.scheme() == "https" || endpoint.scheme() == "http" && loopback)
        || !matches!(endpoint.path(), "" | "/")
        || endpoint.username() != ""
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err("unsupported: Apple actions need a delegated HTTPS or loopback gateway".into());
    }
    let ticket = connection
        .ticket
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("authentication: a delegated ticket is required for Apple actions")?;
    let valid_secret = |value: &str, maximum: usize| {
        !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
    };
    if !valid_secret(&connection.credential, 4096) || !valid_secret(ticket, 8192) {
        return Err("authentication: Apple action credentials exceed bounds".into());
    }
    let payload = serde_json::to_vec(&json!({
        "endpoint": connection.endpoint,
        "requestToken": connection.credential,
        "delegatedTicket": ticket,
    }))
    .map_err(|_| "authentication: cannot encode Apple action enrollment")?;
    if payload.len() > 16_384 {
        return Err("authentication: Apple action enrollment exceeds Keychain bound".into());
    }
    Ok(payload)
}

/// Store a live, explicit delegation in the selected app's unsynchronised Keychain group.
#[cfg(target_os = "macos")]
pub fn store_apple_delegation(connection: &Connection) -> Result<(), String> {
    use security_framework::passwords::{set_generic_password_options, PasswordOptions};
    let payload = apple_delegation_payload(connection)?;
    let mut options =
        PasswordOptions::new_generic_password(APPLE_KEYCHAIN_SERVICE, APPLE_KEYCHAIN_ACCOUNT);
    options.set_access_synchronized(Some(false));
    options.use_protected_keychain();
    set_generic_password_options(&payload, options)
        .map_err(|_| "keychain: could not store the delegated gateway identity".into())
}

/// Remove only the native actions' selected delegation, leaving the live UI session intact.
#[cfg(target_os = "macos")]
pub fn remove_apple_delegation() -> Result<(), String> {
    use security_framework::passwords::{delete_generic_password_options, PasswordOptions};
    let mut options =
        PasswordOptions::new_generic_password(APPLE_KEYCHAIN_SERVICE, APPLE_KEYCHAIN_ACCOUNT);
    options.set_access_synchronized(Some(false));
    options.use_protected_keychain();
    delete_generic_password_options(options)
        .map_err(|_| "keychain: no removable Apple action enrollment was found".into())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRequest {
    pub operation: Vec<String>,
    pub values: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Serialize)]
pub struct HostResult {
    pub success: bool,
    pub outcome: String,
    pub stdout: String,
    pub stderr: String,
}

/// One reviewed action may be submitted once, until any session or tool change.
/// The mutex holding this gate also serializes session transitions and host dispatch.
#[derive(Default)]
pub struct ReviewGate {
    generation: u64,
    pending: Option<(String, Value)>,
}

impl ReviewGate {
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
    }

    pub fn preview(&mut self, action: Value) -> String {
        self.invalidate();
        let id = format!("review-{}", self.generation);
        self.pending = Some((id.clone(), action));
        id
    }

    pub fn consume(&mut self, id: &str, action: &Value) -> Result<(), String> {
        match self.pending.take() {
            Some((expected, reviewed)) if expected == id && &reviewed == action => Ok(()),
            _ => Err("review_expired: inputs or session changed; review the action again".into()),
        }
    }
}

/// The terminal runner and arbitrary local command wrapper are deliberately not UI executors.
pub fn host_unavailable(operation: &[String]) -> Option<&'static str> {
    if !cfg!(target_os = "linux")
        && matches!(
            operation.join(" ").as_str(),
            "workload inspect" | "workload register" | "workload diagnose"
        )
    {
        return Some("Install this workbench on the selected Linux GPU executor to inspect, enroll or diagnose its local package and device. Use the Mac workbench for admitted run, job status and reconciliation.");
    }
    match operation.first().map(String::as_str) {
        Some("run") => Some("Use a governed workflow or GPU workload. The desktop does not execute arbitrary host commands."),
        Some("mount") => Some("Persistent FUSE mounts are owned by the installed service manager; use deployment service configuration."),
        Some("identity") => Some("Identity enrollment is an administrator workflow; connect with your enrolled delegated ticket."),
        _ => None,
    }
}

fn hidden_field(id: &str) -> bool {
    matches!(
        id,
        "help"
            | "version"
            | "auth_token"
            | "rest_auth_token"
            | "ticket"
            | "ticket_ref"
            | "host"
            | "port"
            | "rest_url"
            | "mock"
            | "role"
    )
}

pub fn host_catalog() -> Value {
    let mut schema = coh_cli::ui_schema();
    fn strip(node: &mut Value, path: &mut Vec<String>) {
        if let Some(fields) = node["fields"].as_array_mut() {
            fields.retain(|f| !hidden_field(f["id"].as_str().unwrap_or("")));
        }
        node["unavailable"] = json!(host_unavailable(path));
        let help = match path.join(" ").as_str() {
            "workload inspect" => Some("Preflight one administrator-reviewed CUDA registration, package digest, typed parameters and resource bounds on the GPU host. This does not start work."),
            "workload register" => Some("Install the reviewed registration under the GPU executor owner's private state root. This is a privileged local configuration change."),
            "workload diagnose" => Some("Measure the selected GPU, native health and memory headroom against this executor's current configuration."),
            "gpu workload" => Some("Submit an already approved GPU ticket. Use Job status and reconciliation to obtain its original signed outcome."),
            "job status" => Some("Read the original admitted job's execution, delivery and reserved capacity state."),
            "job reconcile" => Some("Read target results for the original admission after interruption; never resubmit its side effect."),
            "peft release" => Some("Plan or reconcile one admitted adapter release. Inspect the held-out base, incumbent and candidate comparison, real serving canary, rejected candidate and restored generation in its signed evidence."),
            _ => None,
        };
        if let Some(help) = help {
            node["help"] = json!(help);
        }
        if let Some(children) = node["commands"].as_array_mut() {
            for child in children {
                let name = child["name"].as_str().unwrap_or("").to_owned();
                path.push(name);
                strip(child, path);
                path.pop();
            }
        }
    }
    strip(&mut schema, &mut Vec::new());
    schema
}

/// Convert structured values to argv and ask the *same* clap parser to reject ambiguity.
pub fn host_arguments(
    request: &HostRequest,
    connection: Option<&Connection>,
    offline: bool,
) -> Result<Vec<String>, String> {
    if request.operation.is_empty() || request.operation.len() > 4 || request.values.len() > 64 {
        return Err("invalid_operation: choose an operation".into());
    }
    if let Some(reason) = host_unavailable(&request.operation) {
        return Err(format!("unsupported: {reason}"));
    }
    let mut root = coh_cli::Cli::command();
    root.build();
    let mut node = &root;
    let mut argv = vec!["coh".to_owned()];
    let mut owned = BTreeMap::new();
    for arg in node.get_arguments() {
        owned.insert(arg.get_id().to_string(), (arg, 1usize));
    }
    for name in &request.operation {
        node = node
            .find_subcommand(name)
            .ok_or("invalid_operation: unknown command")?;
        argv.push(name.clone());
        for arg in node.get_arguments() {
            owned.insert(arg.get_id().to_string(), (arg, argv.len()));
        }
    }
    if node.get_subcommands().next().is_some() {
        return Err("invalid_operation: choose a complete operation".into());
    }
    let mut insertions: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut positional = Vec::new();
    for (id, values) in &request.values {
        let (arg, at) = owned.get(id).ok_or("invalid_field: unknown field")?;
        if hidden_field(id) {
            return Err("invalid_field: session fields belong to the connection desk".into());
        }
        if values.len() > 32
            || values
                .iter()
                .any(|s| s.len() > 4096 || s.contains('\0') || s.contains('\n') || s.contains('\r'))
        {
            return Err("invalid_field: values exceed bounds or contain control characters".into());
        }
        if let Some(long) = arg.get_long() {
            let dest = insertions.entry(*at).or_default();
            if matches!(arg.get_action(), clap::ArgAction::SetTrue) {
                if values == &["true"] {
                    dest.push(format!("--{long}"));
                } else if values != &["false"] {
                    return Err("invalid_field: expected boolean".into());
                }
            } else {
                for value in values {
                    dest.push(format!("--{long}={value}"));
                }
            }
        } else {
            positional.push((arg.get_index().unwrap_or(0), values.clone()));
        }
    }
    for (at, args) in insertions.into_iter().rev() {
        argv.splice(at..at, args);
    }
    positional.sort_by_key(|(index, _)| *index);
    for (_, values) in positional {
        argv.extend(values);
    }
    let has_connection = owned.contains_key("host");
    if offline && !host_offline(request) {
        return Err("offline: this operation may access a network; connect first".into());
    }
    if has_connection && !host_offline(request) {
        let connection = connection.ok_or("disconnected: connect before running this operation")?;
        if connection.transport != "rest" {
            return Err(
                "console_owned: use a Hive Gateway for host workflows alongside the UI".into(),
            );
        }
        argv.push(format!("--rest-url={}", connection.endpoint));
    }
    if let Some(connection) = connection {
        argv.insert(1, format!("--role={}", connection.role));
        if connection.ticket.is_some() {
            argv.insert(1, "--ticket-ref=env:SWARMUI_DELEGATED_TICKET".into());
        }
    }
    coh_cli::Cli::try_parse_from(&argv).map_err(|e| format!("invalid_arguments: {}", e.kind()))?;
    Ok(argv)
}

fn host_offline(request: &HostRequest) -> bool {
    let path = request.operation.join(" ");
    if cfg!(target_os = "linux")
        && matches!(
            path.as_str(),
            "workload inspect" | "workload register" | "workload diagnose"
        )
    {
        return true;
    }
    let has = |name: &str| {
        request
            .values
            .get(name)
            .is_some_and(|v| v.len() == 1 && !v[0].is_empty())
    };
    matches!(
        path.as_str(),
        "providers"
            | "plan"
            | "explain"
            | "trace"
            | "package verify"
            | "evidence verify"
            | "evidence story"
            | "evidence verify-recovery"
            | "evidence export"
            | "evidence timeline"
    ) || (matches!(path.as_str(), "inspect" | "attest") && has("input"))
        || (path == "peft release" && request.values.get("mode").is_some_and(|v| v == &["plan"]))
}

fn read_output(mut stream: impl Read, exceeded: Arc<AtomicBool>) -> Vec<u8> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 4096];
    while let Ok(count) = stream.read(&mut chunk) {
        if count == 0 {
            break;
        }
        let remaining = OUTPUT_LIMIT.saturating_sub(output.len());
        output.extend_from_slice(&chunk[..count.min(remaining)]);
        if count > remaining {
            exceeded.store(true, Ordering::Release);
        }
    }
    output
}

/// Execute a single installed tool without a shell. Timeout never asserts remote cancellation.
pub fn run_host(
    tool_dir: &Path,
    request: &HostRequest,
    connection: Option<&Connection>,
    offline: bool,
) -> Result<HostResult, String> {
    let argv = host_arguments(request, connection, offline)?;
    let tool = tool_dir.join("coh");
    if !tool.is_file() {
        return Err(
            "tool_unavailable: install coh beside SwarmUI or select its bundle in Settings".into(),
        );
    }
    let schema = run_process(&tool, &["--ui-schema".into()], None)?;
    if !schema.success
        || serde_json::from_str::<Value>(&schema.stdout).ok().as_ref()
            != Some(&coh_cli::ui_schema())
    {
        return Err(
            "tool_mismatch: installed coh and SwarmUI must use the same command schema".into(),
        );
    }
    run_process(&tool, &argv[1..], connection)
}

fn run_process(
    tool: &Path,
    argv: &[String],
    connection: Option<&Connection>,
) -> Result<HostResult, String> {
    let mut command = Command::new(tool);
    command
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "COH_AUTH_TOKEN",
        "COH_AUTH_TOKEN_REF",
        "COH_REST_AUTH_TOKEN",
        "COH_REST_AUTH_TOKEN_REF",
        "SWARMUI_DELEGATED_TICKET",
        "COH_REST_URL",
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
    ] {
        command.env_remove(key);
    }
    if let Some(c) = connection {
        command.env("COH_REST_AUTH_TOKEN", &c.credential);
        if let Some(ticket) = &c.ticket {
            command.env("SWARMUI_DELEGATED_TICKET", ticket);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|_| "tool_unavailable: could not launch the installed coh tool")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("host_io: output pipe unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("host_io: error pipe unavailable")?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&exceeded);
    let out = std::thread::spawn(move || read_output(stdout, flag));
    let flag = Arc::clone(&exceeded);
    let err = std::thread::spawn(move || read_output(stderr, flag));
    let start = Instant::now();
    let (success, outcome) = loop {
        if exceeded.load(Ordering::Acquire) || start.elapsed() >= HOST_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break (false, "interrupted_unknown: controller stopped; use Watch to reconcile the original operation; remote cancellation is not confirmed".to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break (
                    status.success(),
                    if status.success() {
                        "command_completed"
                    } else {
                        "command_failed"
                    }
                    .into(),
                )
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break (false, "process_error: reconcile original operation".into());
            }
        }
    };
    let redact = |bytes: Vec<u8>| {
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(c) = connection {
            text = text.replace(&c.credential, "[redacted]");
            if let Some(ticket) = &c.ticket {
                text = text.replace(ticket, "[redacted]");
            }
        }
        text
    };
    Ok(HostResult {
        success,
        outcome,
        stdout: redact(out.join().map_err(|_| "host_io: output reader failed")?),
        stderr: redact(err.join().map_err(|_| "host_io: error reader failed")?),
    })
}

pub fn tool_directory() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SWARMUI_TOOL_DIR") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe()
        .map_err(|_| "tool_unavailable: executable location unknown".to_owned())?;
    let directory = executable
        .parent()
        .ok_or("tool_unavailable: executable directory unknown")?;
    if directory.ends_with("SwarmUI.app/Contents/MacOS") {
        if let Some(package) = directory.ancestors().nth(3) {
            return Ok(package.join("bin"));
        }
    }
    Ok(directory.to_owned())
}

/// Validate the same bounded namespace contract before creating any console text.
pub fn namespace_command(verb: &str, path: &str, roots: &[String]) -> Result<String, String> {
    if !matches!(verb, "ls" | "cat" | "tail") {
        return Err("invalid_operation: choose list, read or tail".into());
    }
    if !path.starts_with('/')
        || path.len() > cohsh_core::MAX_PATH_LEN
        || path[1..].split('/').count() > 8
        || path.chars().any(char::is_control)
        || path.split('/').any(|p| p == "..")
    {
        return Err(
            "invalid_path: use an absolute bounded namespace without parent traversal".into(),
        );
    }
    if path.split('/').any(|p| p == ".")
        || path.contains("//")
        || path.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err("invalid_path: empty, dot and whitespace components are not allowed".into());
    }
    if !roots
        .iter()
        .any(|root| path == root || path.strip_prefix(root).is_some_and(|s| s.starts_with('/')))
    {
        return Err(
            "unsupported_root: this namespace is not enabled in the selected profile".into(),
        );
    }
    Ok(format!("{verb} {path}"))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub action: String,
    pub fields: BTreeMap<String, String>,
}

/// Guided control forms reuse cohsh's payload builders and normal command parser.
pub fn control_line(request: &ControlRequest, roots: &[String]) -> Result<String, String> {
    if request.fields.len() > 16
        || request
            .fields
            .values()
            .any(|v| v.len() > cohsh_core::command::MAX_LINE_LEN || v.chars().any(char::is_control))
    {
        return Err("invalid_field: control input exceeds bounds".into());
    }
    let get = |key: &str| {
        request
            .fields
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("missing_field: {key}"))
    };
    let catalog = structured_controls();
    if let Some(spec) = catalog
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["action"] == request.action))
    {
        return structured_control_line(request, roots, spec);
    }
    let allowed: &[&str] = match request.action.as_str() {
        "spawn" => &[
            "role",
            "ticks",
            "gpu_id",
            "mem_mb",
            "streams",
            "ttl_s",
            "ops",
            "priority",
            "budget_ttl_s",
            "budget_ops",
        ],
        "kill" => &["id"],
        "budget" => &["ttl_s", "ops"],
        "append" => &["path", "payload"],
        "ping" | "log" | "bootinfo" | "mem" | "test" | "nettest" | "netstats" | "reboot" => &[],
        "caps" | "smp" => &["mode"],
        "cachelog" => &["count"],
        _ => return Err("invalid_operation: unknown control".into()),
    };
    if request
        .fields
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err("invalid_field: unknown control field".into());
    }
    let line = match request.action.as_str() {
        "bootinfo" => "bi".to_owned(),
        "spawn" => {
            let role = get("role")?;
            let args: Vec<_> = request
                .fields
                .iter()
                .filter(|(k, _)| k.as_str() != "role")
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            format!(
                "spawn {}",
                cohsh::queen::spawn(role, args.iter().map(String::as_str))
                    .map_err(|e| format!("invalid_spawn: {e}"))?
                    .trim_end()
            )
        }
        "kill" => {
            cohsh::queen::kill(get("id")?).map_err(|e| format!("invalid_id: {e}"))?;
            format!("kill {}", get("id")?)
        }
        "budget" => {
            let number = |key: &str| {
                request
                    .fields
                    .get(key)
                    .map(|v| {
                        v.parse::<u64>()
                            .map_err(|_| format!("invalid_number: {key}"))
                    })
                    .transpose()
            };
            let payload = cohsh::queen::budget(number("ttl_s")?, number("ops")?)
                .map_err(|e| format!("invalid_budget: {e}"))?;
            format!(
                "echo {} > {}",
                payload.trim_end(),
                cohsh::queen::queen_ctl_path()
            )
        }
        "append" => {
            let path = get("path")?;
            namespace_command("cat", path, roots)?;
            format!("echo {} > {path}", get("payload")?)
        }
        "caps" | "smp" | "cachelog" => {
            let key = if request.action == "cachelog" {
                "count"
            } else {
                "mode"
            };
            match request.fields.get(key) {
                Some(value) if !value.is_empty() && !value.chars().any(char::is_whitespace) => {
                    format!("{} {value}", request.action)
                }
                Some(_) => return Err("invalid_field: select one diagnostic option".into()),
                None => request.action.clone(),
            }
        }
        _ => request.action.clone(),
    };
    cohsh_core::command::CommandParser::parse_line_str(&line)
        .map_err(|_| "invalid_command: structured input exceeds the console contract")?;
    Ok(line)
}

pub fn read_artifact(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|_| "artifact_unavailable: cannot open the selected file")?;
    if !file
        .metadata()
        .map_err(|_| "artifact_unavailable: cannot inspect the selected file")?
        .is_file()
    {
        return Err("invalid_artifact: select a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "artifact_io: could not read the selected file")?;
    if bytes.len() > limit {
        return Err("artifact_oversized: selected file exceeds the generated replay limit".into());
    }
    Ok(bytes)
}

pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Canonical host tooling owns pack inspection and report redaction; the UI receives its result.
pub fn inspect_artifact(tool_dir: &Path, kind: &str, path: &Path) -> Result<Value, String> {
    let schema = run_process(&tool_dir.join("coh"), &["--ui-schema".into()], None)?;
    if !schema.success
        || serde_json::from_str::<Value>(&schema.stdout).ok().as_ref()
            != Some(&coh_cli::ui_schema())
    {
        return Err("tool_mismatch: choose matching installed coh and SwarmUI builds".into());
    }
    let path = path.to_str().ok_or("invalid_artifact: path is not UTF-8")?;
    let argv = match kind {
        "pack" => vec!["inspect".into(), format!("--input={path}"), "--json".into()],
        "report" => vec!["--ui-read-report".into(), path.into()],
        _ => return Err("invalid_artifact: unsupported canonical artifact".into()),
    };
    let result = run_process(&tool_dir.join("coh"), &argv, None)?;
    if !result.success {
        return Err(format!("artifact_refused: {}", result.stderr));
    }
    serde_json::from_str(&result.stdout)
        .map_err(|_| "artifact_format: canonical tool returned invalid JSON".into())
}

/// UI adapters for the documented control files; the target owns admission and state transitions.
pub fn structured_controls() -> Value {
    json!([
        {"action":"lifecycle","name":"Change hive lifecycle","path":cohsh::CLIENT_QUEEN_LIFECYCLE_CTL_PATH,"enabled":true,"fields":[["action","Transition",["cordon","drain","resume","quiesce","reset"]]]},
        {"action":"bind","name":"Bind a namespace","path":cohsh::CLIENT_QUEEN_CTL_PATH,"enabled":true,"fields":[["from","Source path"],["to","Destination path"]]},
        {"action":"mount","name":"Mount a namespace service","path":cohsh::CLIENT_QUEEN_CTL_PATH,"enabled":true,"fields":[["service","Service identity"],["at","Destination path"]]},
        {"action":"schedule","name":"Queue Worker work","path":cohsh::CLIENT_QUEEN_SCHEDULE_CTL_PATH,"enabled":cohsh::CONTROL_SCHEDULE_ENABLED,"fields":[["id","Unique queue identity"],["role","Worker role",["worker-heartbeat","worker-gpu","worker-lora","worker-bus"]],["priority","Priority"],["ticks","Work ticks"],["budget_ms","Time budget (ms)"]]},
        {"action":"dequeue","name":"Accept queued work","path":cohsh::CLIENT_QUEEN_SCHEDULE_CTL_PATH,"enabled":cohsh::CONTROL_SCHEDULE_ENABLED,"fields":[["id","Exact FIFO head identity"]]},
        {"action":"lease-grant","name":"Grant a resource lease","path":cohsh::CLIENT_QUEEN_LEASE_CTL_PATH,"enabled":cohsh::CONTROL_LEASE_ENABLED,"fields":[["id","Lease identity"],["subject","Subject"],["resource","Resource"],["ttl_s","Duration (seconds)"],["priority","Priority"]]},
        {"action":"lease-renew","name":"Renew a resource lease","path":cohsh::CLIENT_QUEEN_LEASE_CTL_PATH,"enabled":cohsh::CONTROL_LEASE_ENABLED,"fields":[["id","Lease identity"],["ttl_s","Duration (seconds)"],["priority","Priority"]]},
        {"action":"lease-preempt","name":"Preempt a resource lease","path":cohsh::CLIENT_QUEEN_LEASE_CTL_PATH,"enabled":cohsh::CONTROL_LEASE_ENABLED,"fields":[["id","Lease identity"],["reason","Reason"]]},
        {"action":"lease-quota","name":"Set resource quotas","path":cohsh::CLIENT_QUEEN_LEASE_CTL_PATH,"enabled":cohsh::CONTROL_LEASE_ENABLED,"fields":[["subject","Subject"],["resource","Resource"],["max_active","Active lease limit"],["max_preemptions","Preemption limit"]]},
        {"action":"export-open","name":"Open an export window","path":cohsh::CLIENT_QUEEN_EXPORT_CTL_PATH,"enabled":cohsh::CONTROL_EXPORT_ENABLED,"fields":[["id","Window identity"],["ttl_s","Duration (seconds)"]]},
        {"action":"export-close","name":"Close an export window","path":cohsh::CLIENT_QUEEN_EXPORT_CTL_PATH,"enabled":cohsh::CONTROL_EXPORT_ENABLED,"fields":[["id","Window identity"],["reason","Reason"]]},
        {"action":"policy-apply","name":"Apply a policy revision","path":cohsh::CLIENT_POLICY_CTL_PATH,"enabled":cohsh::POLICY_ENABLED,"fields":[["id","Revision identity"],["sha256","Revision SHA-256"]]},
        {"action":"policy-rollback","name":"Roll back a policy revision","path":cohsh::CLIENT_POLICY_CTL_PATH,"enabled":cohsh::POLICY_ENABLED,"fields":[["id","Revision identity"]]},
        {"action":"approval","name":"Decide a queued action","path":"/actions/queue","enabled":cohsh::POLICY_ENABLED,"fields":[["id","Single-use decision identity"],["target","Exact control path"],["decision","Decision",["approve","deny"]]]},
        {"action":"audit-replay","name":"Replay retained control actions","path":"/replay/ctl","enabled":true,"fields":[["from","Starting retained cursor"]]}
    ])
}

fn structured_control_line(
    request: &ControlRequest,
    roots: &[String],
    spec: &Value,
) -> Result<String, String> {
    if spec["enabled"] != true {
        return Err("unavailable: this control is disabled by the generated profile".into());
    }
    let path = spec["path"]
        .as_str()
        .ok_or("invalid_control: missing control path")?;
    namespace_command("cat", path, roots)?;
    let fields = spec["fields"]
        .as_array()
        .ok_or("invalid_control: missing field schema")?;
    if request.fields.len() != fields.len() {
        return Err("invalid_field: supply every labelled field, without additional fields".into());
    }
    let mut payload = serde_json::Map::new();
    for field in fields {
        let key = field[0]
            .as_str()
            .ok_or("invalid_control: missing field name")?;
        let value = request
            .fields
            .get(key)
            .ok_or_else(|| format!("missing_field: {key}"))?;
        if value.is_empty() || value.len() > 96 || value.chars().any(char::is_control) {
            return Err(format!(
                "invalid_field: {key} is empty or exceeds the bound"
            ));
        }
        let parsed = if let Some(choices) = field[2].as_array() {
            if !choices.iter().any(|choice| choice == value) {
                return Err(format!("invalid_field: choose a supported {key}"));
            }
            json!(value)
        } else if matches!(
            key,
            "ttl_s" | "priority" | "ticks" | "budget_ms" | "max_active" | "max_preemptions"
        ) || (request.action == "audit-replay" && key == "from")
        {
            let number = value
                .parse::<u64>()
                .map_err(|_| format!("invalid_number: {key}"))?;
            if number == 0 && matches!(key, "ttl_s" | "ticks" | "budget_ms") {
                return Err(format!("invalid_number: {key} must be positive"));
            }
            json!(number)
        } else if key == "sha256" {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("invalid_digest: use 64 lowercase hexadecimal characters".into());
            }
            json!(value)
        } else if matches!(key, "from" | "to" | "at" | "target") {
            namespace_command("cat", value, roots)?;
            json!(value)
        } else {
            cohesix_authority::validate_id(value)
                .map_err(|_| format!("invalid_identifier: {key}"))?;
            json!(value)
        };
        payload.insert(key.to_owned(), parsed);
    }
    let body = match request.action.as_str() {
        "lifecycle" => request
            .fields
            .get("action")
            .ok_or("missing_field: action")?
            .clone(),
        "bind" | "mount" => {
            let mut outer = serde_json::Map::new();
            outer.insert(request.action.clone(), Value::Object(payload));
            Value::Object(outer).to_string()
        }
        "schedule" | "approval" | "audit-replay" => Value::Object(payload).to_string(),
        action => {
            let op = action.split_once('-').map(|(_, op)| op).unwrap_or(action);
            payload.insert("op".into(), json!(op));
            Value::Object(payload).to_string()
        }
    };
    let line = format!("echo {body} > {path}");
    cohsh_core::command::CommandParser::parse_line_str(&line)
        .map_err(|_| "invalid_command: payload exceeds console bounds")?;
    Ok(line)
}
