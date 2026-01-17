// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define UI provider configuration and path matching helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use super::cas::validate_epoch;
use secure9p_codec::MAX_MSIZE;

/// Maximum bytes permitted per UI provider read.
pub const UI_MAX_READ_BYTES: u32 = MAX_MSIZE;
/// Hard cap for total UI provider stream output.
pub const UI_MAX_STREAM_BYTES: usize = 32 * 1024;

/// UI provider configuration for /proc/9p nodes.
#[derive(Debug, Clone, Copy)]
pub struct UiProc9pConfig {
    /// Enable `/proc/9p/sessions` providers.
    pub sessions: bool,
    /// Enable `/proc/9p/outstanding` providers.
    pub outstanding: bool,
    /// Enable `/proc/9p/short_writes` providers.
    pub short_writes: bool,
}

/// UI provider configuration for /proc/ingest nodes.
#[derive(Debug, Clone, Copy)]
pub struct UiProcIngestConfig {
    /// Enable `/proc/ingest/p50_ms` providers.
    pub p50_ms: bool,
    /// Enable `/proc/ingest/p95_ms` providers.
    pub p95_ms: bool,
    /// Enable `/proc/ingest/backpressure` providers.
    pub backpressure: bool,
}

/// UI provider configuration for policy preflight nodes.
#[derive(Debug, Clone, Copy)]
pub struct UiPolicyPreflightConfig {
    /// Enable `/policy/preflight/req` providers.
    pub req: bool,
    /// Enable `/policy/preflight/diff` providers.
    pub diff: bool,
}

/// UI provider configuration for update status nodes.
#[derive(Debug, Clone, Copy)]
pub struct UiUpdatesConfig {
    /// Enable `/updates/<epoch>/manifest.cbor` providers.
    pub manifest: bool,
    /// Enable `/updates/<epoch>/status` providers.
    pub status: bool,
}

/// Top-level UI provider configuration.
#[derive(Debug, Clone, Copy)]
pub struct UiProviderConfig {
    /// /proc/9p provider toggles.
    pub proc_9p: UiProc9pConfig,
    /// /proc/ingest provider toggles.
    pub proc_ingest: UiProcIngestConfig,
    /// /policy/preflight provider toggles.
    pub policy_preflight: UiPolicyPreflightConfig,
    /// /updates UI provider toggles.
    pub updates: UiUpdatesConfig,
}

impl UiProviderConfig {
    /// Return a config with all UI providers disabled.
    pub fn disabled() -> Self {
        Self {
            proc_9p: UiProc9pConfig {
                sessions: false,
                outstanding: false,
                short_writes: false,
            },
            proc_ingest: UiProcIngestConfig {
                p50_ms: false,
                p95_ms: false,
                backpressure: false,
            },
            policy_preflight: UiPolicyPreflightConfig {
                req: false,
                diff: false,
            },
            updates: UiUpdatesConfig {
                manifest: false,
                status: false,
            },
        }
    }
}

impl Default for UiProviderConfig {
    fn default() -> Self {
        Self {
            proc_9p: UiProc9pConfig {
                sessions: true,
                outstanding: true,
                short_writes: true,
            },
            proc_ingest: UiProcIngestConfig {
                p50_ms: true,
                p95_ms: true,
                backpressure: true,
            },
            policy_preflight: UiPolicyPreflightConfig {
                req: true,
                diff: true,
            },
            updates: UiUpdatesConfig {
                manifest: true,
                status: true,
            },
        }
    }
}

/// UI provider variant for text vs CBOR outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiVariant {
    /// Plain-text output.
    Text,
    /// CBOR output.
    Cbor,
}

