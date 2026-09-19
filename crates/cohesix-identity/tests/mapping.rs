// Author: Lukas Bower
// Purpose: Preserve independent identity signature, freshness and delegated-scope refusal contracts.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use cohesix_identity::{map_jwt, map_local, IdentityError, Policy, Rule, Scope};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

// Test-only key. Packages must exclude this test module and all fixture signers.
fn fixture(kind: &str) -> (Policy, SigningKey, Vec<u8>, BTreeSet<String>, Value) {
    let key = SigningKey::from_bytes(&[39; 32]);
    let jwks = serde_json::to_vec(&json!({"keys":[{"kid":"fixture", "alg":"EdDSA",
        "kty":"OKP", "crv":"Ed25519", "x":B64.encode(key.verifying_key().as_bytes())}]}))
    .unwrap();
    let (issuer, subject) = match kind {
        "spiffe" => (
            "spiffe://example.test",
            "spiffe://example.test/ns/edge/sa/agent",
        ),
        "kubernetes" => (
            "https://cluster.example.test",
            "system:serviceaccount:edge:agent",
        ),
        _ => ("https://identity.example.test", "operator-1"),
    };
    let mut claims = json!({"iss":issuer, "sub":subject, "aud":"cohesix", "iat":1000,
        "exp":1200, "nbf":1000, "groups":["operators"]});
    if kind == "spiffe" {
        claims.as_object_mut().unwrap().remove("iss");
    }
    let policy = Policy {
        id: "mapping".into(),
        kind: kind.into(),
        enabled: true,
        issuer: issuer.into(),
        audience: "cohesix".into(),
        algorithm: "EdDSA".into(),
        keyset_sha256: vec![hex::encode(Sha256::digest(&jwks))],
        maximum_ttl_s: 300,
        rules: vec![Rule {
            subject: subject.into(),
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
    (
        policy,
        key,
        jwks,
        BTreeSet::from(["systemd.restart".into()]),
        claims,
    )
}

fn token(key: &SigningKey, claims: &Value) -> Vec<u8> {
    let message = format!(
        "{}.{}",
        B64.encode(br#"{"alg":"EdDSA","kid":"fixture","typ":"JWT"}"#),
        B64.encode(serde_json::to_vec(claims).unwrap())
    );
    format!(
        "{message}.{}",
        B64.encode(key.sign(message.as_bytes()).to_bytes())
    )
    .into_bytes()
}

#[test]
fn external_subjects_require_signature_exact_mapping_and_finite_lifetime() {
    for kind in ["oidc", "spiffe", "kubernetes"] {
        let (mut policy, key, jwks, actions, claims) = fixture(kind);
        let graph = "a".repeat(64);
        let good = token(&key, &claims);
        let verified = map_jwt(&policy, &good, &jwks, 1100, &graph, &actions).unwrap();
        let request = serde_json::to_value(&verified).unwrap();
        assert_eq!(request["authoritative"], false);
        assert_eq!(request["subject"], "operator-1");
        assert_eq!(request["maximum_operations"], 1);
        assert_eq!(request["provider_actions"], json!(["systemd.restart"]));
        assert_eq!(request["expires_unix_s"], 1200);
        assert!(!serde_json::to_string(&request)
            .unwrap()
            .contains(std::str::from_utf8(&good).unwrap()));
        for (field, value, expected) in [
            ("aud", json!("wrong"), IdentityError::Issuer),
            ("exp", json!(1100), IdentityError::Stale),
            ("exp", json!(5000), IdentityError::Stale),
            ("nbf", json!(1101), IdentityError::Stale),
            ("groups", json!(["unmapped"]), IdentityError::Subject),
        ] {
            let mut bad = claims.clone();
            bad[field] = value;
            assert_eq!(
                map_jwt(&policy, &token(&key, &bad), &jwks, 1100, &graph, &actions).unwrap_err(),
                expected
            );
        }
        let other_key = SigningKey::from_bytes(&[40; 32]);
        assert_eq!(
            map_jwt(
                &policy,
                &token(&other_key, &claims),
                &jwks,
                1100,
                &graph,
                &actions
            )
            .unwrap_err(),
            IdentityError::Signature
        );
        policy.rules[0].scopes[0].path = "/".into();
        assert_eq!(
            map_jwt(&policy, &good, &jwks, 1100, &graph, &actions).unwrap_err(),
            IdentityError::Policy
        );
        policy.rules[0].scopes[0].path = "/host/tickets/spec".into();
        policy.enabled = false;
        assert_eq!(
            map_jwt(&policy, &good, &jwks, 1100, &graph, &actions).unwrap_err(),
            IdentityError::Disabled
        );
    }
}

#[test]
fn mapped_exchange_preserves_quota_identity_and_current_policy_scope() {
    use cohesix_ticket::{BudgetSpec, TicketIssuer, TicketToken};
    let (mut policy, key, jwks, actions, claims) = fixture("oidc");
    let graph = "a".repeat(64);
    let credential = token(&key, &claims);
    let first = map_jwt(&policy, &credential, &jwks, 1100, &graph, &actions).unwrap();
    let second = map_jwt(&policy, &credential, &jwks, 1150, &graph, &actions).unwrap();
    let issuer = TicketIssuer::new("identity-delegation-test-issuer");
    let issued = first.issue(&issuer).unwrap();
    assert_eq!(issued, second.issue(&issuer).unwrap());
    let decoded = TicketToken::decode(&issued, &issuer.key()).unwrap();
    assert_eq!(decoded.claims().issued_at_ms, 1_000_000);
    assert_eq!(decoded.claims().budget.ttl_s(), Some(200));
    assert_eq!(decoded.claims().budget.ops(), Some(1));
    assert_eq!(
        policy
            .mapped_rule(decoded.claims(), &graph)
            .unwrap()
            .unwrap()
            .provider_actions,
        ["systemd.restart"]
    );
    let mut expanded = decoded.claims().clone();
    expanded.budget = BudgetSpec::unbounded()
        .with_ops(Some(2))
        .with_ttl(Some(200));
    assert_eq!(
        policy.mapped_rule(&expanded, &graph).unwrap_err(),
        IdentityError::Policy
    );
    expanded = decoded.claims().clone();
    expanded.scopes[0].path = "/host".to_owned();
    assert_eq!(
        policy.mapped_rule(&expanded, &graph).unwrap_err(),
        IdentityError::Policy
    );
    policy.rules[0].provider_actions.clear();
    assert!(policy
        .mapped_rule(decoded.claims(), &graph)
        .unwrap()
        .is_none());
}

#[test]
fn local_identity_comes_from_the_kernel_and_unmapped_uid_is_refused() {
    let (mut policy, _, _, actions, _) = fixture("oidc");
    policy.kind = "local".into();
    policy.issuer = "local:operator-host".into();
    policy.algorithm = "os-euid".into();
    policy.keyset_sha256.clear();
    policy.rules[0].subject = rustix::process::geteuid().as_raw().to_string();
    policy.rules[0].required_groups.clear();
    let request = map_local(&policy, 1100, &"a".repeat(64), &actions).unwrap();
    assert_eq!(
        serde_json::to_value(request).unwrap()["expires_unix_s"],
        1400
    );
    policy.rules[0].subject = rustix::process::geteuid()
        .as_raw()
        .wrapping_add(1)
        .to_string();
    assert_eq!(
        map_local(&policy, 1100, &"a".repeat(64), &actions).unwrap_err(),
        IdentityError::Subject
    );
}
