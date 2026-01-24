<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Purpose
Validate the full Cohesix stack end-to-end: generated artifacts, QEMU boot, TCP console reliability and performance, deterministic replay, and every shipped host tool.

## Goals
- Pre-existing features continue to work; new features are validated against documented behaviour.
- QEMU boots the VM and exposes Secure9P/TCP console without protocol drift.
- TCP console remains reliable under load (no unexpected disconnects/resets/partial writes).
- Performance baselines are captured for TCP throughput/latency.
- Host tools behave correctly: `cohsh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`.
- Deterministic replay passes for cohsh and SwarmUI (trace + hive snapshot).
- Fixtures and manifests remain hash-consistent.

## Scope
- Source tree validation (macOS 26 ARM64 host).
- Release bundle validation on macOS 26 and Ubuntu 24 aarch64.
- Milestone-agnostic: run sections appropriate to the change set.

## Preflight and guardrails
- `scripts/ci/check_test_plan.sh`
- If IR or manifest changes: `cargo run -p coh-rtc` then `scripts/check-generated.sh`.
- Ensure `SEL4_BUILD_DIR=$HOME/seL4/build`.
- Before any QEMU TCP run, start tcpdump and confirm the log path (example: `logs/tcpdump-new-YYYYMMDD-HHMMSS.log`). Use the same path in TCP correlation checks.
- Headless Linux requires `xvfb-run` (`sudo apt-get install -y xvfb` if missing).
- Ensure `/updates` and `/host` are enabled for host tool tests:
  - `cas.enable = true` (and `ui_providers.updates.*` as needed)
  - `ecosystem.host.enable = true` with providers set
  - Re-run `coh-rtc` and `scripts/check-generated.sh` if toggled.
- Clear old logs if needed: `rm -rf out/regression-logs logs`.

## Execution order
Run in order unless explicitly skipped with a recorded reason.

### 1) Artifact and fixture integrity
- `scripts/ci/check_test_plan.sh`
- If IR/manifest changed:
  - `scripts/check-generated.sh`