/// Known UI provider nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiProviderKind {
    /// `/proc/9p/sessions`
    Proc9pSessions,
    /// `/proc/9p/outstanding`
    Proc9pOutstanding,
    /// `/proc/9p/short_writes`
    Proc9pShortWrites,
    /// `/proc/ingest/p50_ms`
    ProcIngestP50,
    /// `/proc/ingest/p95_ms`
    ProcIngestP95,
    /// `/proc/ingest/backpressure`
    ProcIngestBackpressure,
    /// `/policy/preflight/req`
    PolicyPreflightReq,
    /// `/policy/preflight/diff`
    PolicyPreflightDiff,
    /// `/updates/<epoch>/manifest.cbor`
    UpdatesManifest,
    /// `/updates/<epoch>/status`
    UpdatesStatus,
}

impl UiProviderKind {
    /// Return a stable label for audit logs.
    pub fn label(self) -> &'static str {
        match self {
            UiProviderKind::Proc9pSessions => "proc/9p/sessions",
            UiProviderKind::Proc9pOutstanding => "proc/9p/outstanding",
            UiProviderKind::Proc9pShortWrites => "proc/9p/short_writes",
            UiProviderKind::ProcIngestP50 => "proc/ingest/p50_ms",
            UiProviderKind::ProcIngestP95 => "proc/ingest/p95_ms",
            UiProviderKind::ProcIngestBackpressure => "proc/ingest/backpressure",
            UiProviderKind::PolicyPreflightReq => "policy/preflight/req",
            UiProviderKind::PolicyPreflightDiff => "policy/preflight/diff",
            UiProviderKind::UpdatesManifest => "updates/<epoch>/manifest.cbor",
            UiProviderKind::UpdatesStatus => "updates/<epoch>/status",
        }
    }
}

/// Matched UI provider path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiProviderMatch {
    /// Provider kind.
    pub kind: UiProviderKind,
    /// Variant type (text/CBOR).
    pub variant: UiVariant,
}

/// Identify a UI provider for a canonical path.
pub fn match_ui_provider(path: &[String]) -> Option<UiProviderMatch> {
    match path {
        [a, b, c] if a == "proc" && b == "9p" && c == "sessions" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pSessions,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "9p" && c == "sessions.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pSessions,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "proc" && b == "9p" && c == "outstanding" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pOutstanding,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "9p" && c == "outstanding.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pOutstanding,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "proc" && b == "9p" && c == "short_writes" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pShortWrites,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "9p" && c == "short_writes.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::Proc9pShortWrites,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "p50_ms" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestP50,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "p50_ms.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestP50,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "p95_ms" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestP95,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "p95_ms.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestP95,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "backpressure" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestBackpressure,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "proc" && b == "ingest" && c == "backpressure.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::ProcIngestBackpressure,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "policy" && b == "preflight" && c == "req" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::PolicyPreflightReq,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "policy" && b == "preflight" && c == "req.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::PolicyPreflightReq,
                variant: UiVariant::Cbor,
            });
        }
        [a, b, c] if a == "policy" && b == "preflight" && c == "diff" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::PolicyPreflightDiff,
                variant: UiVariant::Text,
            });
        }
        [a, b, c] if a == "policy" && b == "preflight" && c == "diff.cbor" => {
            return Some(UiProviderMatch {
                kind: UiProviderKind::PolicyPreflightDiff,
                variant: UiVariant::Cbor,
            });
        }
        [a, epoch, leaf] if a == "updates" && leaf == "manifest.cbor" => {
            if validate_epoch(epoch).is_ok() {
                return Some(UiProviderMatch {
                    kind: UiProviderKind::UpdatesManifest,
                    variant: UiVariant::Cbor,
                });
            }
        }
        [a, epoch, leaf] if a == "updates" && leaf == "status" => {
            if validate_epoch(epoch).is_ok() {
                return Some(UiProviderMatch {
                    kind: UiProviderKind::UpdatesStatus,
                    variant: UiVariant::Text,
                });
            }
        }
        [a, epoch, leaf] if a == "updates" && leaf == "status.cbor" => {
            if validate_epoch(epoch).is_ok() {
                return Some(UiProviderMatch {
                    kind: UiProviderKind::UpdatesStatus,
                    variant: UiVariant::Cbor,
                });
            }
        }
        _ => {}
    }
    None
}
