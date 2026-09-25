// Author: Lukas Bower
// Purpose: Project selected durable gateway jobs through bounded A2A 0.3 JSON-RPC tasks.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! A2A task IDs are the original admission IDs. This projection has no task
//! store, effect queue or verifier; the shared ledger and native result own truth.

use anyhow::{ensure, Context, Result};
use axum::body::{to_bytes, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{sse::Event, sse::Sse, IntoResponse, Response};
use axum::Json;
use futures_util::stream;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock};
use tokio::sync::Semaphore;
use tokio::time::{sleep, timeout, Duration};

use crate::{jobs, mcp, validate_request_auth, AppState};

pub(super) const MAX_REQUEST_BYTES: usize = 8_192;
const MAX_RESPONSE_BYTES: usize = 16_384;
const MAX_CONCURRENT: usize = 16;
const MAX_STREAM_EVENTS: usize = 64;
const STREAM_INTERVAL: Duration = Duration::from_millis(500);
const STREAM_DEADLINE: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
static PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT);
static STREAM_PERMITS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT)));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    jsonrpc: String,
    id: Value,
    method: String,
    params: Value,
}

fn catalogue() -> Result<Value> {
    Ok(serde_json::from_str(include_str!(
        "../../../configs/generated/a2a_catalogue.json"
    ))?)
}

pub(super) fn validate_catalogue(enabled: bool) -> Result<()> {
    let catalogue = catalogue()?;
    let selected = include_bytes!("../../../configs/generated/root_task_resolved.json");
    ensure!(
        catalogue["schema"] == "cohesix-a2a-catalogue/v1"
            && catalogue["revision"] == "0.3.0"
            && catalogue["binding"] == "JSONRPC"
            && catalogue["enabled"] == enabled
            && catalogue["resolved_manifest_sha256"] == hex::encode(Sha256::digest(selected)),
        "EPERM generated A2A catalogue mismatch"
    );
    let skills = catalogue["skills"].as_array().context("A2A skills")?;
    ensure!(enabled || skills.is_empty(), "EPERM disabled A2A skills");
    Ok(())
}

fn reject(status: StatusCode, reason: &'static str) -> Response {
    (status, Json(json!({"error":reason}))).into_response()
}

fn rpc_error(id: Value, code: i32, message: &'static str) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})).into_response()
}

fn rpc_result(id: Value, result: Value) -> Response {
    let response = json!({"jsonrpc":"2.0","id":id,"result":result});
    if serde_json::to_vec(&response).is_ok_and(|bytes| bytes.len() <= MAX_RESPONSE_BYTES) {
        Json(response).into_response()
    } else {
        rpc_error(id, -32000, "bounded response unavailable")
    }
}

fn valid_id(value: &Value) -> bool {
    value.as_str().is_some_and(|id| id.len() <= 64)
        || value.as_i64().is_some()
        || value.as_u64().is_some()
}

fn task_id(value: &Value) -> Option<&str> {
    let id = value.get("id")?.as_str()?;
    if id.len() > 96 || cohesix_authority::validate_id(id).is_err() {
        return None;
    }
    Some(id)
}

async fn response_value(response: Response) -> Result<(StatusCode, Value)> {
    let status = response.status();
    let body = to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?;
    Ok((status, serde_json::from_slice(&body)?))
}

/// A confirmed ledger entry still needs its matching native terminal. An
/// uncertain result stays unknown, and cancellation is terminal only after
/// the ledger confirms that no effect was dispatched.
fn task_from_reconciliation(result: &Value) -> Result<Value> {
    let record = result.get("record").context("job record missing")?;
    let binding = record.get("binding").context("job binding missing")?;
    let id = binding["admission_id"]
        .as_str()
        .context("admission ID missing")?;
    cohesix_authority::validate_id(id).map_err(|_| anyhow::anyhow!("invalid admission ID"))?;
    let execution = record["execution"].as_str().context("execution missing")?;
    let rows = result["target_results"]
        .as_array()
        .context("target results missing")?;
    let native = rows.last().and_then(|row| row["state"].as_str());
    let state = match execution {
        "reserved" => "submitted",
        "dispatching" => "working",
        "uncertain" => "unknown",
        "refused_no_effect" if record["cancel_requested"] == true => "canceled",
        "refused_no_effect" => "rejected",
        "confirmed" => match native {
            Some("running") => "working",
            Some("succeeded") => "completed",
            Some("cancelled" | "canceled") => "canceled",
            Some("failed" | "recovered_failure") => "failed",
            _ => "unknown",
        },
        _ => return Err(anyhow::anyhow!("unrecognised execution state")),
    };
    let digest = record["result_sha256"].as_str();
    let artifacts = if let Some(digest) = digest {
        vec![json!({
            "artifactId":format!("{id}-result"),
            "name":"Original native result reference",
            "parts":[{"kind":"data","data":{
                "uri":format!("cohesix://jobs/{id}/result"),
                "sha256":digest,
                "receiptStatus":"requires shared verifier",
            }}],
        })]
    } else {
        Vec::new()
    };
    Ok(json!({
        "kind":"task",
        "id":id,
        "contextId":id,
        "status":{"state":state},
        "artifacts":artifacts,
        "metadata":{
            "schema":"cohesix-a2a-task/v1",
            "admissionId":id,
            "ticketId":binding["ticket_id"],
            "idempotencyKey":binding["idempotency_key"],
            "action":binding["action"],
            "target":binding["target"],
            "execution":execution,
            "delivery":record["delivery"],
            "nativeOutcome":native,
            "cancelRequested":record["cancel_requested"],
            "resultSha256":digest,
            "effectReplayAllowed":false,
            "providerVerified":false,
        }
    }))
}

