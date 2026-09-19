// Author: Lukas Bower
// Purpose: Verify external identity before proposing finite Cohesix delegation; only the existing issuer grants tickets.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Host issuer and gateway checks for finite identity-bound delegation.
pub mod delegation;

pub const MAX_TOKEN_BYTES: usize = 8192;
pub const MAX_KEYSET_BYTES: usize = 65536;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("not_enabled identity mapping")]
    Disabled,
    #[error("EPERM identity policy")]
    Policy,
    #[error("ELIMIT identity input")]
    Limit,
    #[error("EPERM identity signature or key")]
    Signature,
    #[error("EPERM identity issuer or audience")]
    Issuer,
    #[error("ESTALE identity lifetime")]
    Stale,
    #[error("EPERM unmapped or ambiguous subject")]
    Subject,
}
type Result<T> = std::result::Result<T, IdentityError>;

/// Trusted compiler input, never taken from the identity token or its URLs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub issuer: String,
    pub audience: String,
    pub algorithm: String,
    pub keyset_sha256: Vec<String>,
    pub maximum_ttl_s: u64,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub subject: String,
    pub required_groups: Vec<String>,
    pub normalized_subject: String,
    pub role: String,
    pub scopes: Vec<Scope>,
    pub provider_actions: Vec<String>,
    pub maximum_operations: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub path: String,
    pub verb: String,
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Policy {
    /// Validate finite exact-match mappings; wildcards and unbounded issuer grants are absent.
    pub fn validate(&self, actions: &BTreeSet<String>) -> Result<()> {
        if cohesix_authority::validate_id(&self.id).is_err()
            || !matches!(
                self.kind.as_str(),
                "oidc" | "spiffe" | "kubernetes" | "local"
            )
            || !text(&self.issuer)
            || !text(&self.audience)
            || self.maximum_ttl_s == 0
            || self.maximum_ttl_s > 3600
            || self.rules.len() > 64
            || self.keyset_sha256.len() > 4
            || self.keyset_sha256.iter().any(|key| !digest(key))
            || (self.enabled && self.rules.is_empty())
        {
            return Err(IdentityError::Policy);
        }
        if self.kind == "local" {
            if self.algorithm != "os-euid" || !self.keyset_sha256.is_empty() {
                return Err(IdentityError::Policy);
            }
        } else if !matches!(self.algorithm.as_str(), "RS256" | "ES256" | "EdDSA")
            || (self.enabled && self.keyset_sha256.is_empty())
            || (self.kind == "spiffe"
                && (!self.issuer.starts_with("spiffe://") || self.issuer[9..].contains('/')))
            || (self.kind != "spiffe" && !self.issuer.starts_with("https://"))
        {
            return Err(IdentityError::Policy);
        }
        let mut subjects = BTreeSet::new();
        let mut normalized_subjects = BTreeSet::new();
        for rule in &self.rules {
            if !text(&rule.subject)
                || !subjects.insert(&rule.subject)
                || !normalized_subjects.insert(&rule.normalized_subject)
                || rule.required_groups.len() > 32
                || rule.required_groups.iter().any(|group| !text(group))
                || cohesix_authority::validate_id(&rule.normalized_subject).is_err()
                || !matches!(
                    rule.role.as_str(),
                    "queen" | "worker-gpu" | "worker-lora" | "worker-bus" | "worker-heartbeat"
                )
                || rule.scopes.is_empty()
                || rule.scopes.len() > 16
                || rule.maximum_operations == 0
                || rule.maximum_operations > 256
                || rule.provider_actions.len() > 32
                || rule
                    .provider_actions
                    .iter()
                    .any(|action| !actions.contains(action))
            {
                return Err(IdentityError::Policy);
            }
            let mut paths = BTreeSet::new();
            for scope in &rule.scopes {
                if scope.path.len() > 255
                    || !scope.path.starts_with('/')
                    || scope.path == "/"
                    || !matches!(scope.verb.as_str(), "read" | "write" | "read_write")
                    || !paths.insert(&scope.path)
                    || scope.path[1..]
                        .split('/')
                        .any(|part| cohesix_authority::validate_id(part).is_err())
                    || (scope.verb != "read" && rule.provider_actions.is_empty())
                    || (scope.verb != "read"
                        && (scope.path != "/host/tickets/spec" || rule.role != "queen"))
                {
                    return Err(IdentityError::Policy);
                }
            }
            if self.kind == "local" && rule.subject.parse::<u32>().is_err() {
                return Err(IdentityError::Policy);
            }
            if self.kind == "kubernetes" {
                let parts: Vec<_> = rule.subject.split(':').collect();
                if parts.len() != 4
                    || parts[..2] != ["system", "serviceaccount"]
                    || parts[2..]
                        .iter()
                        .any(|part| cohesix_authority::validate_id(part).is_err())
                {
                    return Err(IdentityError::Policy);
                }
            }
        }
        Ok(())
    }
}

