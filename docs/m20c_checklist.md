# Author: Lukas Bower
# Purpose: Milestone 20c checklist and DoD evidence mapping.

# Milestone 20c Checklist (SwarmUI Desktop)

## Deliverables
- [ ] `apps/swarmui/` Tauri app linked to `cohsh-core` (host-only; no HTTP/REST deps).
- [ ] Telemetry Rings panel (tail `/worker/*/telemetry`).
- [ ] Fleet Map panel (read `/proc/ingest/*` + worker directories).
- [ ] Optional Namespace Browser (read-only tree over `/proc`, `/queen`, `/worker`, `/log`, `/gpu`).
- [ ] Offline inspection via bounded CBOR cache under `$DATA_DIR/snapshots/` (opt-in; read-only when offline).
- [ ] Ticket/lease auth identical to CLI; per-ticket session views; role-scoped interactions enforced client-side.

## Commands
- [ ] `cargo test -p cohsh-core`
- [ ] `cargo test -p swarmui`
- [ ] `cargo run -p cohsh --features tcp -- --transport tcp --script scripts/cohsh/telemetry_ring.coh`

## Checks (DoD) + Evidence Mapping
- [ ] UI telemetry renders `OK ...` then stream and terminates with `END`, byte-stable vs CLI.
  - Evidence: `apps/swarmui/tests/transcript.rs` diff output + stored transcript artifact.
- [ ] No HTTP/REST dependencies (audit or cargo deny).
  - Evidence: `apps/swarmui/tests/no_http_deps.rs` output (or CI audit log).
- [ ] Unauthorized/expired ticket returns `ERR` verbatim, audit logged; offline mode uses cached CBOR only.
  - Evidence: SwarmUI transcript test coverage + audit log capture + cache test in `apps/swarmui/tests/cache.rs`.
- [ ] UI/CLI/console ACK/ERR/END sequences byte-stable vs 7c baseline.
  - Evidence: transcript parity test + CLI golden transcript diff.
- [ ] No background polling when idle.
  - Evidence: code inspection in `apps/swarmui/src-tauri/main.rs` + test asserting no background watcher.

## Task Breakdown
- [ ] m20c-ui-backend
  - [ ] `apps/swarmui/src-tauri/main.rs` session management (per ticket), ticket auth, telemetry tail via cohsh-core.
  - [ ] `apps/swarmui/Cargo.toml` no HTTP/REST deps; bounded offline cache feature enabled.
  - [ ] Unauthorized ticket returns `ERR` surfaced verbatim; offline mode reads CBOR snapshot only.
  - [ ] Document UI backend notes and cache path in `docs/USERLAND_AND_CLI.md`.
- [ ] m20c-ui-fixtures
  - [ ] `apps/swarmui/tests/transcript.rs` compares UI ACK/ERR/END to CLI golden transcript.
  - [ ] `apps/swarmui/src/cache.rs` snapshot read/write with strict size bounds and expiry handling.
  - [ ] `docs/INTERFACES.md` updated with SwarmUI consumption guidance + non-goals.
