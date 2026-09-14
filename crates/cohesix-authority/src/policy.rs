// Author: Lukas Bower
// Purpose: Define compiler-owned authority limits without promoting future VM or admission claims.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Host/gateway write policy; the VM still authenticates the gateway session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AuthorityPolicy {
    pub production: bool,
    pub delegated_rest: bool,
    pub vm_verified_delegation: bool,
    pub delegated_ticket_entries: u16,
    pub delegated_ticket_max_ttl_s: u32,
    pub strict_queen_intents: bool,
    pub legacy_queen_ctl: bool,
    pub queen_dedupe_entries: u16,
    pub queen_intent_max_bytes: u16,
    pub writer_epoch: u64,
    pub writer_epoch_required: bool,
    pub execution_wal_required: bool,
    pub gpu_frame_max_bytes: u32,
    pub debug_memory: bool,
    pub production_worker_ledger: bool,
    pub production_driver_ledger: bool,
    pub structured_quarantine: bool,
    pub host_ai: bool,
    pub production_failover: bool,
}

impl Default for AuthorityPolicy {
    fn default() -> Self {
        Self {
            production: false,
            delegated_rest: true,
            vm_verified_delegation: false,
            delegated_ticket_entries: 256,
            delegated_ticket_max_ttl_s: 3600,
            strict_queen_intents: true,
            legacy_queen_ctl: true,
            queen_dedupe_entries: 64,
            queen_intent_max_bytes: 2048,
            writer_epoch: 1,
            writer_epoch_required: false,
            execution_wal_required: true,
            gpu_frame_max_bytes: 8192,
            debug_memory: false,
            production_worker_ledger: false,
            production_driver_ledger: false,
            structured_quarantine: false,
            host_ai: false,
            production_failover: false,
        }
    }
}

impl AuthorityPolicy {
    /// Bounded operator snapshot, separate from future VM admission identity.
    pub fn snapshot_bytes(&self) -> Result<alloc::vec::Vec<u8>, serde_json::Error> {
        #[derive(Serialize)]
        struct Snapshot {
            schema: &'static str,
            identity: &'static str,
            writer_epoch: u64,
            epoch_required: bool,
            production: bool,
            debug_memory: bool,
            strict_intents: bool,
        }
        serde_json::to_vec(&Snapshot {
            schema: "authority/v1",
            identity: "gateway_enforced",
            writer_epoch: self.writer_epoch,
            epoch_required: self.writer_epoch_required,
            production: self.production,
            debug_memory: self.debug_memory,
            strict_intents: self.strict_queen_intents,
        })
    }
}

/// Redacted gateway authority and benchmark counters; no caller credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegationStatus {
    pub identity_class: alloc::string::String,
    pub writer_epoch: u64,
    pub cache_entries: usize,
    pub cache_capacity: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub refusals: u64,
    pub audit_records: u64,
    pub audit_emit_ns: u64,
}
