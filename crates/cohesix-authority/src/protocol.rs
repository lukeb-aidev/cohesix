// Author: Lukas Bower
// Purpose: Share the compiler-controlled ceiling for host agent protocol entry points.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::string::{String, ToString};
use serde::{Deserialize, Serialize};

/// The host protocol configuration has a schema independent of the root-task ABI.
pub const PROTOCOL_SCHEMA: &str = "cohesix-agent-protocol-controls/v1";

/// One manifest switch; missing switches are disabled during migration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProtocolSwitch {
    pub enabled: bool,
}

/// Immutable compiler-selected ceiling shared by gateway and clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProtocolControls {
    pub schema: String,
    pub agent_protocols: ProtocolSwitch,
    pub mcp: ProtocolSwitch,
    pub a2a: ProtocolSwitch,
}

impl Default for ProtocolControls {
    fn default() -> Self {
        Self {
            schema: PROTOCOL_SCHEMA.to_string(),
            agent_protocols: ProtocolSwitch::default(),
            mcp: ProtocolSwitch::default(),
            a2a: ProtocolSwitch::default(),
        }
    }
}

impl ProtocolControls {
    /// Read only the protocol fragment from a compiler-resolved manifest.
    /// Older manifests have no fragment and retain the disabled default.
    pub fn from_resolved_manifest(bytes: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(default)]
            gateway: ProtocolControls,
        }
        let controls: Envelope =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        controls.gateway.validate().map_err(str::to_string)?;
        Ok(controls.gateway)
    }

    /// A subprotocol is usable only when its own switch and the master are set.
    #[must_use]
    pub const fn effective_mcp(&self) -> bool {
        self.agent_protocols.enabled && self.mcp.enabled
    }

    /// A subprotocol is usable only when its own switch and the master are set.
    #[must_use]
    pub const fn effective_a2a(&self) -> bool {
        self.agent_protocols.enabled && self.a2a.enabled
    }

    /// Unknown schemas never silently inherit a more permissive interpretation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PROTOCOL_SCHEMA {
            return Err("unsupported gateway protocol control schema");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_combination_is_a_conjunction() {
        for mask in 0..8 {
            let controls = ProtocolControls {
                agent_protocols: ProtocolSwitch {
                    enabled: mask & 1 != 0,
                },
                mcp: ProtocolSwitch {
                    enabled: mask & 2 != 0,
                },
                a2a: ProtocolSwitch {
                    enabled: mask & 4 != 0,
                },
                ..ProtocolControls::default()
            };
            assert_eq!(controls.effective_mcp(), mask & 3 == 3);
            assert_eq!(controls.effective_a2a(), mask & 5 == 5);
        }
    }

    #[test]
    fn old_missing_configuration_is_disabled_and_invalid_shapes_fail() {
        let absent: ProtocolControls = serde_json::from_str("{}").expect("missing flags");
        assert!(!absent.effective_mcp() && !absent.effective_a2a());
        for raw in [
            r#"{"schema":"future"}"#,
            r#"{"mcp":{"enabled":"yes"}}"#,
            r#"{"mcp":{"enabled":true,"unknown":1}}"#,
        ] {
            match serde_json::from_str::<ProtocolControls>(raw) {
                Ok(value) => assert!(value.validate().is_err()),
                Err(_) => {}
            }
        }
        let old = ProtocolControls::from_resolved_manifest(br#"{"root_task":{}}"#)
            .expect("old manifest stays disabled");
        assert!(!old.effective_mcp() && !old.effective_a2a());
        assert!(
            ProtocolControls::from_resolved_manifest(br#"{"gateway":{"mcp":{"enabled":1}}}"#)
                .is_err()
        );
    }
}
