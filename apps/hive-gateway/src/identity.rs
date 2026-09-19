// Author: Lukas Bower
// Purpose: Exchange pinned external identities and enforce their exact generated provider permissions before delegation.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use cohesix_identity::{IdentityError, Policy, Rule, MAX_KEYSET_BYTES, MAX_TOKEN_BYTES};
use cohesix_ticket::{TicketClaims, TicketIssuer, TicketToken};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

pub const MAX_EXCHANGE_BODY: usize = MAX_TOKEN_BYTES + 1024;

/// No client-selected key, URL, subject, TTL, role or action overrides.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRequest {
    pub mapping_id: String,
    pub credential: String,
}

#[derive(Serialize)]
pub struct Issued {
    pub schema: &'static str,
    pub status: &'static str,
    pub authoritative: bool,
    pub identity_class: &'static str,
    pub mapping_id: String,
    pub provider_graph_sha256: String,
    pub credential_sha256: String,
    pub expires_unix_s: u64,
    pub ticket: String,
}

/// Immutable enrollment is read once at startup. Rotation requires explicit
/// restart with the current generated policy and pinned public keyset bytes.
pub struct Identity {
    policies: Vec<Policy>,
    actions: BTreeSet<String>,
    graph: String,
    issuer: Option<TicketIssuer>,
    keysets: BTreeMap<String, Vec<u8>>,
    pub permits: Arc<Semaphore>,
}

fn read_public_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err(anyhow!(
            "EPERM identity enrollment requires an absolute path"
        ));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    let file = std::fs::File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(anyhow!("ELIMIT identity enrollment file"));
    }
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(anyhow!("ELIMIT identity enrollment file"));
    }
    Ok(bytes)
}

impl Identity {
    pub fn configured(issuer: Option<TicketIssuer>, keysets: Option<&Path>) -> Result<Self> {
        let registry = cohesix_authority::provider::registry()?;
        let rows = registry["contract"]["identity_mappings"]
            .as_array()
            .ok_or_else(|| anyhow!("EPERM generated identity policies"))?;
        let mut policies = Vec::new();
        let mut actions = BTreeSet::new();
        let graph = registry["graph_sha256"]
            .as_str()
            .ok_or_else(|| anyhow!("EPERM generated identity graph"))?
            .to_owned();
        for row in rows {
            let id = row["id"]
                .as_str()
                .ok_or_else(|| anyhow!("EPERM identity id"))?;
            let (policy, registered, selected_graph) = cohesix_identity::generated_policy(id)?;
            if selected_graph != graph {
                return Err(anyhow!("EPERM generated identity graph"));
            }
            policy.validate(&registered)?;
            policies.push(policy);
            actions.extend(registered);
        }
        let mut value = Self {
            policies,
            actions,
            graph,
            issuer,
            keysets: BTreeMap::new(),
            permits: Arc::new(Semaphore::new(4)),
        };
        if let Some(path) = keysets {
            // Exact policy-id -> public JWKS file. Repeated keys are refused by
            // the custom visitor; ordinary map deserialization would keep last.
            let paths: KeysetPaths = serde_json::from_slice(&read_public_file(path, 65536)?)?;
            if value.issuer.is_none() || paths.0.len() > value.policies.len() {
                return Err(anyhow!("EPERM identity issuer or enrollment"));
            }
            for (id, path) in paths.0 {
                let policy = value
                    .policies
                    .iter()
                    .find(|policy| policy.id == id)
                    .ok_or_else(|| anyhow!("EPERM identity enrollment mapping"))?;
                if !policy.enabled || policy.kind == "local" {
                    return Err(anyhow!("EPERM identity enrollment mapping"));
                }
                let bytes = read_public_file(&path, MAX_KEYSET_BYTES)?;
                if !policy
                    .keyset_sha256
                    .contains(&hex::encode(Sha256::digest(&bytes)))
                {
                    return Err(anyhow!("EPERM identity enrollment key digest"));
                }
                value.keysets.insert(id, bytes);
            }
        }
        Ok(value)
    }

