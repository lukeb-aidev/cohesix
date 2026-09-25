// Author: Lukas Bower
// Purpose: Project selected admitted jobs through bounded MCP Streamable HTTP messages.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! MCP carries existing selected job requests and observations. It never
//! constructs a provider receipt or treats a tool reply as a terminal result.

use std::io::{self, BufRead, Write};
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use axum::body::{to_bytes, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

use crate::{authority_now_ms, jobs, validate_request_auth, AppState};

pub(super) const REVISION: &str = "2025-11-25";
pub(super) const MAX_REQUEST_BYTES: usize = 8_192;
const MAX_RESPONSE_BYTES: usize = 16_384;
const MAX_CONCURRENT_REQUESTS: usize = 16;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
static PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_REQUESTS);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

fn rpc_result(id: &Value, result: Value) -> Response {
    let message = json!({"jsonrpc":"2.0","id":id,"result":result});
    if serde_json::to_vec(&message).is_ok_and(|bytes| bytes.len() <= MAX_RESPONSE_BYTES) {
        Json(message).into_response()
    } else {
        rpc_error(id.clone(), -32001, "bounded MCP response unavailable")
    }
}

fn rpc_error(id: Value, code: i32, message: &'static str) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})).into_response()
}

fn reject(status: StatusCode, message: &'static str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}

fn valid_origin(headers: &HeaderMap) -> bool {
    if headers.get_all("origin").iter().count() == 0 {
        return true;
    }
    if headers.get_all("origin").iter().count() != 1 || headers.get_all("host").iter().count() != 1
    {
        return false;
    }
    let Some(host) = headers.get("host").and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let local = host == "localhost"
        || host == "127.0.0.1"
        || host.starts_with("localhost:")
        || host.starts_with("127.0.0.1:");
    local && origin == format!("http://{host}")
}

fn valid_accept(headers: &HeaderMap) -> bool {
    headers
        .get("accept")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            let accepts = |media_type| {
                value.split(',').any(|part| {
                    let mut parameters = part.trim().split(';');
                    parameters.next() == Some(media_type)
                        && parameters.all(|parameter| {
                            let parameter = parameter.trim();
                            parameter.strip_prefix("q=").is_none_or(|quality| {
                                quality
                                    .parse::<f32>()
                                    .is_ok_and(|value| value > 0.0 && value <= 1.0)
                            })
                        })
                })
            };
            accepts("application/json") && accepts("text/event-stream")
        })
}

fn valid_id(id: &Value) -> bool {
    id.as_str().is_some_and(|value| value.len() <= 64)
        || id.as_i64().is_some()
        || id.as_u64().is_some()
}

fn generated_catalogue() -> Option<Value> {
    serde_json::from_str(include_str!(
        "../../../configs/generated/mcp_catalogue.json"
    ))
    .ok()
}