### 2) Host-side unit/integration tests (fast)
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --test ticket_mint`
- `cargo test -p cohsh --test transcripts`
- `cargo test -p swarmui --test transcript`
- `cargo test -p swarmui --test security`
- `cargo test -p nine-door --test ui_security`
- `cargo test -p nine-door --test telemetry_create`
- `cargo test -p nine-door --test telemetry_quotas`
- `cargo test -p nine-door --test telemetry_envelope`
- `cargo test -p cohsh-core --test trace`
- `cargo test -p cohsh --test trace`
- `cargo test -p swarmui --test trace`
- Fixture regen (only when needed):
  - `COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace`
  - `COHESIX_WRITE_TRACE=1 cargo test -p swarmui --test trace`

### 3) QEMU boot + TCP console baseline
Start QEMU (source tree or bundle), then verify:
- Capture QEMU serial to `logs/qemu-console.log` (example: `./qemu/run.sh | tee logs/qemu-console.log`).
- `cohsh` (queen): `help`, `attach queen` (skip if you launched cohsh with `--role`),
  `log`, `tail /log/queen.log`, `ls /`, `cat /log/queen.log`,
  `spawn heartbeat ticks=100`, `ls /worker`, `kill worker-<id>`, `ping`,
  `tcp-diag`, `test --mode quick`, `test --mode full` (fresh boot), `quit`
- Capture cohsh output to `logs/cohsh-session.log` (example: `... | tee logs/cohsh-session.log`).
- Success criteria:
  - No unexpected `ERR` lines or reconnect loops.
  - ACK/ERR/END ordering stable.

### 4) TCP reliability & performance (QEMU)
Run while QEMU is up:
- Repeat `tcp-diag` 5–10 times and record results (example: `... | tee logs/tcp-diag.log`).
- Run `pool bench path=/log/queen.log ops=500 batch=8 payload_bytes=64` and record throughput/latency (example: `... | tee logs/pool-bench.log`).
- Reasonable acceptance:
  - `tcp-diag` has zero failures.
  - `pool bench` shows non-zero throughput and stable latency.
  - Any >20% regression vs the last baseline on the same host is a defect to investigate.
- Capture logs:
  - cohsh: `logs/cohsh-session.log`
  - QEMU serial: `logs/qemu-console.log`
  - tcpdump: recorded tcpdump log path
- Fail if any unexpected disconnects:
  - QEMU log: `rg -n "audit tcp\\.conn\\.close reason=error|audit tcp\\.send\\.partial|audit tcp\\.send\\.error|console\\.emit\\.failed" logs/qemu-console.log`
  - cohsh log: `rg -n "\\[cohsh\\]\\[tcp\\] connection lost" logs/cohsh-session.log`
  - tcpdump: `rg -n "Flags \\[R\\]" <tcpdump-log-path>`
- Acceptable disconnects: explicit `quit` or EOF; anything else is a defect.
- `audit tcp.flush.blocked` lines before any client connects are expected; do not treat them as failures.

### 5) Host tools integration (QEMU running)
- QEMU log correlation (required):
  - Record a short note per tool in `logs/host-tool-runs.md` with start/stop time and tool name.
  - In the QEMU log, locate matching `audit tcp.conn.open`/`audit tcp.conn.close` lines for the same window.
  - Verify the session ends cleanly (`reason=quit`/`eof`) and no TCP errors are present in that window.
  - Use: `rg -n "audit tcp\\.conn\\.open|audit tcp\\.conn\\.close|audit tcp\\.send\\.partial|audit tcp\\.send\\.error|console\\.emit\\.failed" logs/qemu-console.log`
- `cohsh` (already covered in Section 3).
- `swarmui` live (observe only):
  - macOS: `./bin/swarmui`
  - headless Linux: `xvfb-run -a ./bin/swarmui`
- `swarmui` replay:
  - Source tree: `./bin/swarmui --replay-trace "$(pwd)/tests/fixtures/traces/trace_v0.trace"`
  - Release bundle: `./bin/swarmui --replay-trace "$(pwd)/traces/trace_v0.trace"`
  - Source tree: `./bin/swarmui --replay "$(pwd)/tests/fixtures/traces/trace_v0.hive.cbor"`
  - Release bundle: `./bin/swarmui --replay "$(pwd)/traces/trace_v0.hive.cbor"`
  - headless Linux: prefix with `xvfb-run -a`
- `cas-tool`:
  - Pad the trace to a 128-byte multiple (matches `cas.store.chunk_bytes`):
    ```bash
    python3 - <<'PY'
    from pathlib import Path
    src = Path("tests/fixtures/traces/trace_v0.trace")
    dst = Path("out/cas/trace_v0.padded")
    data = src.read_bytes()
    pad = (-len(data)) % 128
    dst.write_bytes(data + b"\0" * pad)
    print(f"padded {len(data)} -> {len(data) + pad} bytes")
    PY
    ```
  - Source tree: `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key ./resources/fixtures/cas_signing_key.hex`
  - Release bundle: pad `./traces/trace_v0.trace` into `./out/cas/trace_v0.padded`, then run `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key <path>`
  - `./bin/cas-tool upload --bundle ./out/cas/1 --host 127.0.0.1 --port 31337 --auth-token changeme --ticket "$QUEEN_TICKET"`
- `gpu-bridge-host`:
  - `./bin/gpu-bridge-host --mock --list`
  - Optional NVML: `./bin/gpu-bridge-host --list` (requires `--features nvml`)
- `host-sidecar-bridge`:
  - `./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider nvidia`
  - `./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme` (requires `/host` enabled in `configs/root_task.toml`)
- Deterministic replay via cohsh (no QEMU needed): `./bin/cohsh --transport mock --replay-trace ./traces/trace_v0.trace`

### 6) Regression pack (full-stack, recommended before release)
- `scripts/cohsh/run_regression_batch.sh`
- The batch archives logs under `out/regression-logs/<batch>/<script>.{qemu,out}.log`.
- Verify logs show no unexpected errors or disconnects.

### 7) Release bundle validation (macOS + Ubuntu)
Run Sections 3–5 using the extracted bundle in a clean temp directory (not the repo checkout).
- macOS bundle: `releases/Cohesix-0.1-Alpha-MacOS.tar.gz`
- Ubuntu bundle: `releases/Cohesix-0.1-Alpha-linux.tar.gz`
- Ensure headless Linux uses `xvfb-run` for SwarmUI.

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
- `out/manifests/root_task_resolved.json` — `sha256:ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`

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
- `tests/fixtures/traces/trace_v0.hive.cbor` — `sha256:ba13cc3764e1f4c99fd74735d9023b778359331e89106982fc4c2668ea4105bc`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes above match on-disk fixtures and manifest fingerprints; `scripts/check-generated.sh` invokes it.
