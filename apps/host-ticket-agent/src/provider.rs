// Author: Lukas Bower
// Purpose: Validate provider operands and reject ambiguous target/argument selections before dispatch.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::HostTicketSpec;
use anyhow::{anyhow, bail, Result};
use serde_json::Map;

/// Validated single argv/path operand. Never contains separators or options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderToken(String);

impl ProviderToken {
    /// Reject path separators, option prefixes, empty and oversized operands.
    pub fn parse(value: &str) -> Result<Self> {
        cohesix_authority::validate_id(value)
            .map_err(|_| anyhow!("EPERM invalid-provider-token"))?;
        Ok(Self(value.to_owned()))
    }
}

/// Validate the exact supported fields, independently of executor defaults.
pub fn validate(spec: &HostTicketSpec) -> Result<()> {
    let encoded = serde_json::to_vec(spec)?;
    cohesix_authority::provider::validate_request_size(&spec.action, encoded.len())?;
    ProviderToken::parse(&spec.id)?;
    ProviderToken::parse(&spec.idempotency_key)?;
    let components: Vec<_> = match &spec.target {
        Some(target) => {
            if target.len() > 255 {
                bail!("ELIMIT provider-target");
            }
            let value = target.strip_prefix('/').unwrap_or(target);
            let components: Vec<_> = value.split('/').collect();
            for part in &components {
                ProviderToken::parse(part)?;
            }
            components
        }
        None => Vec::new(),
    };
    if spec.schema == crate::HOST_TICKET_V2_SCHEMA {
        return crate::claim::validate_v2_action_args(spec);
    }
    let empty = Map::new();
    let args = if spec.args.is_null() {
        &empty
    } else {
        spec.args
            .as_object()
            .ok_or_else(|| anyhow!("EPERM provider-args-object-required"))?
    };
    let allowed = cohesix_authority::PROVIDER_V1_FIELDS
        .iter()
        .find(|(action, _)| *action == spec.action)
        .map(|(_, fields)| *fields)
        .ok_or_else(|| anyhow!("EPERM unsupported-provider-action"))?;
    for (name, value) in args {
        if !allowed.contains(&name.as_str()) {
            bail!("EPERM unsupported-provider-field {name}");
        }
        match name.as_str() {
            "ttl_s" | "mem_mb" | "budget_ttl_s" | "budget_ops" => {
                if !value
                    .as_u64()
                    .is_some_and(|v| v > 0 && v <= u32::MAX as u64)
                {
                    bail!("EPERM invalid-provider-bound {name}");
                }
            }
            "priority" | "streams" => {
                if !value
                    .as_u64()
                    .is_some_and(|v| v <= 255 && (name == "priority" || v > 0))
                {
                    bail!("EPERM invalid-provider-bound {name}");
                }
            }
            "publish" => {
                if !value.is_boolean() {
                    bail!("EPERM invalid-provider-boolean");
                }
            }
            "out_dir" | "out" | "adapter_dir" | "from" | "export_root" | "export"
            | "registry_root" | "registry" => {
                let path = value
                    .as_str()
                    .ok_or_else(|| anyhow!("EPERM invalid-provider-path"))?;
                if path.len() > 1024 {
                    bail!("ELIMIT provider-path");
                }
                for part in path.strip_prefix('/').unwrap_or(path).split('/') {
                    ProviderToken::parse(part)?;
                }
            }
            _ => {
                ProviderToken::parse(
                    value
                        .as_str()
                        .ok_or_else(|| anyhow!("EPERM invalid-provider-string"))?,
                )?;
            }
        }
    }
    for (first, alias) in [
        ("job_id", "job"),
        ("model_id", "model"),
        ("out_dir", "out"),
        ("adapter_dir", "from"),
        ("export_root", "export"),
        ("registry_root", "registry"),
    ] {
        if args.contains_key(first) && args.contains_key(alias) {
            bail!("EPERM ambiguous-provider-alias {first}");
        }
    }
    if spec.action.starts_with("mac_release.") || spec.action.starts_with("endpoint_compliance.") {
        let id = args
            .get("target_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("EPERM macos-target"))?;
        if args.len() != 1 || spec.target.as_deref() != Some(id) {
            bail!("EPERM ambiguous-macos-target");
        }
        if !cohesix_authority::mac_release::targets()?
            .iter()
            .any(|t| t.id == id && t.operation.action() == spec.action)
        {
            bail!("not_enabled macos-target");
        }
        return Ok(());
    }
    if spec.action.starts_with("launchd.") {
        let id = args
            .get("service")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("EPERM missing-launchd-service"))?;
        if args.len() != 1 || spec.target.as_deref() != Some(id) {
            bail!("EPERM ambiguous-launchd-target");
        }
        if !cohesix_authority::macos::launchd_targets()?
            .iter()
            .any(|t| t.id == id)
        {
            bail!("not_enabled launchd-target");
        }
        return Ok(());
    }
    if spec.action.starts_with("modbus.") || spec.action.starts_with("dnp3.") {
        let endpoint_id = args
            .get("endpoint")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("EPERM missing-field-bus-endpoint"))?;
        let point_id = args
            .get("point")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("EPERM missing-field-bus-point"))?;
        if spec.target.as_deref() != Some(endpoint_id) || args.len() != 2 {
            bail!("EPERM ambiguous-field-bus-target");
        }
        let (endpoint, point) = cohesix_authority::bus::resolve(endpoint_id, point_id)?;
        let family = match endpoint.protocol {
            cohesix_authority::bus::Protocol::Modbus => "modbus",
            cohesix_authority::bus::Protocol::Dnp3 => "dnp3",
        };
        let verb = if point.operation.is_control() {
            "control"
        } else {
            "read"
        };
        if spec.action != format!("{family}.{verb}") {
            bail!("EPERM field-bus-action-map-mismatch");
        }
        return Ok(());
    }
    let path = if components.first() == Some(&"host") {
        &components[1..]
    } else {
        &components[..]
    };
    let (provider, field, index) = if spec.action.starts_with("systemd.") {
        ("systemd", "unit", 1)
    } else if spec.action.starts_with("docker.") {
        ("docker", "container", 1)
    } else if spec.action.starts_with("k8s.") {
        ("k8s", "node", 2)
    } else if spec.action.starts_with("gpu.") {
        ("gpu", "gpu_id", 1)
    } else {
        if !path.is_empty() {
            bail!("EPERM peft-target-must-use-args");
        }
        let required: &[(&str, &str)] = match spec.action.as_str() {
            "peft.export" => &[("job_id", "job")],
            "peft.import" => &[
                ("job_id", "job"),
                ("model_id", "model"),
                ("adapter_dir", "from"),
            ],
            "peft.activate" => &[("model_id", "model")],
            _ => &[],
        };
        for (name, alias) in required {
            if !args.contains_key(*name) && !args.contains_key(*alias) {
                bail!("EPERM missing-provider-field {name}");
            }
        }
        return Ok(());
    };
    if !path.is_empty() {
        if path.first() != Some(&provider)
            || path.len() < index + 1
            || path.len() > index + 2
            || (provider == "k8s" && path.get(1) != Some(&"node"))
        {
            bail!("EPERM invalid-provider-target");
        }
        if let Some(suffix) = path.get(index + 1) {
            let expected = spec
                .action
                .strip_prefix(provider)
                .and_then(|v| v.strip_prefix('.'))
                .ok_or_else(|| anyhow!("EPERM invalid-provider-action"))?;
            let endpoint = if provider == "gpu" { "lease" } else { expected };
            if *suffix != endpoint && !(expected == "lease.sync" && *suffix == "lease-sync") {
                bail!("EPERM provider-target-action-mismatch");
            }
        }
        if let Some(value) = args.get(field) {
            if value.as_str() != path.get(index).copied() {
                bail!("EPERM ambiguous-provider-target");
            }
        }
    } else if !args.contains_key(field) && spec.action != "docker.status-check" {
        bail!("EPERM missing-provider-target");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn injection_ambiguous_targets_and_unknown_arguments_never_validate() {
        let mut spec = HostTicketSpec {
            id: "request".into(),
            idempotency_key: "retry".into(),
            action: "systemd.restart".into(),
            target: Some("/host/systemd/service.service/restart".into()),
            ..HostTicketSpec::default()
        };
        validate(&spec).expect("canonical target");
        for unit in ["", "-root", "../service", "a/b", "a\nb"] {
            spec.args = serde_json::json!({"unit":unit});
            assert!(validate(&spec).is_err());
        }
        spec.args = serde_json::json!({"unit":"different.service"});
        assert!(validate(&spec).is_err());
        spec.args = serde_json::json!({"extra":true});
        assert!(validate(&spec).is_err());
        spec.args = Value::Null;
        spec.target = Some("/host/systemd/service.service/stop".into());
        assert!(validate(&spec).is_err());
        spec.target = Some("/host//systemd/service.service/restart".into());
        assert!(validate(&spec).is_err());
    }
}