pub(super) fn validate_catalogue(enabled: bool) -> Result<()> {
    let catalogue = generated_catalogue().context("generated MCP catalogue missing")?;
    let controls = cohesix_authority::standing::StandingControls::from_resolved_manifest(
        include_bytes!("../../../configs/generated/root_task_resolved.json"),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    let actions: Vec<_> = catalogue["actions"]
        .as_array()
        .context("generated MCP actions missing")?
        .iter()
        .map(|action| action["id"].as_str().map(str::to_owned))
        .collect::<Option<_>>()
        .context("generated MCP action id missing")?;
    let manifest_digest = hex::encode(Sha256::digest(include_bytes!(
        "../../../configs/generated/root_task_resolved.json"
    )));
    let provider_graph = cohesix_authority::provider::registry()?["graph_sha256"]
        .as_str()
        .context("generated provider graph missing")?
        .to_owned();
    anyhow::ensure!(
        catalogue["schema"] == "cohesix-mcp-catalogue/v1"
            && catalogue["enabled"] == enabled
            && catalogue["revision"] == REVISION
            && catalogue["bounds"]["request_bytes"] == MAX_REQUEST_BYTES
            && catalogue["bounds"]["response_bytes"] == MAX_RESPONSE_BYTES
            && catalogue["bounds"]["concurrent_requests"] == MAX_CONCURRENT_REQUESTS
            && catalogue["bounds"]["call_timeout_ms"] == CALL_TIMEOUT.as_millis() as u64
            && catalogue["bounds"]["session_mode"] == "stateless"
            && catalogue["transports"] == json!(["streamable-http", "stdio"])
            && catalogue["http_path"] == "/mcp"
            && actions == controls.actions
            && catalogue["resolved_manifest_sha256"] == manifest_digest
            && catalogue["provider_graph_sha256"] == provider_graph,
        "EPERM generated MCP catalogue drift"
    );
    Ok(())
}

fn catalogue(actions: &[String], can_write: bool) -> Option<Vec<Value>> {
    let generated = generated_catalogue()?;
    let rows = generated["tools"].as_array()?;
    let mut tools = Vec::new();
    for row in rows {
        let read_only = row["read_only"].as_bool()?;
        if !read_only && !can_write {
            continue;
        }
        let selected = row["selected_actions"].as_array()?;
        if !selected.is_empty()
            && !selected
                .iter()
                .filter_map(Value::as_str)
                .any(|id| actions.iter().any(|action| action == id))
        {
            continue;
        }
        tools.push(json!({
            "name":row["name"],
            "description":row["description"],
            "inputSchema":row["input_schema"],
            "outputSchema":row["output_schema"],
            "annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only}
        }));
    }
    Some(tools)
}

fn visible_catalogue(state: &AppState, headers: &HeaderMap) -> Option<Vec<Value>> {
    let principal = jobs::authorize_status_principal(state, headers).ok()?;
    let now = authority_now_ms().ok()?;
    let can_write = state.inner.delegation.lock().ok()?.permits_path(
        &principal.ticket_hash,
        "/host/tickets/spec",
        true,
        now,
    );
    let actions = jobs::available_actions(state, &principal.subject).ok()?;
    catalogue(&actions, can_write)
}

async fn project(response: Response) -> Value {
    let status = response.status();
    let body = match to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return json!({"content":[{"type":"text","text":"bounded gateway response unavailable"}],"isError":true})
        }
    };
    let value = serde_json::from_slice::<Value>(&body)
        .unwrap_or_else(|_| json!({"error":"invalid gateway response"}));
    let is_error = !status.is_success();
    let text = value.to_string();
    json!({"content":[{"type":"text","text":text}],"structuredContent":value,"isError":is_error})
}

fn admission_id(arguments: &Value) -> Result<String, &'static str> {
    let id = arguments
        .get("admission_id")
        .and_then(Value::as_str)
        .ok_or("admission_id required")?;
    if id.len() > 96 || cohesix_authority::validate_id(id).is_err() {
        return Err("invalid admission_id");
    }
    Ok(id.to_owned())
}

async fn call(state: AppState, headers: HeaderMap, params: &Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return json!({"content":[{"type":"text","text":"tool name required"}],"isError":true});
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        return json!({"content":[{"type":"text","text":"object arguments required"}],"isError":true});
    }
    let response = match name {
        "cohesix.available_services"
            if arguments.as_object().is_some_and(serde_json::Map::is_empty) =>
        {
            jobs::available_scopes(State(state), headers).await
        }
        "cohesix.available_selected_jobs"
            if arguments.as_object().is_some_and(serde_json::Map::is_empty) =>
        {
            jobs::available_selected(State(state), headers).await
        }
        "cohesix.start_approved_service" => {
            if arguments.as_object().is_none_or(|map| map.len() != 2) {
                return invalid_arguments();
            }
            let Some(scope_id) = arguments.get("scope_id").and_then(Value::as_str) else {
                return invalid_arguments();
            };
            if cohesix_authority::validate_id(scope_id).is_err() || scope_id.len() > 96 {
                return invalid_arguments();
            }
            let request = match serde_json::from_value::<jobs::ApprovedStartRequest>(
                json!({"request_id":arguments.get("request_id")}),
            ) {
                Ok(request) => request,
                Err(_) => return invalid_arguments(),
            };
            jobs::start_approved(
                State(state),
                Path(scope_id.to_owned()),
                headers,
                Json(request),
            )
            .await
        }
        "cohesix.submit_selected_job" => {
            let request = match serde_json::from_value::<jobs::SubmitRequest>(arguments) {
                Ok(request) => request,
                Err(_) => return invalid_arguments(),
            };
            jobs::submit(State(state), headers, Json(request)).await
        }
        "cohesix.preflight_selected_job" => {
            let request = match serde_json::from_value::<jobs::PreflightRequest>(arguments) {
                Ok(request) => request,
                Err(_) => return invalid_arguments(),
            };
            jobs::preflight(State(state), headers, Json(request)).await
        }
        "cohesix.inspect_job" | "cohesix.request_cancel" | "cohesix.recover_job" => {
            if arguments.as_object().is_none_or(|map| map.len() != 1) {
                return invalid_arguments();
            }
            let id = match admission_id(&arguments) {
                Ok(id) => id,
                Err(_) => return invalid_arguments(),
            };
            match name {
                "cohesix.inspect_job" => jobs::status(State(state), Path(id), headers).await,
                "cohesix.request_cancel" => jobs::cancel(State(state), Path(id), headers).await,
                _ => jobs::reconcile(State(state), Path(id), headers).await,
            }
        }
        _ => return json!({"content":[{"type":"text","text":"tool unavailable"}],"isError":true}),
    };
    project(response).await
}

