// Author: Lukas Bower
// Purpose: Restore strict version-1 host ticket arguments and authority correlation while enforcing the generated writer fence.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

extern crate alloc;

use super::{
    generated, host_ticket_action_label, host_ticket_lifecycle_label, validate_host_ticket_token,
    HostState, NineDoorBridgeError,
};
use alloc::{collections::BTreeMap, string::String};
use cohesix_authority::AdmissionCorrelation;
use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer,
};

#[derive(Debug)]
enum Argument {
    Text(String),
    Unsigned(u64),
    Boolean(bool),
    Null,
}

impl<'de> Deserialize<'de> for Argument {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArgumentVisitor;
        impl<'de> Visitor<'de> for ArgumentVisitor {
            type Value = Argument;
            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a primitive provider argument")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > 1024 {
                    return Err(E::custom("provider text bound"));
                }
                Ok(Argument::Text(String::from(value)))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Argument::Unsigned(value))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(Argument::Boolean(value))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Argument::Null)
            }
        }
        // No sequence/map visitor: nested provider values fail before recursive
        // Content allocation, preserving the root parser's small stack budget.
        deserializer.deserialize_any(ArgumentVisitor)
    }
}

#[derive(Debug, Default)]
struct Arguments(BTreeMap<String, Argument>);

impl<'de> Deserialize<'de> for Arguments {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArgumentsVisitor;
        impl<'de> Visitor<'de> for ArgumentsVisitor {
            type Value = Arguments;
            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("bounded unique primitive provider arguments")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Argument>()? {
                    if key.len() > 64 || values.len() >= 32 || values.insert(key, value).is_some() {
                        return Err(de::Error::custom("invalid provider argument map"));
                    }
                }
                Ok(Arguments(values))
            }
        }
        deserializer.deserialize_map(ArgumentsVisitor)
    }
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    id: String,
    idempotency_key: String,
    action: String,
    #[serde(default, deserialize_with = "present")]
    writer_epoch: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    admission: Option<AdmissionCorrelation>,
    #[serde(default, deserialize_with = "present")]
    target: Option<String>,
    #[serde(default)]
    args: Option<Arguments>,
    #[serde(default, deserialize_with = "present")]
    expires_unix_ms: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    source_hive: Option<String>,
    #[serde(default, deserialize_with = "present")]
    target_hive: Option<String>,
    #[serde(default, deserialize_with = "present")]
    relay_hop: Option<u16>,
    #[serde(default, deserialize_with = "present")]
    relay_correlation_id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    receipt_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultRecord {
    schema: String,
    id: String,
    idempotency_key: String,
    action: String,
    state: String,
    #[serde(default, deserialize_with = "present")]
    writer_epoch: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    admission: Option<AdmissionCorrelation>,
    #[serde(default, deserialize_with = "present")]
    message: Option<String>,
    #[serde(default, deserialize_with = "present")]
    source_hive: Option<String>,
    #[serde(default, deserialize_with = "present")]
    target_hive: Option<String>,
    #[serde(default, deserialize_with = "present")]
    relay_hop: Option<u16>,
    #[serde(default, deserialize_with = "present")]
    relay_correlation_id: Option<String>,
    #[serde(default, deserialize_with = "present")]
    receipt_mode: Option<String>,
}

fn authority(
    epoch: Option<u64>,
    admission: Option<&AdmissionCorrelation>,
    required: bool,
    selected: u64,
) -> Result<(), NineDoorBridgeError> {
    if (required || epoch.is_some()) && epoch != Some(selected) {
        return Err(NineDoorBridgeError::Permission);
    }
    if let Some(correlation) = admission {
        correlation
            .validate()
            .map_err(|_| NineDoorBridgeError::InvalidPayload)?;
    }
    Ok(())
}