async fn inspect(state: AppState, headers: HeaderMap, id: String) -> Result<Value> {
    let response = jobs::reconcile(State(state.clone()), Path(id.clone()), headers.clone()).await;
    let (status, value) = response_value(response).await?;
    if status.is_success() {
        return task_from_reconciliation(&value);
    }
    ensure!(
        status == StatusCode::SERVICE_UNAVAILABLE,
        "job unavailable to subject"
    );
    let (status, record) =
        response_value(jobs::status(State(state), Path(id), headers).await).await?;
    ensure!(status.is_success(), "job unavailable to subject");
    task_from_reconciliation(&json!({"record":record,"target_results":[]}))
}

fn task_data(params: &Value) -> Result<Value> {
    let message = params.get("message").context("message missing")?;
    ensure!(message["role"] == "user", "user role required");
    ensure!(
        message["messageId"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 96),
        "bounded message ID required"
    );
    ensure!(
        message.get("taskId").is_none() && message.get("contextId").is_none(),
        "new task cannot amend another task"
    );
    let parts = message["parts"]
        .as_array()
        .context("message parts missing")?;
    ensure!(
        parts.len() == 1 && parts[0]["kind"] == "data",
        "one data part required"
    );
    let data = &parts[0]["data"];
    ensure!(data.is_object(), "data object required");
    let skill = data["skillId"].as_str().context("skill ID missing")?;
    let selected = catalogue()?["skills"]
        .as_array()
        .context("selected skills missing")?
        .iter()
        .any(|row| row["id"] == skill);
    ensure!(selected, "skill not selected");
    ensure!(
        data.as_object().is_some_and(|object| object.len() == 3),
        "unknown task data"
    );
    ensure!(
        data.get("binding").is_some() ^ data.get("scopeId").is_some(),
        "one admission form required"
    );
    ensure!(data["ticket"]["action"] == skill, "skill/action mismatch");
    Ok(data.clone())
}

async fn prepare_request(
    state: AppState,
    headers: HeaderMap,
    data: &Value,
) -> Result<jobs::SubmitRequest> {
    let request: jobs::SubmitRequest = if let Some(scope_id) = data.get("scopeId") {
        let preflight: jobs::PreflightRequest = serde_json::from_value(json!({
            "scope_id":scope_id,"ticket":data["ticket"]
        }))?;
        let (status, value) =
            response_value(jobs::preflight(State(state), headers, Json(preflight)).await).await?;
        ensure!(status.is_success(), "selected preflight refused");
        serde_json::from_value(value["request"].clone())?
    } else {
        serde_json::from_value(json!({
            "binding":data["binding"],"ticket":data["ticket"]
        }))?
    };
    ensure!(
        serde_json::to_value(&request)?["binding"]["action"] == data["skillId"],
        "skill/action mismatch"
    );
    Ok(request)
}