/// A proposal only: provider action restrictions must be enforced by the issuer/admission policy.
/// Private fields and no Deserialize prevent accidental use of client-authored output as verified identity.
#[derive(Debug, Serialize)]
pub struct MappedRequest {
    schema: &'static str,
    authoritative: bool,
    proof_class: &'static str,
    mode: &'static str,
    mapping_id: String,
    mapping_sha256: String,
    provider_graph_sha256: String,
    source_identity_sha256: String,
    credential_sha256: String,
    subject: String,
    role: String,
    scopes: Vec<Scope>,
    provider_actions: Vec<String>,
    maximum_operations: u64,
    issued_unix_s: u64,
    expires_unix_s: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    typ: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: Option<String>,
    sub: String,
    aud: Audience,
    iat: u64,
    exp: u64,
    nbf: Option<u64>,
    #[serde(default)]
    groups: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Keyset {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
struct Key {
    kty: String,
    kid: String,
    alg: String,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
    d: Option<String>,
}

fn decode(value: &str, maximum: usize) -> Result<Vec<u8>> {
    if value.len() > maximum.saturating_mul(2) {
        return Err(IdentityError::Limit);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| IdentityError::Signature)?;
    if bytes.len() > maximum {
        return Err(IdentityError::Limit);
    }
    Ok(bytes)
}

fn key_bytes(value: &Option<String>, maximum: usize) -> Result<Vec<u8>> {
    decode(value.as_deref().ok_or(IdentityError::Signature)?, maximum)
}

fn verify_signature(
    header: &Header,
    keys: &[u8],
    message: &[u8],
    signature_bytes: &[u8],
) -> Result<()> {
    let keyset: Keyset = serde_json::from_slice(keys).map_err(|_| IdentityError::Signature)?;
    if keyset.keys.is_empty() || keyset.keys.len() > 32 {
        return Err(IdentityError::Limit);
    }
    let mut ids = BTreeSet::new();
    if keyset
        .keys
        .iter()
        .any(|key| !text(&key.kid) || !ids.insert(&key.kid) || key.d.is_some())
    {
        return Err(IdentityError::Signature);
    }
    let key = keyset
        .keys
        .iter()
        .find(|key| key.kid == header.kid)
        .ok_or(IdentityError::Signature)?;
    if key.alg != header.alg
        || key.usage.as_deref().is_some_and(|usage| usage != "sig")
        || key.key_ops.as_ref().is_some_and(|ops| ops != &["verify"])
    {
        return Err(IdentityError::Signature);
    }
    let valid = match (header.alg.as_str(), key.kty.as_str(), key.crv.as_deref()) {
        ("EdDSA", "OKP", Some("Ed25519")) => {
            signature::UnparsedPublicKey::new(&signature::ED25519, key_bytes(&key.x, 32)?)
                .verify(message, signature_bytes)
        }
        ("ES256", "EC", Some("P-256")) => {
            let x = key_bytes(&key.x, 32)?;
            let y = key_bytes(&key.y, 32)?;
            if x.len() != 32 || y.len() != 32 {
                return Err(IdentityError::Signature);
            }
            let mut public = vec![4];
            public.extend(x);
            public.extend(y);
            signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public)
                .verify(message, signature_bytes)
        }
        ("RS256", "RSA", None) => {
            let n = key_bytes(&key.n, 512)?;
            let e = key_bytes(&key.e, 8)?;
            signature::RsaPublicKeyComponents { n: &n, e: &e }.verify(
                &signature::RSA_PKCS1_2048_8192_SHA256,
                message,
                signature_bytes,
            )
        }
        _ => return Err(IdentityError::Signature),
    };
    valid.map_err(|_| IdentityError::Signature)
}

fn request(
    policy: &Policy,
    subject: &str,
    groups: &[String],
    credential: &[u8],
    now: u64,
    expiry: u64,
    graph: &str,
) -> Result<MappedRequest> {
    if !digest(graph) {
        return Err(IdentityError::Policy);
    }
    let matches: Vec<_> = policy
        .rules
        .iter()
        .filter(|rule| {
            rule.subject == subject
                && rule
                    .required_groups
                    .iter()
                    .all(|group| groups.contains(group))
        })
        .collect();
    if matches.len() != 1 {
        return Err(IdentityError::Subject);
    }
    let rule = matches[0];
    let mapping = serde_json::to_vec(policy).map_err(|_| IdentityError::Policy)?;
    Ok(MappedRequest {
        schema: "cohesix-identity-request/v1",
        authoritative: false,
        proof_class: "verified_subject_request",
        mode: "live",
        mapping_id: policy.id.clone(),
        mapping_sha256: hex::encode(Sha256::digest(mapping)),
        provider_graph_sha256: graph.into(),
        source_identity_sha256: hex::encode(Sha256::digest(format!(
            "{}\0{}",
            policy.issuer, subject
        ))),
        credential_sha256: hex::encode(Sha256::digest(credential)),
        subject: rule.normalized_subject.clone(),
        role: rule.role.clone(),
        scopes: rule.scopes.clone(),
        provider_actions: rule.provider_actions.clone(),
        maximum_operations: rule.maximum_operations,
        issued_unix_s: now,
        expires_unix_s: expiry,
    })
}

/// Verify a JWT using locally pinned public keys, fixed algorithms and exact generated subject mappings.
pub fn map_jwt(
    policy: &Policy,
    token: &[u8],
    keys: &[u8],
    now: u64,
    graph: &str,
    actions: &BTreeSet<String>,
) -> Result<MappedRequest> {
    policy.validate(actions)?;
    if !policy.enabled {
        return Err(IdentityError::Disabled);
    }
    if policy.kind == "local" {
        return Err(IdentityError::Policy);
    }
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES || keys.len() > MAX_KEYSET_BYTES {
        return Err(IdentityError::Limit);
    }
    if !policy
        .keyset_sha256
        .contains(&hex::encode(Sha256::digest(keys)))
    {
        return Err(IdentityError::Signature);
    }
    let raw = std::str::from_utf8(token).map_err(|_| IdentityError::Signature)?;
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() != 3 {
        return Err(IdentityError::Signature);
    }
    let header: Header =
        serde_json::from_slice(&decode(parts[0], 1024)?).map_err(|_| IdentityError::Signature)?;
    if header.alg != policy.algorithm
        || !text(&header.kid)
        || header.typ.as_deref().is_some_and(|typ| typ != "JWT")
    {
        return Err(IdentityError::Signature);
    }
    let signature_bytes = decode(parts[2], 512)?;
    let message_length = parts[0].len() + 1 + parts[1].len();
    verify_signature(&header, keys, &token[..message_length], &signature_bytes)?;
    let claims: Claims = serde_json::from_slice(&decode(parts[1], MAX_TOKEN_BYTES)?)
        .map_err(|_| IdentityError::Subject)?;
    let audiences = match claims.aud {
        Audience::One(value) => vec![value],
        Audience::Many(values) => values,
    };
    if audiences.len() != 1 || audiences[0] != policy.audience {
        return Err(IdentityError::Issuer);
    }
    if policy.kind == "spiffe" {
        let prefix = format!("{}/", policy.issuer);
        if !claims.sub.starts_with(&prefix)
            || claims.sub[prefix.len()..]
                .split('/')
                .any(|part| cohesix_authority::validate_id(part).is_err())
            || claims.iss.is_some()
        {
            return Err(IdentityError::Issuer);
        }
    } else if claims.iss.as_deref() != Some(&policy.issuer) {
        return Err(IdentityError::Issuer);
    }
    if now == 0
        || claims.iat > now
        || claims.exp <= now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > policy.maximum_ttl_s
        || claims.nbf.is_some_and(|nbf| nbf > now || nbf > claims.exp)
    {
        return Err(IdentityError::Stale);
    }
    if !text(&claims.sub)
        || claims.groups.len() > 32
        || claims.groups.iter().any(|group| !text(group))
    {
        return Err(IdentityError::Limit);
    }
    request(
        policy,
        &claims.sub,
        &claims.groups,
        token,
        claims.iat,
        claims.exp,
        graph,
    )
}

/// Local identity is supplied by the kernel for this process, never an argv/env username or uid.
#[cfg(unix)]
pub fn map_local(
    policy: &Policy,
    now: u64,
    graph: &str,
    actions: &BTreeSet<String>,
) -> Result<MappedRequest> {
    policy.validate(actions)?;
    if !policy.enabled {
        return Err(IdentityError::Disabled);
    }
    if policy.kind != "local" || now == 0 {
        return Err(IdentityError::Policy);
    }
    let uid = rustix::process::geteuid().as_raw().to_string();
    let expiry = now
        .checked_add(policy.maximum_ttl_s)
        .ok_or(IdentityError::Stale)?;
    request(policy, &uid, &[], uid.as_bytes(), now, expiry, graph)
}

/// Select generated policy without letting the caller provide a permissive replacement.
pub fn generated_policy(id: &str) -> Result<(Policy, BTreeSet<String>, String)> {
    let registry = cohesix_authority::provider::registry().map_err(|_| IdentityError::Policy)?;
    let rows = registry["contract"]["identity_mappings"]
        .as_array()
        .ok_or(IdentityError::Policy)?;
    let selected = rows
        .iter()
        .find(|row| row["id"] == id)
        .ok_or(IdentityError::Disabled)?;
    let policy: Policy =
        serde_json::from_value(selected.clone()).map_err(|_| IdentityError::Policy)?;
    let actions = registry["contract"]["families"]
        .as_array()
        .ok_or(IdentityError::Policy)?
        .iter()
        .flat_map(|family| family["actions"].as_array().into_iter().flatten())
        .filter_map(|action| action["id"].as_str().map(str::to_owned))
        .collect();
    let graph = registry["graph_sha256"]
        .as_str()
        .filter(|value| digest(value))
        .ok_or(IdentityError::Policy)?
        .to_owned();
    Ok((policy, actions, graph))
}