fn invalid_arguments() -> Value {
    json!({"content":[{"type":"text","text":"invalid tool arguments"}],"isError":true})
}

/// Serve one stateless MCP request on the already authenticated gateway bind.
pub(super) async fn post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !valid_origin(&headers) {
        return reject(StatusCode::FORBIDDEN, "invalid Origin");
    }
    if validate_request_auth(&headers, state.request_auth_token()).is_err() {
        return reject(StatusCode::UNAUTHORIZED, "request authentication required");
    }
    if !valid_accept(&headers) {
        return reject(
            StatusCode::NOT_ACCEPTABLE,
            "MCP requires JSON and event-stream Accept types",
        );
    }
    if headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value.split(';').next().map(str::trim) != Some("application/json"))
    {
        return reject(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "MCP requires application/json",
        );
    }
    let permit = match PERMITS.try_acquire() {
        Ok(permit) => permit,
        Err(_) => return reject(StatusCode::TOO_MANY_REQUESTS, "MCP concurrency limit"),
    };
    let request = match serde_json::from_slice::<Request>(&body) {
        Ok(request) if request.jsonrpc == "2.0" => request,
        _ => return rpc_error(Value::Null, -32600, "invalid JSON-RPC request"),
    };
    if request.id.as_ref().is_some_and(|id| !valid_id(id)) {
        return rpc_error(Value::Null, -32600, "invalid JSON-RPC id");
    }
    if request.method == "initialize" {
        let Some(id) = request.id else {
            return rpc_error(Value::Null, -32600, "initialize requires an id");
        };
        if request
            .params
            .get("protocolVersion")
            .and_then(Value::as_str)
            != Some(REVISION)
        {
            return rpc_error(id, -32602, "unsupported MCP revision");
        }
        return rpc_result(
            &id,
            json!({
                "protocolVersion":REVISION,
                "capabilities":{"tools":{},"resources":{}},
                "serverInfo":{"name":"cohesix-hive-gateway","version":env!("CARGO_PKG_VERSION")},
                "instructions":"Tool replies report gateway admission or observation only. Inspect and recover the original job for a verified native outcome."
            }),
        );
    }
    if headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        != Some(REVISION)
    {
        return reject(StatusCode::BAD_REQUEST, "unsupported MCP revision header");
    }
    if request.method == "notifications/initialized" && request.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let Some(id) = request.id else {
        return rpc_error(Value::Null, -32600, "invalid JSON-RPC notification");
    };
    let result = match request.method.as_str() {
        "ping" => json!({}),
        "tools/list" => match visible_catalogue(&state, &headers) {
            Some(tools) => json!({"tools":tools}),
            None => return reject(StatusCode::FORBIDDEN, "delegated read required"),
        },
        "tools/call" => {
            let Some(tools) = visible_catalogue(&state, &headers) else {
                return reject(StatusCode::FORBIDDEN, "delegated read required");
            };
            let selected = request.params.get("name").and_then(Value::as_str);
            if !tools.iter().any(|tool| tool["name"].as_str() == selected) {
                return rpc_error(id, -32601, "tool unavailable for caller");
            }
            let operation = timeout(CALL_TIMEOUT, call(state, headers, &request.params)).await;
            match operation {
                Ok(result) => result,
                Err(_) => {
                    json!({"content":[{"type":"text","text":"gateway operation timed out; recover the original identity before another submit"}],"isError":true})
                }
            }
        }
        "resources/list" => {
            if jobs::authorize_status(&state, &headers).is_err() {
                return reject(StatusCode::FORBIDDEN, "delegated read required");
            }
            json!({"resources":[]})
        }
        "resources/templates/list" => {
            if jobs::authorize_status(&state, &headers).is_err() {
                return reject(StatusCode::FORBIDDEN, "delegated read required");
            }
            json!({"resourceTemplates":[{
            "uriTemplate":"cohesix://jobs/{admission_id}",
            "name":"Selected job observation",
            "description":"Read the original scoped job state; no model bytes or execution side effect.",
            "mimeType":"application/json"
            }]})
        }
        "resources/read" => {
            let uri = request
                .params
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or("");
            let Some(job_id) = uri.strip_prefix("cohesix://jobs/") else {
                return rpc_error(id, -32602, "invalid job resource URI");
            };
            if cohesix_authority::validate_id(job_id).is_err() || job_id.len() > 96 {
                return rpc_error(id, -32602, "invalid job resource URI");
            }
            let response = jobs::status(State(state), Path(job_id.to_owned()), headers).await;
            if !response.status().is_success() {
                return rpc_error(id, -32001, "job resource unavailable");
            }
            let body = match to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await {
                Ok(body) => body,
                Err(_) => return rpc_error(id, -32001, "job resource unavailable"),
            };
            let text = match std::str::from_utf8(&body) {
                Ok(text) => text,
                Err(_) => return rpc_error(id, -32001, "job resource unavailable"),
            };
            json!({"contents":[{"uri":uri,"mimeType":"application/json","text":text}]})
        }
        _ => return rpc_error(id, -32601, "MCP method unavailable"),
    };
    drop(permit);
    rpc_result(&id, result)
}

