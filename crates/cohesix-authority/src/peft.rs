// Author: Lukas Bower
// Purpose: Bind native private adapter release authority to immutable host inputs without accepting client receipts or commands.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Only a configured host CAS request can select the release and compensation scope.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArgs {
    pub request_sha256: String,
    /// Fresh authority may compensate only this original frozen release request.
    #[serde(default, skip_serializing_if = "is_false")]
    pub recovery_only: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// Shared no_std admission grammar for Root, host executor and controller.
pub fn validate_release_args(args: &serde_json::Value) -> bool {
    serde_json::from_value::<ReleaseArgs>(args.clone()).is_ok_and(|args| {
        args.request_sha256.len() == 64
            && args
                .request_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_input_cannot_supply_commands_paths_or_outcomes() {
        use serde_json::json;
        assert!(super::validate_release_args(
            &json!({"request_sha256":"a".repeat(64)})
        ));
        for bad in [
            json!({}),
            json!({"request_sha256":"../private"}),
            json!({"request_sha256":"A".repeat(64)}),
            json!({"request_sha256":"a".repeat(64),"approved":true}),
            json!({"request_sha256":"a".repeat(64),"command":"train"}),
            json!({"request_sha256":"a".repeat(64),"receipt":"pass"}),
        ] {
            assert!(!super::validate_release_args(&bad));
        }
    }
}