async fn dispatch(state: AppState, headers: HeaderMap, request: Request) -> Response {
    let id = request.id;
    match request.method.as_str() {
        "message/send" | "message/stream" => {
            let data = match task_data(&request.params) {
                Ok(data) => data,
                Err(_) => return rpc_error(id, -32602, "invalid selected job request"),
            };
            let payload = match prepare_request(state.clone(), headers.clone(), &data).await {
                Ok(payload) => payload,
                Err(_) => return rpc_error(id, -32000, "selected job preflight refused"),
            };
            let admission = match serde_json::to_value(&payload) {
                Ok(value) => value["binding"]["admission_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                Err(_) => return rpc_error(id, -32602, "invalid selected job request"),
            };
            let response = jobs::submit(State(state.clone()), headers.clone(), Json(payload)).await;
            let Ok((status, _)) = response_value(response).await else {
                return rpc_error(id, -32000, "gateway result unavailable");
            };
            if !status.is_success() && status != StatusCode::CONFLICT {
                return rpc_error(id, -32000, "selected job refused");
            }
            let task = match inspect(state.clone(), headers.clone(), admission.clone()).await {
                Ok(task) => task,
                Err(_) => return rpc_error(id, -32000, "original job unavailable"),
            };
            if request.method == "message/stream" {
                return stream_tasks(state, headers, admission, id, task);
            }
            rpc_result(id, task)
        }
        "tasks/get" | "tasks/cancel" | "tasks/resubscribe" => {
            let Some(admission) = task_id(&request.params).map(str::to_owned) else {
                return rpc_error(id, -32602, "invalid task ID");
            };
            if request.method == "tasks/cancel" {
                let response = jobs::cancel(
                    State(state.clone()),
                    Path(admission.clone()),
                    headers.clone(),
                )
                .await;
                if !response.status().is_success() {
                    return rpc_error(id, -32000, "cancellation refused");
                }
            }
            let task = match inspect(state.clone(), headers.clone(), admission.clone()).await {
                Ok(task) => task,
                Err(_) => return rpc_error(id, -32001, "task unavailable"),
            };
            if request.method == "tasks/resubscribe" {
                return stream_tasks(state, headers, admission, id, task);
            }
            rpc_result(id, task)
        }
        "tasks/pushNotificationConfig/set"
        | "tasks/pushNotificationConfig/get"
        | "tasks/pushNotificationConfig/list"
        | "tasks/pushNotificationConfig/delete" => {
            rpc_error(id, -32004, "push callbacks unsupported")
        }
        _ => rpc_error(id, -32601, "method not found"),
    }
}

fn stream_tasks(
    state: AppState,
    headers: HeaderMap,
    admission: String,
    rpc_id: Value,
    first: Value,
) -> Response {
    let Ok(permit) = STREAM_PERMITS.clone().try_acquire_owned() else {
        return reject(
            StatusCode::TOO_MANY_REQUESTS,
            "A2A stream capacity exhausted",
        );
    };
    let started = std::time::Instant::now();
    let stream = stream::unfold(
        (
            state,
            headers,
            admission,
            rpc_id,
            Some(first),
            None::<String>,
            0usize,
            started,
            permit,
        ),
        |(state, headers, admission, rpc_id, first, previous, count, started, permit)| async move {
            if count >= MAX_STREAM_EVENTS || started.elapsed() >= STREAM_DEADLINE {
                return None;
            }
            let task = if let Some(task) = first {
                task
            } else {
                sleep(STREAM_INTERVAL).await;
                inspect(state.clone(), headers.clone(), admission.clone())
                    .await
                    .ok()?
            };
            let serialized = task.to_string();
            if previous.as_deref() == Some(serialized.as_str()) {
                return Some((
                    Ok::<Event, std::convert::Infallible>(Event::default().comment("unchanged")),
                    (
                        state,
                        headers,
                        admission,
                        rpc_id,
                        None,
                        previous,
                        count + 1,
                        started,
                        permit,
                    ),
                ));
            }
            let terminal = matches!(
                task["status"]["state"].as_str(),
                Some("completed" | "failed" | "canceled" | "rejected")
            );
            let data = json!({"jsonrpc":"2.0","id":rpc_id,"result":task});
            let event = Event::default().data(data.to_string());
            let next_count = if terminal {
                MAX_STREAM_EVENTS
            } else {
                count + 1
            };
            Some((
                Ok::<Event, std::convert::Infallible>(event),
                (
                    state,
                    headers,
                    admission,
                    rpc_id,
                    None,
                    Some(serialized),
                    next_count,
                    started,
                    permit,
                ),
            ))
        },
    );
    Sse::new(stream).into_response()
}

pub(super) async fn agent_card(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match jobs::authorize_status_principal(&state, &headers) {
        Ok(principal) => principal,
        Err(_) => return reject(StatusCode::UNAUTHORIZED, "delegated read required"),
    };
    let Some(host) = headers.get("host").and_then(|value| value.to_str().ok()) else {
        return reject(StatusCode::BAD_REQUEST, "host required");
    };
    if host.len() > 255
        || !host.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
    {
        return reject(StatusCode::BAD_REQUEST, "invalid host");
    }
    let actions = match jobs::available_actions(&state, &principal.subject) {
        Ok(actions) => actions,
        Err(_) => return reject(StatusCode::FORBIDDEN, "selected skills unavailable"),
    };
    let Ok(catalogue) = catalogue() else {
        return reject(StatusCode::INTERNAL_SERVER_ERROR, "catalogue unavailable");
    };
    let skills: Vec<_> = catalogue["skills"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            row["id"]
                .as_str()
                .is_some_and(|id| actions.iter().any(|a| a == id))
        })
        .map(|row| {
            json!({
                "id":row["id"],"name":row["name"],"description":row["description"],
                "tags":row["tags"],
            })
        })
        .collect();
    Json(json!({
        "name":"Cohesix selected durable jobs",
        "description":"Scoped CUDA and PEFT delegation over existing Cohesix admissions and results.",
        "url":format!("http://{host}/a2a"),
        "version":"1.1.0b1",
        "protocolVersion":"0.3.0",
        "preferredTransport":"JSONRPC",
        "capabilities":{"streaming":true,"pushNotifications":false,
            "stateTransitionHistory":false},
        "defaultInputModes":["application/json"],
        "defaultOutputModes":["application/json"],
        "skills":skills,
    }))
    .into_response()
}