pub(super) async fn method_not_allowed(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !valid_origin(&headers) {
        return reject(StatusCode::FORBIDDEN, "invalid Origin");
    }
    if validate_request_auth(&headers, state.request_auth_token()).is_err() {
        return reject(StatusCode::UNAUTHORIZED, "request authentication required");
    }
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

/// Read one bounded stdio message without allocating from an untrusted length.
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unterminated MCP stdio frame",
                ))
            };
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(available.len(), |index| index + 1);
        if frame.len().saturating_add(count) > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP stdio frame too large",
            ));
        }
        frame.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            frame.pop();
            return Ok(Some(frame));
        }
    }
}

/// Run the packaged local transport with a caller ticket resolved at startup.
/// Only JSON-RPC messages are written to stdout; diagnostics remain on stderr.
pub(super) async fn serve_stdio(state: AppState, ticket_ref: &str) -> Result<()> {
    let ticket = cohesix_authority::secret::resolve_reference(ticket_ref)
        .context("resolve MCP stdio delegated ticket")?;
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("localhost"));
    headers.insert(
        "accept",
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("mcp-protocol-version", HeaderValue::from_static(REVISION));
    headers.insert(
        "x-cohesix-auth",
        HeaderValue::from_str(state.request_auth_token())?,
    );
    headers.insert("x-cohesix-ticket", HeaderValue::from_str(&ticket)?);
    loop {
        let frame = {
            let stdin = io::stdin();
            read_frame(&mut stdin.lock())?
        };
        let Some(frame) = frame else {
            break;
        };
        let request_id = serde_json::from_slice::<Value>(&frame)
            .ok()
            .and_then(|value| value.get("id").cloned())
            .unwrap_or(Value::Null);
        let response = post(State(state.clone()), headers.clone(), Bytes::from(frame)).await;
        if response.status() == StatusCode::ACCEPTED {
            continue;
        }
        let status = response.status();
        let body = to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?;
        let output = if status.is_success() && serde_json::from_slice::<Value>(&body).is_ok() {
            body.to_vec()
        } else {
            json!({
                "jsonrpc":"2.0",
                "id":request_id,
                "error":{"code":-32000,"message":format!("MCP transport refused request ({status})")}
            })
            .to_string()
            .into_bytes()
        };
        let stdout = io::stdout();
        let mut writer = stdout.lock();
        writer.write_all(&output)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    state.inner.shutdown.store(true, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_requires_a_terminated_bounded_message() {
        assert_eq!(
            read_frame(&mut io::Cursor::new(b"{\"id\":1}\n".to_vec())).unwrap(),
            Some(b"{\"id\":1}".to_vec())
        );
        assert_eq!(
            read_frame(&mut io::Cursor::new(b"{\"id\":1}".to_vec()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut oversized = vec![b'x'; MAX_REQUEST_BYTES];
        oversized.push(b'\n');
        assert_eq!(
            read_frame(&mut io::Cursor::new(oversized))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
