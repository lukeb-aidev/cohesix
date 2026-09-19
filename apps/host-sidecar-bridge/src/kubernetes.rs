// Author: Lukas Bower
// Purpose: Observe Kubernetes UID/resourceVersion identities and use conditional cordon and eviction APIs without force deletion.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, ensure, Result};
use reqwest::blocking::Client;
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::time::{Duration, Instant};

const MAX_BYTES: u64 = 65_536;
const MAX_ROWS: usize = 64;

/// Only public resource identities and bounded status fields leave the API adapter.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NodeObservation {
    /// Native node name.
    pub name: String,
    /// Immutable Kubernetes object UID.
    pub uid: String,
    /// Opaque resourceVersion cursor; never treated as a numeric counter.
    pub resource_version: String,
    /// Observed metadata generation if this API server exposes it.
    pub generation: Option<u64>,
    /// Native scheduling flag.
    pub unschedulable: bool,
    /// Readiness condition; absent is explicitly unknown.
    pub ready: Option<bool>,
}

/// Exact pod identity used for policy/v1 Eviction preconditions.
#[derive(Debug, Clone, Serialize)]
pub struct PodIdentity {
    /// Pod namespace.
    pub namespace: String,
    /// Pod name.
    pub name: String,
    /// Immutable pod UID.
    pub uid: String,
    /// Precondition cursor.
    pub resource_version: String,
}

fn token(value: &str) -> Result<&str> {
    cohesix_authority::validate_id(value).map_err(|_| anyhow!("EPERM kubernetes identifier"))?;
    Ok(value)
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    token(
        value[name]
            .as_str()
            .ok_or_else(|| anyhow!("invalid_observation kubernetes identity"))?,
    )
}

/// Parse structured native node state; CLI tables and mutable labels are not authority.
pub fn parse_node(value: &Value) -> Result<NodeObservation> {
    ensure!(
        value["kind"] == "Node" && value["apiVersion"] == "v1",
        "invalid_observation kubernetes kind"
    );
    let meta = &value["metadata"];
    let conditions = value["status"]["conditions"]
        .as_array()
        .ok_or_else(|| anyhow!("invalid_observation kubernetes conditions"))?;
    ensure!(conditions.len() <= 32, "ELIMIT kubernetes conditions");
    let ready = conditions
        .iter()
        .filter(|entry| entry["type"] == "Ready")
        .collect::<Vec<_>>();
    ensure!(
        ready.len() <= 1,
        "invalid_observation duplicate node readiness"
    );
    let ready = match ready
        .first()
        .and_then(|condition| condition["status"].as_str())
    {
        Some("True") => Some(true),
        Some("False") => Some(false),
        Some("Unknown") | None => None,
        _ => bail!("invalid_observation kubernetes readiness"),
    };
    let unschedulable = match value["spec"].get("unschedulable") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => bail!("invalid_observation kubernetes scheduling"),
    };
    Ok(NodeObservation {
        name: field(meta, "name")?.into(),
        uid: field(meta, "uid")?.into(),
        resource_version: field(meta, "resourceVersion")?.into(),
        generation: meta["generation"].as_u64(),
        unschedulable,
        ready,
    })
}

fn rows(value: &Value, kind: &str) -> Result<Vec<Value>> {
    ensure!(
        value["kind"] == kind && value["apiVersion"] == "v1",
        "invalid_observation kubernetes list"
    );
    ensure!(
        value["metadata"]["continue"]
            .as_str()
            .is_none_or(str::is_empty),
        "ELIMIT kubernetes paginated inventory"
    );
    let items = value["items"]
        .as_array()
        .ok_or_else(|| anyhow!("invalid_observation kubernetes rows"))?;
    ensure!(items.len() <= MAX_ROWS, "ELIMIT kubernetes rows");
    Ok(items.clone())
}

/// Whole-operation deadline and TLS credentials are held only by this native API owner.
pub struct Kubernetes {
    client: Client,
    endpoint: Url,
    bearer: String,
    deadline: Instant,
}