    pub fn exchange(&self, request: &ExchangeRequest, now: u64) -> Result<Issued, IdentityError> {
        if request.credential.len() > MAX_TOKEN_BYTES
            || cohesix_authority::validate_id(&request.mapping_id).is_err()
        {
            return Err(IdentityError::Limit);
        }
        let policy = self
            .policies
            .iter()
            .find(|policy| policy.id == request.mapping_id)
            .ok_or(IdentityError::Subject)?;
        if !policy.enabled {
            return Err(IdentityError::Disabled);
        }
        // A remote request cannot attest the kernel uid of its caller. The
        // local CLI maps its own euid and uses the separately enrolled issuer.
        if policy.kind == "local" {
            return Err(IdentityError::Policy);
        }
        let keys = self
            .keysets
            .get(&policy.id)
            .ok_or(IdentityError::Signature)?;
        let issuer = self.issuer.as_ref().ok_or(IdentityError::Policy)?;
        let mapped = cohesix_identity::map_jwt(
            policy,
            request.credential.as_bytes(),
            keys,
            now,
            &self.graph,
            &self.actions,
        )?;
        Ok(Issued {
            schema: "cohesix-identity-exchange/v1",
            status: "OK",
            authoritative: false,
            identity_class: "gateway_enforced",
            mapping_id: policy.id.clone(),
            provider_graph_sha256: self.graph.clone(),
            credential_sha256: mapped.credential_sha256().to_owned(),
            expires_unix_s: mapped.expires_unix_s(),
            ticket: mapped.issue(issuer)?,
        })
    }

    /// The caller routes the reserved subject prefix here and charges the
    /// original signed token identity. Return normalized claims only for role
    /// ownership checks; the policy binding remains in the signed token.
    pub fn verify(
        &self,
        token: &str,
        path: &str,
        lines: Option<&[&str]>,
        read: bool,
    ) -> Result<(TicketToken, TicketClaims), &'static str> {
        let key = self
            .issuer
            .as_ref()
            .ok_or("EPERM identity issuer unavailable")?
            .key();
        let verified =
            TicketToken::decode(token, &key).map_err(|_| "EPERM mapped ticket signature")?;
        let mut matched: Option<&Rule> = None;
        for policy in &self.policies {
            if let Some(rule) = policy
                .mapped_rule(verified.claims(), &self.graph)
                .map_err(|_| "EPERM mapped ticket policy")?
            {
                if matched.replace(rule).is_some() {
                    return Err("EPERM ambiguous mapped ticket policy");
                }
            }
        }
        let rule = matched.ok_or("EPERM mapped ticket policy unavailable")?;
        if !read {
            if path != "/host/tickets/spec" {
                return Err("EPERM mapped provider path");
            }
            let lines = lines
                .filter(|lines| !lines.is_empty())
                .ok_or("EPERM mapped provider request")?;
            for line in lines {
                // The root remains the full host-ticket schema validator.
                // Deserialize action exactly once, reject duplicate keys and
                // trailing JSON, then enforce the generated finite action set.
                #[derive(Deserialize)]
                struct Action {
                    action: String,
                }
                if line.len() > cohsh_core::MAX_LINE_LEN {
                    return Err("ELIMIT mapped provider request");
                }
                let action: Action =
                    serde_json::from_str(line).map_err(|_| "EPERM mapped provider request")?;
                if !rule.provider_actions.contains(&action.action) {
                    return Err("EPERM mapped provider action");
                }
                cohesix_authority::provider::validate_request_size(&action.action, line.len())
                    .map_err(|_| "ELIMIT mapped provider request")?;
            }
        }
        let mut claims = verified.claims().clone();
        claims.subject = Some(rule.normalized_subject.clone());
        Ok((verified, claims))
    }
}

