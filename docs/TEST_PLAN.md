<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Purpose
Ensure cohsh and SwarmUI share console grammar parity, ticket quotas are enforced, multi-worker flows are stable, and deterministic trace replay is verifiable for Milestone 20g.

## Scope (Milestone 20g goals)
- Cohsh command surface works against queen and multiple workers without ACK/ERR/END drift.
- `scripts/cohsh/run_regression_batch.sh` is the reliable manual compliance pack for BUILD_PLAN.md.
- SwarmUI console transport matches cohsh grammar and transcript fixtures.
- Trace record/replay fixtures are deterministic; replay parity holds across cohsh, SwarmUI, and coh-status.

## Preflight and guardrails
- `scripts/ci/check_test_plan.sh`
- If IR or manifest changes: `cargo run -p coh-rtc` then `scripts/check-generated.sh`.
- Ensure `SEL4_BUILD_DIR=$HOME/seL4/build`.
- Before any QEMU TCP run, prompt the operator to start tcpdump and confirm the log path (example: `logs/tcpdump-new-20260121-140023.log`). Do not proceed until confirmed.
- Clear old regression logs if needed: `rm -rf out/regression-logs`.

## Execution order (Milestone 20g)
Run in the order shown; all commands are macOS ARM64 compatible.
- Milestone 20g requires steps 1 and 1b; steps 2–7 are the Milestone 20h release gate.

### 1) Host-only unit and integration tests
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --test script_catalog`
- `cargo test -p cohsh --test transcripts`
- `cargo test -p cohsh --test client_lib`
- `cargo test -p coh-status --test transcript`
- `cargo test -p swarmui --test transcript`
- `cargo test -p swarmui --test security`
- `cargo test -p nine-door --test ui_security`

### 1b) Trace replay (Milestone 20g)
- `cargo test -p cohsh-core --test trace`
- `cargo test -p cohsh --test trace`
- `cargo test -p swarmui --test trace`
- `cargo test -p coh-status --test trace`
- Fixture regen (only when needed): `COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace`

### 2) Convergence harness (Milestone 20e)
- `scripts/regression/transcript_compare.sh`
- `scripts/regression/client_vs_console.sh`
- Timing tolerance is 50 ms (test harness tolerance, not a protocol contract).
- Fixture normalization: transcripts include only `OK ...`, `ERR ...`, and `END`; `OK/ERR AUTH` lines are excluded.
- Converge sequence: `help -> attach -> log -> spawn -> tail -> quit` with `spawn` executed via `/queen/ctl`.

### 3) QEMU regression batch (manual compliance pack)
- `scripts/cohsh/run_regression_batch.sh`
- The batch runs base + gated scripts and archives logs to `out/regression-logs/<batch>/<script>.{qemu,out}.log`.
- Worker-id dependent scripts are isolated into their own QEMU boots to keep deterministic `worker-1`/`worker-2` paths.
- Override timeouts with `READY_TIMEOUT`, `PORT_TIMEOUT`, `QUIT_CLOSE_TIMEOUT` when needed.
- For cold builds, set `READY_TIMEOUT=600` so the initial boot has enough time before the ready marker.

### 4) Cohsh command surface (manual checklist)
After a QEMU boot with TCP transport:
- Attach as queen: `help`, `log`, `tail /log/queen.log`, `cat /log/queen.log`, `ls /`, `echo /log/queen.log`, `spawn`, `kill`, `ping`, `test --mode quick`, `pool bench <opts>`, `tcp-diag`, `bind <src> <dst>`, `mount <service> <path>`, `detach`, `quit` (quit last).
- Multi-worker coverage (queen session): spawn two workers, tail `/shard/<label>/worker/<id>/telemetry` for each, and kill both without path errors.
- CLI-local commands: `detach`, `pool bench <opts>`, `bind <src> <dst>` (expect `OK DETACH`, pool bench summary, and `OK BIND`).
- Success criteria: no invalid UTF-8 frames, no reconnect loops, `OK/ERR/END` ordering stable.

### 5) QEMU ↔ cohsh console correlation (manual)
During the command-surface checklist, capture both sides and confirm there are no unexpected resets.
- Confirm tcpdump capture is active for this run before comparing logs; if not, stop and ask the operator to restart with tcpdump enabled.
- Capture cohsh output (example): `./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen | tee out/manual/cohsh-session.log`
- Use the QEMU serial log from the same run (example): `out/cohesix/logs/qemu-live-*.log`
- Fail the run if any unexpected disconnects appear:
  - QEMU log: `rg -n "audit tcp\\.conn\\.close reason=error|audit tcp\\.send\\.partial|audit tcp\\.send\\.error|console\\.emit\\.failed" out/cohesix/logs/qemu-live-*.log`
  - cohsh log: `rg -n "\\[cohsh\\]\\[tcp\\] connection lost" out/manual/cohsh-session.log`
- tcpdump: `rg -n "Flags \\[R\\]" logs/tcpdump-new-20260121-140023.log`
- Acceptable disconnects: explicit `quit` (reason=`quit`/`eof`) or pool bench with injected short writes. Anything else is a failure.
- `audit tcp.flush.blocked` lines before any client connects are expected; do not treat them as failures.

### 6) SwarmUI console grammar alignment
- `cargo test -p swarmui --test transcript`
- `cargo test -p swarmui --test security`
- Manual smoke: run SwarmUI with console transport, connect to the queen session, confirm ACK/ERR/END lines match cohsh fixtures during tail/cat/spawn/kill flows.

### 7) In-session selftests (manual, QEMU)
After a QEMU boot with TCP transport:
- `./out/cohesix/host-tools/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen`
- Run: `test --mode quick` during command surface, then run `test --mode full` in a fresh boot (before any worker spawns).
- Selftest scripts live under `/proc/tests/` and must pass after console, namespace, or policy changes.

## Trace replay limits
<!-- coh-rtc:trace-policy:start -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Generated trace replay snippet consumed by docs/TEST_PLAN.md. -->

### Trace replay limits (generated)
- `trace.format.version`: `1`
- `trace.hash`: `sha256`
- `trace.max_bytes`: `1048576`
- `trace.max_frame_bytes`: `8192`
- `trace.max_ack_bytes`: `256`

_Generated by coh-rtc (sha256: `c502a57721e43d5c38f5499767a8668eb593ac74f25cb2389632804c4d7f15f2`)._
<!-- coh-rtc:trace-policy:end -->

## Manifest fingerprints
- `out/manifests/root_task_resolved.json` — `sha256:61c0fcf26398e77b38f9ea82dc2f1a619bd3151de43f90acab748b9a7dc88435`

## Transcript fixture hashes
- `tests/fixtures/transcripts/boot_v0/serial.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/boot_v0/core.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/boot_v0/tcp.txt` — `sha256:2ea58218a937f0c702fd67dac83aa838a8c49b9d1fba1e0165dfa93a44ab3c6d`
- `tests/fixtures/transcripts/abuse/serial.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/core.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/tcp.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/converge_v0/serial.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/core.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/tcp.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/cohsh.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/trace_v0/cohsh.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/swarmui.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/coh-status.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`

## Trace fixture hashes
- `tests/fixtures/traces/trace_v0.trace` — `sha256:0f5a1935e973fbdb57e73a952b9cd02d1060086167efb4b9e79b28169f308561`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes above match on-disk fixtures and manifest fingerprints; `scripts/check-generated.sh` invokes it.
