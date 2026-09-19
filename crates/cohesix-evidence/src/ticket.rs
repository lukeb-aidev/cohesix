// Author: Lukas Bower
// Purpose: Bind signed host-ticket intents to exact Root-resolved Worker admissions without accepting caller-authored resolution fields.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use crate::{Binding, Error};
use serde_json::Value;
use sha2::{Digest, Sha256};

const RESOLVED: [&str; 3] = [
    "resolved_worker_slot",
    "resolved_lease_epoch",
    "admission_sequence",
];

/// Exact Root correlation namespace; length prefixes prevent concatenation collisions.
pub fn current_path(id: &str, idempotency: &str) -> Result<String, Error> {
    crate::bounded_id(id)?;
    crate::bounded_id(idempotency)?;
    let mut hash = Sha256::new();
    hash.update(b"host-ticket-correlation/v1\0");
    hash.update(
        u16::try_from(id.len())
            .map_err(|_| Error::Limit)?
            .to_be_bytes(),
    );
    hash.update(id.as_bytes());
    hash.update(
        u16::try_from(idempotency.len())
            .map_err(|_| Error::Limit)?
            .to_be_bytes(),
    );
    hash.update(idempotency.as_bytes());
    Ok(format!(
        "/host/tickets/current/{}",
        hex::encode(hash.finalize())
    ))
}

/// Normalize only the documented optional args and Root-owned v2 resolution.
pub fn caller_request(request: &Value, admitted: bool) -> Result<Value, Error> {
    let mut request = request.clone();
    let v2 = request["schema"] == "host-ticket/v2";
    if !v2 && request["schema"] != "host-ticket/v1" {
        return Err(Error::Schema);
    }
    let object = request.as_object_mut().ok_or(Error::Schema)?;
    if object.get("args").is_some_and(Value::is_null) {
        object.remove("args");
    }
    for name in RESOLVED {
        if v2 && admitted {
            let value = object.remove(name).ok_or(Error::Identity)?;
            let number = value.as_u64().ok_or(Error::Identity)?;
            if (name == "resolved_worker_slot" && number > u16::MAX.into())
                || (name != "resolved_worker_slot" && number == 0)
            {
                return Err(Error::Identity);
            }
        } else if object.contains_key(name) {
            return Err(Error::Identity);
        }
    }
    Ok(request)
}

/// Independent enrollment pins the image and supervisor generation. Root's
/// signed admission artifact additionally pins slot, lease and cap generations.
pub fn require_binding(binding: &Binding, request: &Value, admitted: bool) -> Result<(), Error> {
    caller_request(request, admitted)?;
    if request["id"] != binding.ticket_id
        || request["idempotency_key"] != binding.idempotency_key
        || request["action"] != binding.action
        || request["writer_epoch"].as_u64() != Some(binding.writer_epoch)
    {
        return Err(Error::Identity);
    }
    match (&binding.worker, request["schema"].as_str()) {
        (None, Some("host-ticket/v1")) => Ok(()),
        (Some(worker), Some("host-ticket/v2"))
            if request["receipt_mode"] == "worker"
                && request["receipt_worker_id"] == worker.id
                && request["receipt_worker_role"] == worker.role
                && request["receipt_supervisor_generation"].as_u64() == Some(worker.generation)
                && request["receipt_cap_generation"]
                    .as_u64()
                    .is_some_and(|v| v > 0) =>
        {
            Ok(())
        }
        _ => Err(Error::Identity),
    }
}

/// A retained Root row must match every caller field, with exactly its three
/// additional v2 resolution fields. Duplicate identities are handled by callers.
pub fn matches_admission(row: &Value, caller: &Value) -> Result<bool, Error> {
    Ok(caller_request(row, row["schema"] == "host-ticket/v2")? == caller_request(caller, false)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_is_root_only_and_cannot_change_caller_fields() {
        let raw = serde_json::json!({"schema":"host-ticket/v2", "id":"ticket",
            "idempotency_key":"once", "action":"gpu.workload.submit", "args":{"lease_id":"lease"}});
        let mut row = raw.clone();
        row["resolved_worker_slot"] = 0.into();
        row["resolved_lease_epoch"] = 7.into();
        row["admission_sequence"] = 9.into();
        assert!(matches_admission(&row, &raw).unwrap());
        assert!(caller_request(&row, false).is_err());
        assert!(caller_request(&raw, true).is_err());
        row["resolved_lease_epoch"] = 0.into();
        assert!(matches_admission(&row, &raw).is_err());
        row["resolved_lease_epoch"] = 7.into();
        row["args"]["lease_id"] = "different".into();
        assert!(!matches_admission(&row, &raw).unwrap());
        row = serde_json::json!({"schema":"host-ticket/v1", "args":null});
        assert_eq!(
            caller_request(&row, false).unwrap(),
            serde_json::json!({"schema":"host-ticket/v1"})
        );
        row["resolved_worker_slot"] = 1.into();
        assert!(caller_request(&row, true).is_err());
    }
}