pub(super) async fn post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !mcp::valid_origin(&headers) {
        return reject(StatusCode::FORBIDDEN, "origin refused");
    }
    if headers.get_all("content-type").iter().count() != 1
        || !headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|value| value.trim() == "application/json")
            })
    {
        return reject(StatusCode::UNSUPPORTED_MEDIA_TYPE, "JSON required");
    }
    if headers.get_all("a2a-version").iter().count() > 1
        || headers
            .get("a2a-version")
            .is_some_and(|value| value != "0.3.0")
    {
        return reject(StatusCode::BAD_REQUEST, "unsupported A2A revision");
    }
    if validate_request_auth(&headers, state.request_auth_token()).is_err()
        || jobs::authorize_status_principal(&state, &headers).is_err()
    {
        return reject(StatusCode::UNAUTHORIZED, "delegated identity required");
    }
    if body.len() > MAX_REQUEST_BYTES {
        return reject(StatusCode::PAYLOAD_TOO_LARGE, "request too large");
    }
    let Ok(request) = serde_json::from_slice::<Request>(&body) else {
        return rpc_error(Value::Null, -32700, "invalid JSON-RPC request");
    };
    if request.jsonrpc != "2.0" || !valid_id(&request.id) {
        return rpc_error(Value::Null, -32600, "invalid JSON-RPC request");
    }
    let Ok(_permit) = PERMITS.try_acquire() else {
        return reject(StatusCode::TOO_MANY_REQUESTS, "A2A capacity exhausted");
    };
    match timeout(CALL_TIMEOUT, dispatch(state, headers, request)).await {
        Ok(response) => response,
        Err(_) => rpc_error(
            Value::Null,
            -32000,
            "A2A call timed out; inspect original task",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_requires_native_outcome_and_uncertain_stays_unknown() {
        let mut result = json!({
            "record":{"binding":{"admission_id":"job-1","ticket_id":"ticket-1",
                "idempotency_key":"original-1","action":"gpu.workload.submit",
                "target":"gpu-0"},"execution":"uncertain","delivery":"pending",
                "cancel_requested":true,"result_sha256":null},
            "target_results":[],
        });
        assert_eq!(
            task_from_reconciliation(&result).unwrap()["status"]["state"],
            "unknown"
        );
        result["record"]["execution"] = "confirmed".into();
        result["record"]["result_sha256"] = json!("a".repeat(64));
        assert_eq!(
            task_from_reconciliation(&result).unwrap()["status"]["state"],
            "unknown"
        );
        result["target_results"] = json!([{"state":"running"}]);
        assert_eq!(
            task_from_reconciliation(&result).unwrap()["status"]["state"],
            "working"
        );
        result["target_results"] = json!([{"state":"recovered_failure"}]);
        assert_eq!(
            task_from_reconciliation(&result).unwrap()["status"]["state"],
            "failed"
        );
        result["target_results"] = json!([{"state":"succeeded"}]);
        let task = task_from_reconciliation(&result).unwrap();
        assert_eq!(task["status"]["state"], "completed");
        assert_eq!(task["metadata"]["providerVerified"], false);
    }
}