struct KeysetPaths(BTreeMap<String, PathBuf>);
impl<'de> Deserialize<'de> for KeysetPaths {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = KeysetPaths;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unique generated mapping ids and absolute public JWKS paths")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut paths = BTreeMap::new();
                while let Some((id, path)) = map.next_entry::<String, PathBuf>()? {
                    if paths.len() >= 64 || paths.insert(id, path).is_some() {
                        return Err(serde::de::Error::custom(
                            "identity enrollment duplicate or limit",
                        ));
                    }
                }
                Ok(KeysetPaths(paths))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    use cohesix_authority::policy::AuthorityPolicy;
    use cohesix_identity::{delegation::gateway_issuer, Scope};
    use cohesix_ticket::{Role, TicketKey};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn fixture() -> (Identity, ExchangeRequest) {
        let signing = SigningKey::from_bytes(&[63; 32]);
        let keys = serde_json::to_vec(&json!({"keys":[{
            "kid":"fixture", "alg":"EdDSA", "kty":"OKP", "crv":"Ed25519",
            "x":B64.encode(signing.verifying_key().as_bytes())
        }]}))
        .unwrap();
        let policy = Policy {
            id: "mapped-operator".into(),
            kind: "oidc".into(),
            enabled: true,
            issuer: "https://identity.example.test".into(),
            audience: "cohesix".into(),
            algorithm: "EdDSA".into(),
            keyset_sha256: vec![hex::encode(Sha256::digest(&keys))],
            maximum_ttl_s: 300,
            rules: vec![Rule {
                subject: "operator-1".into(),
                required_groups: vec!["operators".into()],
                normalized_subject: "operator-1".into(),
                role: "queen".into(),
                scopes: vec![Scope {
                    path: "/host/tickets/spec".into(),
                    verb: "write".into(),
                }],
                provider_actions: vec!["systemd.restart".into()],
                maximum_operations: 1,
            }],
        };
        let message = format!(
            "{}.{}",
            B64.encode(br#"{"alg":"EdDSA","kid":"fixture","typ":"JWT"}"#),
            B64.encode(
                serde_json::to_vec(&json!({
                    "iss":policy.issuer, "aud":policy.audience, "sub":"operator-1",
                    "groups":["operators"], "iat":1_900_000_000_u64, "exp":1_900_000_200_u64
                }))
                .unwrap()
            )
        );
        let credential = format!(
            "{message}.{}",
            B64.encode(signing.sign(message.as_bytes()).to_bytes())
        );
        let request = ExchangeRequest {
            mapping_id: policy.id.clone(),
            credential,
        };
        (
            Identity {
                keysets: BTreeMap::from([(policy.id.clone(), keys)]),
                policies: vec![policy],
                actions: BTreeSet::from(["systemd.restart".into()]),
                graph: "a".repeat(64),
                issuer: Some(gateway_issuer("test-enrolled-delegation-secret")),
                permits: Arc::new(Semaphore::new(4)),
            },
            request,
        )
    }

    #[test]
    fn identity_exchange_binds_current_policy_action_domain_and_quota_before_mutation() {
        let (identity, request) = fixture();
        let issued = identity.exchange(&request, 1_900_000_100).unwrap();
        assert!(issued.ticket.len() <= 224);
        assert_eq!(
            issued.ticket,
            identity.exchange(&request, 1_900_000_150).unwrap().ticket
        );
        // The underlying secret cannot authenticate a mapped credential to a
        // VM or the ordinary delegated verifier: their signing domain differs.
        assert!(TicketToken::decode(
            &issued.ticket,
            &TicketKey::from_secret("test-enrolled-delegation-secret")
        )
        .is_err());
        let identity = Arc::new(identity);
        let mut gateway = crate::auth::Delegation::new(
            Some(TicketKey::from_secret("test-enrolled-delegation-secret")),
            AuthorityPolicy::default(),
            Role::Queen,
            None,
            1_900_000_100_000,
        )
        .unwrap()
        .with_identity(identity.clone());
        for lines in [
            vec![r#"{"action":"docker.stop"}"#],
            vec![r#"{"action":"systemd.restart","action":"docker.stop"}"#],
            vec![r#"{"action":"systemd.restart"} {"action":"docker.stop"}"#],
            vec![
                r#"{"action":"systemd.restart"}"#,
                r#"{"action":"docker.stop"}"#,
            ],
        ] {
            assert!(gateway
                .authorize_write(
                    Some(&issued.ticket),
                    "/host/tickets/spec",
                    &lines,
                    1_900_000_100_000
                )
                .is_err());
            assert_eq!(gateway.snapshot().cache_entries, 0);
        }
        assert!(gateway
            .authorize_write(
                Some(&issued.ticket),
                "/queen/ctl",
                &[r#"{"action":"systemd.restart"}"#],
                1_900_000_100_000
            )
            .is_err());
        assert!(gateway
            .authorize_write(
                Some(&issued.ticket),
                "/host/tickets/spec",
                &[r#"{"action":"systemd.restart"}"#],
                1_900_000_100_000
            )
            .is_ok());
        assert_eq!(
            gateway
                .authorize_write(
                    Some(&issued.ticket),
                    "/host/tickets/spec",
                    &[r#"{"action":"systemd.restart"}"#],
                    1_900_000_150_000
                )
                .unwrap_err(),
            "ELIMIT delegated-ticket-quota"
        );
        assert!(gateway
            .authorize_read(
                Some(&issued.ticket),
                "/host/tickets/spec",
                1,
                1_900_000_200_000
            )
            .is_err());
        let (mut changed, _) = fixture();
        changed.policies[0].rules[0].provider_actions = vec!["docker.stop".into()];
        assert_eq!(
            changed
                .verify(&issued.ticket, "/host/tickets/spec", None, true)
                .unwrap_err(),
            "EPERM mapped ticket policy unavailable"
        );
        changed.graph = "b".repeat(64);
        assert!(changed
            .verify(&issued.ticket, "/host/tickets/spec", None, true)
            .is_err());
    }

    #[test]
    fn identity_exchange_refuses_untrusted_claims_and_pinned_enrollment_drift() {
        let (mut identity, mut request) = fixture();
        assert_eq!(
            identity.exchange(&request, 1_900_000_200).err(),
            Some(IdentityError::Stale)
        );
        request.mapping_id = "unmapped".into();
        assert_eq!(
            identity.exchange(&request, 1_900_000_100).err(),
            Some(IdentityError::Subject)
        );
        request.mapping_id = "mapped-operator".into();
        identity.policies[0].audience = "another-service".into();
        assert_eq!(
            identity.exchange(&request, 1_900_000_100).err(),
            Some(IdentityError::Issuer)
        );
        identity.policies[0].audience = "cohesix".into();
        identity
            .keysets
            .get_mut(&request.mapping_id)
            .unwrap()
            .push(b' ');
        assert_eq!(
            identity.exchange(&request, 1_900_000_100).err(),
            Some(IdentityError::Signature)
        );
        identity.policies[0].kind = "local".into();
        assert_eq!(
            identity.exchange(&request, 1_900_000_100).err(),
            Some(IdentityError::Policy)
        );
        assert!(serde_json::from_str::<ExchangeRequest>(
            r#"{"mapping_id":"a","mapping_id":"b","credential":"x"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ExchangeRequest>(
            r#"{"mapping_id":"a","credential":"x","role":"queen"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<KeysetPaths>(r#"{"a":"/x","a":"/y"}"#).is_err());
        assert!(Identity::configured(None, None).is_ok());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keys.json");
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(read_public_file(&path, 2).unwrap(), b"{}");
        assert!(read_public_file(&path, 1).is_err());
        let link = directory.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_public_file(&link, 2).is_err());
    }
}