impl Kubernetes {
    /// Resolve deployment-owned endpoint and credential refs; absent configuration is unavailable.
    pub fn from_environment(timeout: Duration) -> Result<Self> {
        let endpoint = std::env::var("COHESIX_K8S_API_URL")
            .map_err(|_| anyhow!("not_enabled kubernetes endpoint"))?;
        let credential_ref = std::env::var("COHESIX_K8S_TOKEN_REF")
            .map_err(|_| anyhow!("not_enabled kubernetes credential"))?;
        let bearer = cohesix_authority::secret::resolve_reference(&credential_ref)?;
        let ca_path = std::env::var("COHESIX_K8S_CA_FILE")
            .map_err(|_| anyhow!("not_enabled kubernetes TLS root"))?;
        let mut ca = Vec::new();
        std::fs::File::open(ca_path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut ca)?;
        ensure!(ca.len() as u64 <= MAX_BYTES, "ELIMIT kubernetes TLS root");
        let endpoint = Url::parse(&endpoint)?;
        ensure!(
            endpoint.scheme() == "https"
                && endpoint.host_str().is_some()
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.path() == "/"
                && endpoint.query().is_none()
                && endpoint.fragment().is_none(),
            "EPERM kubernetes endpoint"
        );
        ensure!(
            timeout > Duration::ZERO && timeout <= Duration::from_secs(30),
            "ELIMIT kubernetes deadline"
        );
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .tls_certs_only([reqwest::Certificate::from_pem(&ca)?])
            .build()?;
        Ok(Self {
            client,
            endpoint,
            bearer,
            deadline: Instant::now() + timeout,
        })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        patch: bool,
    ) -> Result<Value> {
        let timeout = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|timeout| !timeout.is_zero())
            .ok_or_else(|| anyhow!("timeout kubernetes operation"))?;
        let url = self.endpoint.join(path)?;
        ensure!(
            url.origin() == self.endpoint.origin(),
            "EPERM kubernetes origin"
        );
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.bearer)
            .header("Accept", "application/json")
            .timeout(timeout);
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body)?;
            ensure!(bytes.len() <= 8192, "ELIMIT kubernetes request");
            request = request
                .header(
                    "Content-Type",
                    if patch {
                        "application/json-patch+json"
                    } else {
                        "application/json"
                    },
                )
                .body(bytes);
        }
        let response = request
            .send()
            .map_err(|_| anyhow!("unavailable kubernetes transport"))?;
        let status = response.status();
        let mut bytes = Vec::new();
        response.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "ELIMIT kubernetes response"
        );
        if status == StatusCode::TOO_MANY_REQUESTS {
            bail!("busy kubernetes disruption-budget");
        }
        if status == StatusCode::CONFLICT {
            bail!("stale kubernetes resourceVersion-or-UID");
        }
        ensure!(
            status.is_success(),
            "unavailable kubernetes api-status={}",
            status.as_u16()
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Bounded native discovery; a partial paginated list never becomes complete inventory.
    pub fn nodes(&self) -> Result<Vec<NodeObservation>> {
        let value = self.request(Method::GET, "/api/v1/nodes?limit=65", None, false)?;
        rows(&value, "NodeList")?
            .into_iter()
            .map(|mut value| {
                let object = value
                    .as_object_mut()
                    .ok_or_else(|| anyhow!("invalid_observation kubernetes node list item"))?;
                object.entry("kind").or_insert_with(|| json!("Node"));
                object.entry("apiVersion").or_insert_with(|| json!("v1"));
                parse_node(&value)
            })
            .collect()
    }

    /// Read exact node identity and current server-owned resource cursor.
    pub fn node(&self, name: &str) -> Result<NodeObservation> {
        let value = self.request(
            Method::GET,
            &format!("/api/v1/nodes/{}", token(name)?),
            None,
            false,
        )?;
        let node = parse_node(&value)?;
        ensure!(
            node.name == name,
            "invalid_observation kubernetes node identity"
        );
        Ok(node)
    }

    /// Conditional cordon; UID/resourceVersion tests precede mutation atomically in the API server.
    pub fn cordon(&self, before: &NodeObservation) -> Result<NodeObservation> {
        if !before.unschedulable {
            self.request(
                Method::PATCH,
                &format!("/api/v1/nodes/{}", token(&before.name)?),
                Some(cordon_patch(before)),
                true,
            )?;
        }
        let after = self.node(&before.name)?;
        ensure!(
            after.uid == before.uid && after.unschedulable,
            "invalid_observation kubernetes cordon postcondition"
        );
        Ok(after)
    }

    /// List evictable pods on one node. DaemonSets/mirror pods remain, and are explicitly excluded.
    pub fn drain_candidates(&self, node: &str) -> Result<Vec<PodIdentity>> {
        let value = self.request(
            Method::GET,
            &format!(
                "/api/v1/pods?limit=65&fieldSelector=spec.nodeName%3D{}",
                token(node)?
            ),
            None,
            false,
        )?;
        let mut pods = Vec::new();
        for value in rows(&value, "PodList")? {
            ensure!(
                value["spec"]["nodeName"] == node,
                "invalid_observation kubernetes pod node"
            );
            let meta = &value["metadata"];
            if matches!(
                value["status"]["phase"].as_str(),
                Some("Succeeded" | "Failed")
            ) || meta["annotations"]["kubernetes.io/config.mirror"].is_string()
                || meta["ownerReferences"].as_array().is_some_and(|owners| {
                    owners
                        .iter()
                        .any(|owner| owner["kind"] == "DaemonSet" && owner["controller"] == true)
                })
            {
                continue;
            }
            pods.push(PodIdentity {
                namespace: field(meta, "namespace")?.into(),
                name: field(meta, "name")?.into(),
                uid: field(meta, "uid")?.into(),
                resource_version: field(meta, "resourceVersion")?.into(),
            });
        }
        Ok(pods)
    }

    /// Respect disruption budgets using policy/v1; no force deletion or arbitrary patch is exposed.
    pub fn evict(&self, pod: &PodIdentity) -> Result<()> {
        self.request(Method::POST, &format!("/api/v1/namespaces/{}/pods/{}/eviction", token(&pod.namespace)?, token(&pod.name)?),
            Some(json!({"apiVersion":"policy/v1","kind":"Eviction","metadata":{"name":pod.name,"namespace":pod.namespace},
                "deleteOptions":{"gracePeriodSeconds":30,"preconditions":{"uid":pod.uid,"resourceVersion":pod.resource_version}}})), false)?;
        Ok(())
    }
}