fn identity(
    id: &str,
    key: &str,
    action: &str,
    receipt_mode: Option<&str>,
    host: &HostState,
) -> Result<(), NineDoorBridgeError> {
    validate_host_ticket_token(id)?;
    validate_host_ticket_token(key)?;
    if !host
        .ticket_action_allowlist
        .iter()
        .any(|allowed| host_ticket_action_label(*allowed) == action)
        || host.receipt_action(action)
        || receipt_mode.is_some_and(|mode| mode != "none")
    {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

fn federation(
    source: Option<&str>,
    target: Option<&str>,
    hop: Option<u16>,
    correlation: Option<&str>,
) -> Result<(), NineDoorBridgeError> {
    if source.is_some() != target.is_some() || hop.is_some_and(|value| value == 0 || value > 32) {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    for value in [source, target, correlation].into_iter().flatten() {
        validate_host_ticket_token(value)?;
    }
    Ok(())
}

fn arguments(action: &str, values: Option<&Arguments>) -> Result<(), NineDoorBridgeError> {
    let Some(values) = values else {
        return Ok(());
    };
    let allowed = cohesix_authority::PROVIDER_V1_FIELDS
        .iter()
        .find(|(id, _)| *id == action)
        .map(|(_, fields)| *fields)
        .ok_or(NineDoorBridgeError::InvalidPayload)?;
    for (key, value) in &values.0 {
        if !allowed.contains(&key.as_str()) {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        match value {
            Argument::Text(text)
                if text.is_empty() || text.len() > 1024 || text.chars().any(char::is_control) =>
            {
                return Err(NineDoorBridgeError::InvalidPayload)
            }
            Argument::Unsigned(number) if *number > u32::MAX as u64 => {
                return Err(NineDoorBridgeError::InvalidPayload)
            }
            Argument::Boolean(value) => {
                let _ = value;
            }
            Argument::Null => return Err(NineDoorBridgeError::InvalidPayload),
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn requests(payload: &str, host: &HostState) -> Result<(), NineDoorBridgeError> {
    let mut saw_line = false;
    for line in payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        saw_line = true;
        host.validate_ticket_line_bytes(line)?;
        let request: Request =
            serde_json::from_str(line).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        if request.schema != host.ticket_request_schema
            || request.schema != "host-ticket/v1"
            || !host.accepted_request_schema(&request.schema)
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        authority(
            request.writer_epoch,
            request.admission.as_ref(),
            generated::AUTHORITY_POLICY.writer_epoch_required,
            generated::AUTHORITY_POLICY.writer_epoch,
        )?;
        identity(
            &request.id,
            &request.idempotency_key,
            &request.action,
            request.receipt_mode.as_deref(),
            host,
        )?;
        if request
            .target
            .as_deref()
            .is_some_and(|target| target.trim().is_empty())
            || request.expires_unix_ms == Some(0)
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        arguments(&request.action, request.args.as_ref())?;
        federation(
            request.source_hive.as_deref(),
            request.target_hive.as_deref(),
            request.relay_hop,
            request.relay_correlation_id.as_deref(),
        )?;
    }
    if !saw_line {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

pub(super) fn results(payload: &str, host: &HostState) -> Result<(), NineDoorBridgeError> {
    let mut saw_line = false;
    for line in payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        saw_line = true;
        host.validate_ticket_line_bytes(line)?;
        let result: ResultRecord =
            serde_json::from_str(line).map_err(|_| NineDoorBridgeError::InvalidPayload)?;
        if result.schema != host.ticket_result_schema
            || result.schema != "host-ticket-result/v1"
            || !host.accepted_result_schema(&result.schema)
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        authority(
            result.writer_epoch,
            result.admission.as_ref(),
            generated::AUTHORITY_POLICY.writer_epoch_required,
            generated::AUTHORITY_POLICY.writer_epoch,
        )?;
        identity(
            &result.id,
            &result.idempotency_key,
            &result.action,
            result.receipt_mode.as_deref(),
            host,
        )?;
        if !host
            .ticket_lifecycle
            .iter()
            .any(|allowed| host_ticket_lifecycle_label(*allowed) == result.state)
            || result
                .message
                .as_deref()
                .is_some_and(|message| message.trim().is_empty())
        {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        federation(
            result.source_hive.as_deref(),
            result.target_hive.as_deref(),
            result.relay_hop,
            result.relay_correlation_id.as_deref(),
        )?;
    }
    if !saw_line {
        return Err(NineDoorBridgeError::InvalidPayload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_v1_preserves_nested_args_and_refuses_ambiguous_fields_and_stale_writer() {
        let raw = r#"{"schema":"host-ticket/v1","id":"read-1","idempotency_key":"once","action":"systemd.status-check","args":{"unit":"ssh.service"},"writer_epoch":7,"receipt_mode":"none"}"#;
        let request: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(request.writer_epoch, Some(7));
        assert!(arguments(&request.action, request.args.as_ref()).is_ok());
        assert!(authority(Some(7), None, true, 7).is_ok());
        assert_eq!(
            authority(None, None, true, 7),
            Err(NineDoorBridgeError::Permission)
        );
        assert_eq!(
            authority(Some(6), None, true, 7),
            Err(NineDoorBridgeError::Permission)
        );
        assert!(authority(None, None, false, 7).is_ok());
        for bad in [
            raw.replace("\"writer_epoch\":7", "\"writer_epoch\":null"),
            raw.replace(
                "\"writer_epoch\":7",
                "\"writer_epoch\":7,\"writer_epoch\":7",
            ),
            raw.replace(
                "\"unit\":\"ssh.service\"",
                "\"unit\":\"ssh.service\",\"unit\":\"other.service\"",
            ),
            raw.replace("\"unit\":\"ssh.service\"", "\"unit\":[\"ssh.service\"]"),
            raw.replace("\"receipt_mode\":\"none\"", "\"unknown\":true"),
        ] {
            assert!(serde_json::from_str::<Request>(&bad).is_err());
        }
        let result:ResultRecord=serde_json::from_str(r#"{"schema":"host-ticket-result/v1","id":"read-1","idempotency_key":"once","action":"systemd.status-check","state":"succeeded","message":"native_observation=sha256:abc","writer_epoch":7,"receipt_mode":"none"}"#).unwrap();
        assert_eq!(result.writer_epoch, Some(7));
    }
}