/// Pure conditional mutation document; mutable names cannot substitute for observed UID binding.
pub fn cordon_patch(before: &NodeObservation) -> Value {
    json!([{"op":"test","path":"/metadata/uid","value":before.uid},
        {"op":"test","path":"/metadata/resourceVersion","value":before.resource_version},
        {"op":"add","path":"/spec/unschedulable","value":true}])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_identity_and_atomic_patch_keep_resource_fences() {
        let mut raw = json!({"apiVersion":"v1","kind":"Node", "metadata":{"name":"edge-1","uid":"uid-123","resourceVersion":"opaque-4"},
            "spec":{},"status":{"conditions":[{"type":"Ready","status":"True"}]}});
        let node = parse_node(&raw).unwrap();
        assert_eq!(node.ready, Some(true));
        assert!(!node.unschedulable);
        assert_eq!(
            cordon_patch(&node),
            json!([
            {"op":"test","path":"/metadata/uid","value":"uid-123"},
            {"op":"test","path":"/metadata/resourceVersion","value":"opaque-4"},
            {"op":"add","path":"/spec/unschedulable","value":true}])
        );
        raw["metadata"]["uid"] = json!("../escape");
        assert!(parse_node(&raw).is_err());
        assert!(rows(
            &json!({"apiVersion":"v1","kind":"NodeList","metadata":{"continue":"next"},"items":[]}),
            "NodeList"
        )
        .is_err());
    }
}
