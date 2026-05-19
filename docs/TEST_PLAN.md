<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Mandatory Agent Execution Contract
This section is a mandatory execution contract for all contributors and agents working this repository.

1. Use scripted stages as the source of truth.
- Run `scripts/ci/test_plan_run.sh --list` first.
- Execute stages in order with a shared state dir: `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`.
- Stage scripts are authoritative:
  - `scripts/ci/test_plan_stage_01_integrity.sh`
  - `scripts/ci/test_plan_stage_02_host_fast.sh`
  - `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
  - `scripts/ci/test_plan_stage_04_rest_multiplexer.sh`
  - `scripts/ci/test_plan_stage_05_due_diligence.sh`

2. Defect resolution is mandatory before progression.
- If any stage fails, stop immediately.
- Fix the root cause in code/docs/scripts first; do not bypass by proceeding to a later stage.
- Re-run the failed stage until green, then continue to the next stage.
- Do not mark the run complete until Stage 05 (`scripts/ci/due_diligence_gate.sh`) passes.

3. No silent skips.
- Stage scripts treat skips as **INCOMPLETE**: they write an `incomplete/` record under the shared state dir and the stage exits non-zero.
- A run with any INCOMPLETE marker is **not** a PASS and must not be treated as release-ready.
- Platform-specific **NA** checks (for example, Linux-only mount coverage on macOS) are logged as `NA` and do not block PASS.

4. Keep docs and scripts aligned.
- If execution behavior changes, update this document and the corresponding scripts in the same change.
- `scripts/ci/check_test_plan.sh` must pass before continuing.

## Definition of "Test Plan PASS" (Normative)
A run is **PASS** if and only if:
- It is executed via the staged runner with a shared state dir: `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`.
- Stages **01-05** complete successfully and create `stage_01.done` ... `stage_05.done` in the shared state dir.
- No stage wrote an INCOMPLETE marker (presence of any `stage_*.incomplete` or any files under `out/test-plan/<run-id>/incomplete/` means **FAIL**).
- Stage 05 runs `scripts/ci/due_diligence_gate.sh` and it is green.

Notes:
- Running individual stages (for example `--stage 2`) is for iteration only; it is not a "PASS" run.
- "NA" checks must still be logged, but they do not cause failure; INCOMPLETE always fails.

## Purpose
Validate the full Cohesix stack end-to-end: generated artifacts, QEMU boot, TCP console reliability and performance, deterministic replay, and every shipped host tool.

## Goals
- Pre-existing features continue to work; new features are validated against documented behaviour.
- QEMU boots the VM and exposes Secure9P/TCP console without protocol drift.
- TCP console remains reliable under load (no unexpected disconnects/resets/partial writes).
- Performance baselines are captured and stored under `docs/bench/` (see `docs/BENCHMARKS.md`) for any changes affecting throughput/latency.
- Host tools behave correctly: `coh`, `cohsh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`.
- Deterministic replay passes for cohsh and SwarmUI (trace + hive snapshot).
- Fixtures and manifests remain hash-consistent.

## Scope
- Source tree validation (macOS 26 ARM64 host).
- Release bundle validation on macOS 26 and Ubuntu 24 aarch64.
- Milestone-agnostic: run sections appropriate to the change set.

## Preflight and guardrails
- `scripts/ci/test_plan_run.sh --list` (verify scripted stage inventory before execution)
- `scripts/ci/check_test_plan.sh`
- If IR or manifest changes: `cargo run -p coh-rtc` then `scripts/check-generated.sh`.
- Ensure `SEL4_BUILD_DIR` points at the SMP kernel build (`$REPO/seL4/SMP_build` by default); override to `$REPO/seL4/build` when validating single-core baselines.
- Default QEMU SMP topology is four single-threaded cores; set `COHESIX_QEMU_SMP=1` for single-core baselines or `COHESIX_QEMU_SMP_TOPO` for explicit topologies.
- macOS: FUSE mount coverage is optional unless the MacFUSE runtime is installed and approved (verify `/dev/macfuse0` exists, or `/dev/osxfuse0` on older OSXFUSE).
- If the host lacks EL2/virtualization support or KVM cannot provide GICv2, set `COHESIX_QEMU_VIRT=off` and/or `COHESIX_QEMU_MACHINE_EXTRA=kernel-irqchip=off` when invoking the release `qemu/run.sh`.
- Before any QEMU TCP run, start tcpdump and confirm the log path (example: `logs/tcpdump-new-YYYYMMDD-HHMMSS.log`). Use the same path in TCP correlation checks.
- Headless Linux requires `xvfb-run` (`sudo apt-get install -y xvfb` if missing).
- Ensure `/updates` and `/host` are enabled for host tool tests:
  - `cas.enable = true` (and `ui_providers.updates.*` as needed)
  - `ecosystem.host.enable = true` with providers set
  - Re-run `coh-rtc` and `scripts/check-generated.sh` if toggled.
- Clear old logs if needed: `rm -rf out/regression-logs logs`.

## Performance baselines (Authoritative)
- Performance evidence is only valid when it is **stored and reviewable**:
  - Commit harness artifacts under `docs/bench/` (JSON/CSV/SVG) and
  - Index/interpret them in `docs/BENCHMARKS.md`.
- Do not use "last local run" as a baseline. If you need a new baseline, commit it and update `docs/BENCHMARKS.md` in the same change.

## Execution order
Run in order. Skips produce INCOMPLETE markers and the stage will fail.
- Scripted runner (recommended): `scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>`

### 1) Artifact and fixture integrity
- `scripts/ci/test_plan_stage_01_integrity.sh`
- `scripts/ci/check_test_plan.sh`
- If IR/manifest changed:
  - `scripts/check-generated.sh`

### 2) Host-side unit/integration tests (fast)
- `scripts/ci/test_plan_stage_02_host_fast.sh`
- `cargo test -p coh --features mock`
- `cargo test -p cohesix-rest`
- `cargo test -p gpu-bridge-host`
- `cargo test -p cohsh-core`
- `cargo test -p cohsh --test ticket_mint`
- `cargo test -p cohsh --test transcripts`
- `cargo test -p cohsh --test control_plane`
- `cargo test -p cohsh` (REST transport is enabled by default; use `--no-default-features` to verify minimal builds)
- `cargo check -p swarmui --bin swarmui` (Tauri 2 command/context wiring; SwarmUI keeps the Tauri binary out of Cargo test harnesses)
- `python3 scripts/ci/check_swarmui_dependencies.py` (default REST projection may use `ureq`; `--no-default-features` must not pull HTTP clients)
- `cargo test -p swarmui --test dependency_policy`
- `cargo test -p swarmui --test transcript`
- `cargo test -p swarmui --test console_parity`
- `cargo test -p swarmui --test security`
- `cargo test -p swarmui --test tauri2_config`
- `cargo test -p host-sidecar-bridge`
- `cargo test -p host-ticket-agent`
- `cargo test -p nine-door --test ui_security`
- `cargo test -p nine-door --test session_state`
- `pytest tests/test_pi4_trace_normalize.py`
- `pytest tests/test_pi4_gate_proof.py`
- `cargo test -p nine-door --test pressure_counters`
- `cargo test -p nine-door --test schedule_create`
- `cargo test -p nine-door --test schedule_bounds`
- `cargo test -p nine-door --test lease_bounds`
- `cargo test -p nine-door --test policy_ctl`
- `cargo test -p nine-door --test export_ctl`
- `cargo test -p nine-door --test telemetry_create`
- `cargo test -p nine-door --test telemetry_quotas`
- `cargo test -p nine-door --test telemetry_envelope`
- `cargo test -p nine-door --test integration`
- `cargo test -p cohsh-core --test trace`
- `cargo test -p cohsh --test trace`
- `cargo test -p swarmui --test trace`
- `cargo run -p coh --features mock -- doctor --mock`
- `cargo test -p hive-gateway`
- `cargo test -p tests`
- `python3 scripts/ci/check_driver_test_coverage.py`
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139`
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio`
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci`
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio`
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::flush_tx_backpressure_does_not_count_as_budget_overrun`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_defers_buffered_network_console_lines_for_driver_task_turn`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::bcmgenet`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::cyw43`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::bcmgenet`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::nettest_reports_wifi_host_eapol_pending_detail`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::netstats_emits_compact_status_line`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::`
- `cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance`
- `SEL4_BUILD_DIR=$REPO/seL4/SMP_build cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu`
- `SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4`
- `CARGO_INCREMENTAL=0 cargo test --workspace`
- If `pytest` is not available in the host `python3`, `scripts/ci/test_plan_stage_02_host_fast.sh` auto-creates `${TEST_PLAN_STATE_DIR}/.venv` and installs `pytest` there.
- `python3 -m pytest tools/cohesix-py/tests`
- `python3 tools/cohesix-py/examples/lease_run.py --mock`
- `python3 tools/cohesix-py/examples/peft_roundtrip.py --mock`
- `python3 tools/cohesix-py/examples/telemetry_write_pull.py --mock`
- `python3 tools/cohesix-py/examples/use_case_playbook.py --playbook mixed-closed-loop-ai-factory --dry-run --mock --no-proc-snapshot --no-host-snapshot --no-push-host-snapshot --out out/test-plan/python-playbooks`
- Fixture regen (only when needed):
  - `COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace`
  - `COHESIX_WRITE_TRACE=1 cargo test -p swarmui --test trace`
- Explicit scale proof (not part of the fast stage):
  - `cargo test -p nine-door --features scale-tests --test shard_scale sharded_attach_1k_scale_gate_exports_metrics -- --nocapture`

Pi 4 trace evidence remains a post-capture host workflow. `scripts/pi4-image-build.sh` stages USB/Wi-Fi trace helpers, but fast host tests invoke `scripts/pi4_trace_normalize.py` and `scripts/pi4_gate_proof.sh` tests directly and do not require a flashed SD card or serial log. The same normalizer also provides `--gate-summary` plus repeated `--expect KEY=VALUE` checks for narrow USB/Wi-Fi hardware runs, so a serial capture can fail fast on regressions such as `USB_BLOCKER=cmd-submit-proof-timer-preempted`, `USB_BLOCKER=usbcmd-run-preserved-reset-bit`, `WIFI_BLOCKER=armcr4-prereset-fgc-cmd53-r5-rejected`, `WIFI_BLOCKER=ht-clock-timeout`, `BOOT_HALTED=yes`, `PANIC_SEEN=yes`, `PANIC_REASON=bootinfo-snapshot-corrupted`, or `TIMER_IRQ27_SEEN=yes`. The Pi 4 local-seat driver coverage module is part of Stage 02 because it owns USB keyboard input proof contracts, Caps/Num/Scroll LED bitmaps, post-seal LED-sync enablement, HDMI progress refresh cadence, and Wi-Fi progress suppression while USB boot activity is active.

Reopened Milestones 26a/26b also require HAL driver-task contract coverage before hardware claims: `hal::driver_task` must validate the serial, USB/local-seat, HDMI, GENET, CYW43, SDIO host, PCIe root, RTL8139, and virtio-net contracts. Historical M26B completion evidence remains a compatibility baseline, not reopened acceptance proof. Reopened Pi 4 captures must include compact `DRIVER_TASK_*`, `SCHED_CONTRACT`, `BUDGET_OVERRUN`, observed per-driver latency, `SERIAL_ECHO`, `USB_BURST`, and `HDMI_RESPONSIVE` evidence; `scripts/pi4_trace_normalize.py --gate-summary` now exposes those as machine-checkable hardware proof fields. Dedicated-driver-task closure is stricter than contract declaration: `DRIVER_TASK_DEDICATED` must cover the required active roles, `DRIVER_TASK_COMPATIBILITY` must be `0`, `DRIVER_TASK_DEDICATED_READY=yes` must be present, serial, USB/local-seat, display, and network role booleans must all be `yes`, and substrate/capset/fault/revoke/scheduling proof fields must all be `yes` when `scripts/pi4_gate_proof.sh --require-driver-task-proof` is used.

### 3) QEMU boot + TCP console baseline
- `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
- Stage 03 sets resilient defaults for clean hosts: `TP_STAGE3_READY_TIMEOUT=900`, `TP_STAGE3_PORT_TIMEOUT=60`, `TP_STAGE3_AUTH_READY_TIMEOUT=120`, `TP_STAGE3_QUIT_CLOSE_TIMEOUT=60` (override as needed).
- `scripts/cohsh/run_regression_batch.sh` keeps Cargo build cache by default; set `COHSH_BATCH_CLEAN_TARGET=1` only for deliberate clean-rebuild validation.
- `scripts/cohsh/run_regression_batch.sh` (invoked by stage 03; can be run directly for bring-up)
- Stage 03 archives per-script logs under the stage state dir (for example `out/test-plan/<run-id>/qemu-regression-logs/`).
- Manual runs of `scripts/cohsh/run_regression_batch.sh` default to `out/regression-logs/` unless `COHSH_LOG_ROOT` is set.
Start QEMU (source tree or bundle), then verify:
- Capture QEMU serial to `logs/qemu-console.log` (example: `./qemu/run.sh | tee logs/qemu-console.log`).
- `cohsh` (queen): `help`, `attach queen` (skip if you launched cohsh with `--role`),
  `log`, `tail /log/queen.log`, `ls /`, `cat /log/queen.log`,
  `test --mode quick`, `test --mode full`, `test --mode smp` (fresh boot),
  `spawn heartbeat ticks=100`, `ls /worker`, `kill worker-<id>`, `ping`,
  `tcp-diag`, `quit`
  - If policy gating is enabled (see `/policy/rules`), enqueue approvals before `spawn` and `kill`:
    - `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
    - `echo {"id":"kill-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
- Capture cohsh output to `logs/cohsh-session.log` (example: `... | tee logs/cohsh-session.log`).
- Success criteria:
  - No unexpected `ERR` lines or reconnect loops.
  - ACK/ERR/END ordering stable.

### 4) TCP reliability & performance (QEMU)
Stage 04 is self-contained for local QEMU. If no `COHESIX_GATEWAY_URL`
(`HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL`) is supplied, the stage
boots a local QEMU instance, starts `hive-gateway` against that TCP console, and
uses a stage-local request-auth token. Local mode allocates free loopback ports
by default; override the local bind/port with `TP_STAGE4_GATEWAY_BIND` and
`TP_STAGE4_QEMU_TCP_PORT`. Supplying an explicit gateway URL keeps the
external-gateway path and requires
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` (`COHSH_REST_AUTH_TOKEN` or
`COH_REST_AUTH_TOKEN`).

Run while QEMU is up:
- Repeat `tcp-diag` 5–10 times and record results (example: `... | tee logs/tcp-diag.log`).
- Run `pool bench path=/log/queen.log ops=500 batch=8 payload_bytes=64` and record throughput/latency (example: `... | tee logs/pool-bench.log`).
- Reasonable acceptance:
  - `tcp-diag` has zero failures.
  - `pool bench` shows non-zero throughput and stable latency.
  - Any performance regression claim must be backed by committed baseline artifacts under `docs/bench/` (and indexed in `docs/BENCHMARKS.md` when applicable); do not compare against unpublished local runs.
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
- Control grammar sanity (requires `control_plane.*` + `/proc` observability enabled):
  - `echo {"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120} > /queen/schedule/ctl`
  - `cat /proc/schedule/summary` and `cat /proc/schedule/queue`
  - `echo {"op":"grant","id":"lease-1","subject":"queen","resource":"gpu0","ttl_s":300,"priority":5} > /queen/lease/ctl`
  - `echo {"op":"preempt","id":"lease-1","reason":"timeout"} > /queen/lease/ctl`
  - `cat /proc/lease/summary`, `cat /proc/lease/active`, `cat /proc/lease/preemptions`
  - `echo {"op":"open","id":"export-1","ttl_s":900} > /queen/export/ctl`
  - `echo {"op":"close","id":"export-1","reason":"window-complete"} > /queen/export/ctl`
  - `echo {"op":"apply","id":"rev-2026-02-03","sha256":"<64-hex>"} > /policy/ctl`
  - `echo {"op":"rollback","id":"rev-2026-02-03"} > /policy/ctl`
- `coh` (TCP console; requires `out/coh_policy.toml`):
  - `./bin/coh gpu list --host 127.0.0.1 --port 31337`
  - `./bin/coh gpu lease --host 127.0.0.1 --port 31337 --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60`
  - `./bin/coh run --host 127.0.0.1 --port 31337 --gpu GPU-0 -- echo ok`
  - `./bin/coh gpu status --host 127.0.0.1 --port 31337 --gpu GPU-0`
  - `./bin/coh telemetry pull --host 127.0.0.1 --port 31337 --out ./out/telemetry`
  - Live PEFT flow (requires live GPU bridge publish):
    - Preflight: `./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen -c "ls /queen/export/lora_jobs"`
      - If `/queen/export/lora_jobs` is missing in dev-virt, **skip live PEFT** and rely on the mock PEFT tests above (this indicates no export job was seeded in the VM).
    - `./bin/coh --host 127.0.0.1 --port 31337 peft export --job job_0001 --out ./out/peft_export`
    - `./bin/coh --host 127.0.0.1 --port 31337 peft import --publish --model demo-model --from demo/peft_adapter --job job_0001 --export ./out/peft_export --registry ./out/peft_registry`
    - `./bin/coh --host 127.0.0.1 --port 31337 peft activate --model demo-model --registry ./out/peft_registry`
    - Verify in `cohsh` (after closing SwarmUI): `ls /gpu/models/available` and `cat /gpu/models/active`
  - Optional FUSE: `./bin/coh mount --host 127.0.0.1 --port 31337 --at /tmp/coh-mount` (requires a FUSE runtime; on macOS this means MacFUSE installed and approved, typically `/dev/macfuse0`).
- `swarmui` live (console + observability; do not attach cohsh simultaneously):
  - macOS: `./bin/swarmui`
  - headless Linux: `xvfb-run -a ./bin/swarmui`
  - Live telemetry (required for milestone-flagged UI changes):
    - Preconditions: Queen session reachable, workers emitting telemetry, and no parallel `cohsh` session attached.
    - In SwarmUI: set role `queen` + ticket, click **Connect**, then **Hive Start**.
    - Click a worker dot; confirm the detail panel updates within 1–2 poll intervals.
    - Click a telemetry overlay card; confirm the dot selection updates and the detail panel matches the card agent.
    - Confirm overlays show recent lines (no empty state) for at least one worker.
    - Record the result in `logs/host-tool-runs.md` with timestamps.
  - SwarmUI console exposes the core console verbs; CLI-only commands remain in `cohsh`.
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
  - Optional NVML: `./bin/gpu-bridge-host --list` (enabled by default on Linux builds; omit NVML with `--no-default-features`)
  - Live publish: `./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --interval-ms 1000 --registry demo/peft_registry`
    - On macOS without NVML, use `--mock --publish` to avoid NVML load failures.
- `host-sidecar-bridge`:
  - `./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider docker --provider nvidia`
  - `./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --watch` (requires `/host` enabled in `configs/root_task.toml`)
- `host-ticket-agent`:
  - `./bin/host-ticket-agent --mock --run-once`
  - `./bin/host-ticket-agent --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --run-once`
  - REST mode (gateway required): `./bin/host-ticket-agent --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --run-once`

### 5a) Mandatory control-ticket matrix (Milestone 25g)
All runs are required unless explicitly marked `NA` by platform constraints.
- Ticket namespace and bounds:
  - Append valid ticket JSONL to `/host/tickets/spec`; verify success.
  - Append malformed or over-bound lines; verify deterministic `ERR`.
- GPU/PEFT ticket flow:
  - Submit `gpu.lease.grant` and `gpu.lease.release` tickets (or `NA` if no GPU surface).
  - Submit `peft.import`, `peft.activate`, and `peft.rollback` tickets against a test registry.
  - Verify lifecycle receipts under `/host/tickets/status|deadletter`.
- systemd/docker remediation flow:
  - Submit `systemd.restart` and `docker.restart` tickets for allowlisted targets.
  - Verify receipts are deterministic and bounded.
  - Verify non-allowlisted actions are rejected before execution.
- K8s coexistence translation flow:
  - Use Python translation helpers (`K8sRbacIntent` -> `/host/tickets/spec`) to submit `k8s.cordon`, `k8s.drain`, and `k8s.lease.sync`.
  - Verify no scheduler-replacement semantics are introduced (coexistence only).
- Evidence/timeline replay flow:
  - `./bin/coh evidence pack --host 127.0.0.1 --port 31337 --out ./out/evidence/tickets --with-telemetry`
  - `./bin/coh evidence timeline --in ./out/evidence/tickets`
  - Verify `timeline.ndjson` contains host ticket correlation keys (`id + idempotency_key`).
- `hive-gateway` (REST gateway, Linux/systemd required for this section):
  - Install unit + env file (examples):
    - `sudo cp resources/systemd/hive-gateway.service /etc/systemd/system/`
    - `sudo tee /etc/cohesix/hive-gateway.env >/dev/null <<'EOF'`
    - `COH_TCP_HOST=127.0.0.1`
    - `COH_TCP_PORT=31337`
    - `COH_AUTH_TOKEN=changeme`
    - `COH_ROLE=queen`
    - `COH_TICKET=`
    - `HIVE_GATEWAY_BIND=127.0.0.1:8080`
    - `EOF`
  - `sudo systemctl daemon-reload`
  - `sudo systemctl enable --now hive-gateway`
  - Validate REST responds: `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
  - Restart QEMU and confirm auto-reconnect:
    - stop QEMU, wait 5–10s, restart QEMU
    - `journalctl -u hive-gateway -n 200 --no-pager | rg -n "reconnect|connected|disconnected"`
    - Re-run: `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
  - `sudo systemctl stop hive-gateway`
- Multiplexer regression (REST gateway, QEMU running; `hive-gateway` is the sole console client):
  - REST API smoke (manifest + namespace + log tail):
    - `curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lifecycle/state&max_bytes=64' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/tail?path=/log/queen.log&max_bytes=512' | jq .`
  - REST `/proc` bounds (schedule + lease):
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/summary&max_bytes=128' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/queue&max_bytes=256' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/summary&max_bytes=128' | jq .`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/active&max_bytes=256' | jq .`
  - Policy approval (only if `/policy/rules` exists):
    - `curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" -H 'Content-Type: application/json' -d '{"path":"/actions/queue","line":"{\"id\":\"approve-rest-1\",\"target\":\"/queen/ctl\",\"decision\":\"approve\"}"}'`
  - REST spawn (heartbeat) through the gateway:
    - `curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" -H 'Content-Type: application/json' -d '{"path":"/queen/ctl","line":"{\"spawn\":\"heartbeat\",\"ticks\":120,\"budget\":{\"ttl_s\":300,\"ops\":500}}"}'`
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/worker' | jq .`
  - Host publishers over REST:
    - `./bin/gpu-bridge-host --publish --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --interval-ms 1000`
    - `./bin/host-sidecar-bridge --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --watch --provider systemd --provider nvidia`
    - Validate: `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/gpu' | jq .` and `curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/host' | jq .`
  - `coh` REST path coverage (queen role via gateway):
    - `./bin/coh gpu --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" list`
    - `./bin/coh gpu --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" lease --gpu GPU-0 --mem-mb 2048 --streams 1 --ttl-s 120`
    - `./bin/coh run --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --gpu GPU-0 -- echo ok`
    - `./bin/coh telemetry --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" pull --out ./out/telemetry-rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" export --job job_0001 --out ./out/peft_export_rest` (skip if no export job is seeded)
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" import --publish --model demo-model --from demo/peft_adapter --job job_0001 --export ./out/peft_export_rest --registry ./out/peft_registry_rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" activate --model demo-model --registry ./out/peft_registry_rest`
    - `./bin/coh peft --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" rollback --registry ./out/peft_registry_rest`
  - REST mount exclusivity:
    - `./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --at /tmp/coh-mount-rest`
    - In a second shell: `./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --at /tmp/coh-mount-rest-2` → must fail with an exclusive lock error.
    - Read/write smoke (supported MIME types only):
      - `cat /tmp/coh-mount-rest/proc/lifecycle/state` (must be non-empty)
      - `head -n 5 /tmp/coh-mount-rest/log/queen.log` (must be non-empty)
      - `DEV=tp-mount-xfer-1; printf '{"new":"segment","mime":"text/plain"}\n' >> "/tmp/coh-mount-rest/queen/telemetry/${DEV}/ctl"`
      - `printf 'hello-from-test-plan ts_ms=%s\n' "$(date +%s000)" >> "/tmp/coh-mount-rest/queen/telemetry/${DEV}/seg/seg-000001"`
      - `cat "/tmp/coh-mount-rest/queen/telemetry/${DEV}/latest"` (expects `seg-000001`)
  - `cas-tool` REST upload:
    - `./bin/cas-tool pack --epoch 1 --input ./out/cas/trace_v0.padded --out-dir ./out/cas/1 --signing-key ./resources/fixtures/cas_signing_key.hex`
    - `./bin/cas-tool upload --bundle ./out/cas/1 --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"`
  - `cohsh` REST CLI:
    - `./bin/cohsh --transport rest --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN"`
    - `attach queen`
    - `cat /proc/schedule/summary`
    - `cat /proc/lease/summary`
  - SwarmUI via gateway (REST transport enabled by default):
    - `SWARMUI_TRANSPORT=rest SWARMUI_REST_URL=http://127.0.0.1:8080 ./bin/swarmui`
    - Confirm Live Hive view renders telemetry and the console panel accepts standard verbs.
    - Open DevTools and run `window.__SWARMUI_HIVE_DEBUG.getMetrics()`; confirm renders advance and the UI stays responsive.
- Deterministic replay via cohsh (no QEMU needed):
  - Source tree: `./bin/cohsh --transport mock --replay-trace ./tests/fixtures/traces/trace_v0.trace`
  - Release bundle: `./bin/cohsh --transport mock --replay-trace ./traces/trace_v0.trace`

### UI Presentation Layer — SwarmUI (Playwright)

**Additive UI-only layer.** Playwright tests **DO NOT** assert control-plane correctness and **MUST NOT** introduce new verbs, protocols, or semantics. They validate presentation (rendering, wiring, and transcript parity) using deterministic replay fixtures.

#### 1) Scope
- Covers: SwarmUI launch, Spectrum shell control wiring, canvas presence, deterministic replay rendering, mint-ticket UI wiring, and embedded `>coh` console transcript output.
- Excludes: control-plane logic, NineDoor semantics, ticket validation correctness, and any non-UI behavior already covered by `.coh` scripts or regression pack.

#### 2) Modes
- **Replay mode (required, gating):** UI is driven from trace/snapshot fixtures and deterministic transcript outputs.
- **Live mode (optional, smoke only):** non-gating checks for basic launch and visibility; no protocol assertions.

#### 3) Test Categories
- Launch + smoke (UI loads and renders key panels).
- Replay visual regression (banner/shell screenshot baseline).
- Spectrum shell controls (buttons, text fields, pickers mount and remain wired to existing IDs/flows).
- Interactive `>coh` prompt (type commands, assert transcript lines).
- Mint ticket flow (UI-only assertion that the host-returned token is surfaced back into the session field).
- Live Hive UX (labels, role colors, and dot selection wiring).
- Live Hive performance harness (bounded render cadence and backlog checks).
- Failure UI (auth error, disconnected state) as UI-only states.

#### 4) Determinism Rules
- Replay-first: all UI assertions are driven from replay fixtures.
- Avoid unbounded timing-based assertions; use explicit replay fixtures and the Live Hive metrics harness for bounded render/backlog checks.
- Transcript-based assertions only (match `OK`, `ERR`, `END` and static help lines).
- Shell assertions must preserve the existing SwarmUI IDs and Tauri invoke contract even when the underlying controls are Spectrum Web Components.

#### 5) CI Positioning
- Runs **after** `.coh` scripts and the regression pack.
- **Blocking (mandatory):** replay-mode UI tests (snapshot + transcript parity + Live Hive UX + performance harness).
- **Warn-only:** live-mode smoke checks.

**Playwright commands (macOS ARM64):**
- `cd tools/swarmui-ui-tests`
- `npm ci`
- `npx playwright install webkit chromium`
- Source UI (default harness target): `npm test`
- Explicit source UI override: `SWARMUI_UI_ROOT=../../apps/swarmui/frontend npm test`
- Release bundle verification: `SWARMUI_RELEASE_DIR=../releases/<latest> npm test`
- Update snapshots only when UI changes are intended: `npm run test:update`

**Notes**
- The Playwright harness targets the **source SwarmUI frontend** by default so UI regressions are measured against the current shell, not a stale release bundle.
- Set `SWARMUI_RELEASE_DIR` to verify a packaged release bundle; set `SWARMUI_UI_ROOT` only when overriding the default source path.
- The harness injects a deterministic Tauri `invoke` mock for UI-only replay and transcript assertions; it does not exercise control-plane behavior.
- The current shell uses a vendored Spectrum Web Components layer for operator controls; Playwright interacts with the effective editable/button controls while keeping the canonical SwarmUI element IDs stable.
- The Live Hive performance harness reads `window.__SWARMUI_HIVE_DEBUG.getMetrics()`; `pending` must stay ≤ `swarmui.hive.pending_event_cap` and `renders` should advance without UI stalls under replay fixtures.
- Browser binaries are installed into the user Playwright cache (not committed).
- Snapshot coverage runs against the current browser matrix: `webkit-desktop` (baseline shell), `webkit-narrow` (responsive shell and scheduler), and `chromium-tablet` (interaction parity without snapshot gating).

### 6) Regression pack (full-stack, recommended before release)
- `scripts/ci/test_plan_stage_04_rest_multiplexer.sh` (self-contained local QEMU by default; set `COHESIX_GATEWAY_URL` or equivalent to target an already running gateway)
- `COHESIX_GATEWAY_URL=http://<gateway-host>:<port> HIVE_GATEWAY_REQUEST_AUTH_TOKEN=<token> scripts/cohsh/REST_regression_batch.sh`
- Stage 04 runs two REST batches:
  - A concurrent "core" batch (boot/proc/pool coverage): `scripts/cohsh/boot_v0.coh`, `scripts/cohsh/observe_watch.coh`, `scripts/cohsh/session_pool.coh`.
  - A strict "parity" batch (control-plane smoke): `scripts/cohsh/rest_control_plane_smoke.coh`.
    - Note: `scripts/cohsh/busy_backpressure.coh` and `scripts/cohsh/policy_gate.coh` remain covered by the TCP/QEMU regression matrix (Stage 03), where console-parser semantics are validated directly.
- Stage 04 also runs a Python REST smoke (`tools/cohesix-py` `RestBackend`) that performs `LS /` and reads `/proc/lifecycle/state` against the same gateway.
- Logs:
  - Scripted Stage 04 writes REST batch logs under the stage state dir (for example `out/test-plan/<run-id>/rest-regression-logs/`).
  - Manual runs of `scripts/cohsh/REST_regression_batch.sh` default to `out/regression-logs/<batch>/<script>.run*.log` unless `COHSH_LOG_ROOT` is set.
- Verify logs show no unexpected errors or disconnects.
- From Milestone 25 onward, use the REST batch above; the TCP/QEMU batch remains a local bring-up tool only.

### 6a) SMP parity (Milestone 25+)
- Boot QEMU with a single core: `COHESIX_QEMU_SMP=1 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_1.txt`
- Reboot QEMU with multiple cores (match the SMP kernel build): `COHESIX_QEMU_SMP=4 scripts/cohesix-build-run.sh --transport tcp`
- Run `./cohsh --transport tcp --tcp-port 31337 --script scripts/cohsh/smp_parity.coh > out/smp_parity_4.txt`
- Compare transcripts: `diff -u out/smp_parity_1.txt out/smp_parity_4.txt` (must be byte-identical).

### 6b) Gateway large-telemetry reliability gate (Milestone 25f, mandatory)
Run this matrix with `hive-gateway` attached and **no retry paths**. These runs are required both locally and on the G5g host.
- `python3 scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01`

Pass criteria:
- Every run exits `0`.
- Summary artifacts exist (`*.summary.json`, `*.ops.csv`, `*.ramp.csv`, `*.ramp.svg`).
- `error_budget_pass=true` and `error_rate <= 0.01` in each summary JSON.
- `no_retries=true`, `fast_ramp=true`, and `scenario` equals the requested preset in each summary JSON.

Failure policy:
- Any scenario above the error budget is a release-blocking defect.
- Do not use retry flags or ad-hoc rerun wrappers to mask failures; tune/fix code and re-run the same matrix.

### 6c) Multi-hive federation relay gate (Milestone 25h, mandatory)
Run this matrix with three independent hives (`hive-a`, `hive-b`, `hive-c`) and one `host-ticket-agent --relay` per hive.

Required checks:
- Relay success path:
  - Append one federated spec line from `hive-a` to target `hive-b` via REST `/v1/fs/echo` (`source_hive`, `target_hive`, `relay_hop`, `relay_correlation_id` populated).
  - Verify target hive receives one request and one terminal receipt (no duplicates).
- Relay dedupe path:
  - Re-append the same spec line (`id`, `idempotency_key`, `source_hive`, `target_hive` unchanged).
  - Verify no duplicate side effects; relay counters show dedupe increment.
- Relay failure + WAL resume:
  - Stop target gateway temporarily; submit federated tickets from source.
  - Verify source relay queue/WAL grows deterministically and `relay_remote_write_failures` increments.
  - Restore target gateway; verify pending WAL entries drain exactly once.
- Failover pause/resume integration:
  - Run `python3 scripts/failover_watchdog.py --help` and execute watchdog with `--relay-pause-cmd` and `--relay-resume-cmd`.
  - Planned and unplanned cutover paths must pause relay before cutover and resume only after standby health checks pass.
- Evidence/timeline correlation:
  - `./bin/coh evidence pack --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" --out ./out/evidence/federation --with-telemetry`
  - `./bin/coh evidence timeline --in ./out/evidence/federation`
  - Verify timeline rows include federated fields (`source_hive`, `target_hive`, `relay_hop`) and stable correlation IDs.
- Multi-hive scale gate:
  - `python3 scripts/rest_perf_harness.py simulate --multi-hive --hives 3 --workers-per-hive 1000 --no-retries --error-budget-rate 0.01`
  - Summary JSON must report `multi_hive=true`, `hives=3`, `workers_per_hive=1000`, and pass error budget.

Pass criteria:
- No split-brain writes: mutation authority remains single-writer per hive.
- Relay retries are deterministic and idempotent across restarts.
- No ACK/ERR/END grammar drift versus existing fixtures.
- Any failed mandatory federation check is release-blocking.

### 6d) UEFI no-NIC + attestation baseline (Milestone 26)
Run this matrix in addition to the staged runner when Milestone 26 files change.

- Manifest + schema gate:
  - `cargo run -p coh-rtc -- configs/root_task_uefi_aarch64.toml --out out/uefi/generated --manifest out/uefi/root_task_resolved_uefi.json --cas-manifest-template out/uefi/cas_manifest_template_uefi.json --cli-script out/uefi/boot_v0_uefi.coh --doc-snippet out/uefi/root_task_manifest_uefi.md --gpu-breadcrumbs-snippet out/uefi/gpu_breadcrumbs_uefi.md --observability-interfaces-snippet out/uefi/observability_interfaces_uefi.md --observability-security-snippet out/uefi/observability_security_uefi.md --ticket-quotas-snippet out/uefi/ticket_quotas_uefi.md --trace-policy-snippet out/uefi/trace_policy_uefi.md --cas-interfaces-snippet out/uefi/cas_interfaces_uefi.md --cas-security-snippet out/uefi/cas_security_uefi.md --cohesix-py-defaults out/uefi/cohesix_py_defaults_uefi.py --cohesix-py-doc out/uefi/cohesix_py_defaults_uefi.md --coh-doctor-doc out/uefi/coh_doctor_checks_uefi.md --cohsh-policy out/uefi/cohsh_policy_uefi.toml --cohsh-policy-rust out/uefi/cohsh_policy_uefi.rs --cohsh-policy-doc out/uefi/cohsh_policy_uefi.md --cohsh-client-rust out/uefi/cohsh_client_uefi.rs --cohsh-client-doc out/uefi/cohsh_client_uefi.md --cohsh-grammar-doc out/uefi/cohsh_grammar_uefi.md --cohsh-ticket-policy-doc out/uefi/cohsh_ticket_policy_uefi.md --coh-policy out/uefi/coh_policy_uefi.toml --coh-policy-rust out/uefi/coh_policy_uefi.rs --coh-policy-doc out/uefi/coh_policy_uefi.md --swarmui-defaults out/uefi/swarmui_defaults_uefi.toml --swarmui-defaults-rust out/uefi/swarmui_defaults_uefi.rs --swarmui-defaults-doc out/uefi/swarmui_defaults_uefi.md`
- UEFI ESP packaging gate:
  - `scripts/uefi/esp-build.sh --manifest out/uefi/root_task_resolved_uefi.json --out-dir out/uefi/m26`
  - Verify `out/uefi/m26/esp.sha256` and `out/uefi/m26/esp-meta.json` exist and are deterministic between reruns.
- UEFI QEMU boot gate:
  - `scripts/uefi/qemu-uefi.sh --esp-dir out/uefi/m26/esp --console serial`
  - Confirm boot output includes:
    - `manifest.hw.no_nic=true`
    - `manifest.hw.networking=disabled-m26-baseline`
    - `attestation.bound_manifest_sha256=<manifest hash>`
    - `attestation.evidence_sha256=<64-hex>`
- Runtime boundary gate:
  - `rg -n "EFI_|boot_services|runtime_services|uefi::" apps/root-task/src apps/nine-door/src tools/coh-rtc/src`
  - The runtime code path must not introduce direct EFI service calls after seL4 handoff.

### 6e) Pi 4 DHCP + U-Boot policy compatibility and reopened driver-task proof (Milestones 26a/26b)
Run this matrix in addition to the staged runner when Milestone 26a or 26b files change. Older checked-in M26B Wi-Fi/DHCP captures prove the retained compatibility baseline only; reopened 26a/26b closure additionally requires fresh USB/serial/HDMI responsiveness evidence under wired and Wi-Fi load plus the driver-task scheduling fields below.

- Compiler + docs gate:
  - `cargo test -p coh-rtc`
  - `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest out/manifests/root_task_resolved.json`
- Host DHCP/policy gate:
  - `cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture`
  - Confirms the bounded DHCP core plus runtime policy override plumbing without changing QEMU grammar.
- Pi 4 image / U-Boot gate:
  - `scripts/pi4-image-build.sh --manifest out/manifests/root_task_resolved.json`
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
  - `scripts/uboot/qemu-uboot-smoke.sh --net user`
  - Confirm U-Boot env control remains deterministic (`ipaddr`, `serverip`, `coh_net_mode`, `coh_net_interface`), `CONFIG_PREBOOT` stays on the serial/video console path, the staged Pi 4 boot script owns the first menu/input USB bootstrap, reloads `cohesix.env`, mirrors `coh_net_*` values into the staged padded `bcm2711-rpi-4-b.dtb`, and boots the seL4 elfloader through U-Boot `bootm` with that DTB.
- QEMU compatibility gate:
  - `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none`
  - Existing QEMU hostfwd defaults (`127.0.0.1:{31337,31338,31339}`) and ACK/ERR/END fixtures must remain unchanged.
- Pi 4 runtime evidence gate:
  - Build-only/stage-only validation is useful but is not Pi 4 acceptance. A reopened 26a/26b hardware run must include a fresh serial capture from the reflashed image, not an older checked-in or operator-provided transcript.
  - The minimum 26a wired/GENET closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-usb-ready --require-wired-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=genet --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - The minimum 26b Wi-Fi closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - Existing logs may be normalized for triage only:
    - `scripts/pi4_gate_proof.sh --normalize-only --log <existing-log> --allow-summary-only`
    - `--allow-summary-only` is not acceptance proof and must not be combined with any `--require-*` hardware acceptance flag.
  - When `cohsh` reaches the Pi over Wi-Fi/TCP, keep the raw serial log and the `cohsh` transcript together in the Pi 4 evidence directory. TCP `cohsh` output is not mirrored back into the UART log, so the normalizer may be run over a combined serial-plus-`cohsh` evidence file for the final `netstats`/`netstatus` assertions while retaining the raw serial log as the boot source of truth.
  - Capture boot evidence showing:
    - `manifest.hw.network.mode=<static|dhcp>`
    - `manifest.hw.network.interface=<wired|wifi|auto>`
    - `[net-policy] source=<manifest|dtb> ...` or `[net-policy] source=dtb rejected reason=<reason> ...`
    - explicit `wifi` boots may now emit `[net-console] pending-link backend=<driver> active=<iface> detail=wifi-associating ...` before later association / DHCP progress
    - when saved Cohesix policy exists, the U-Boot wizard defaults to `Continue with existing config`; otherwise it defaults to `Boot with manifest defaults`
    - for static boots sourced from the U-Boot wizard, `/chosen/cohesix,static-ipv4`, `/chosen/cohesix,static-prefix-len`, and optional `/chosen/cohesix,static-gateway` appear in the U-Boot handoff log
    - for DHCP boots, `[net-console] pending-dhcp ...` followed by `[dhcp] lease bound ...`
    - USB cold-boot proof shows `USB_BOOTLOADER_HANDOFF_SEEN=no` and `USB_COLD_BOOT_SEEN=yes`; any U-Boot xHCI handoff, stop-seed, preserve-state, bootloader-authorized reset, or `run-uboot` label fails the Pi 4 USB gate.
    - USB keyboard proof reaches `USB_GATE=10` / `USB_BLOCKER=none` with first-byte proof, and hardware acceptance captures a printable-key line such as `runtime keyboard first-printable-byte ...` before claiming the local-seat keyboard experience is complete.
    - if the attached keyboard exposes lock LEDs, Caps Lock, Num Lock, and Scroll Lock testing either proves the preallocated EP0 OUT DMA path (`xhci-control-out-prealloc` plus `pi4 keyboard led sync ready ...`) or cleanly logs `keyboard led sync unavailable ... action=disabled` without blocking input.
    - HDMI local-seat acceptance observes typed USB keyboard bytes echoing at parser ingress and boot/progress messages refreshing at the documented 5-10 s cadence, with Stage 02 driver coverage guarding the cadence constants and Wi-Fi progress suppression during USB boot activity and after USB first-byte proof.
  - `netstats` must report:
    - `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`
    - `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1>`
    - driver-task scheduling evidence for the active hardware path in reopened 26a/26b acceptance captures: contract name, service class, isolation mode, poll/service count, budget exhaustion/yield count, RX/TX queue depth, drop count, and observed service latency. The normalizer exposes this as `DRIVER_TASK_CONTRACTS`, `DRIVER_TASK_DEDICATED`, `DRIVER_TASK_COMPATIBILITY`, `DRIVER_TASK_DEDICATED_READY`, `DRIVER_TASK_SERIAL_DEDICATED`, `DRIVER_TASK_USB_DEDICATED`, `DRIVER_TASK_DISPLAY_DEDICATED`, `DRIVER_TASK_NET_DEDICATED`, `DRIVER_TASK_SUBSTRATE_READY`, `DRIVER_TASK_CAPSET_PROOF`, `DRIVER_TASK_FAULT_PROOF`, `DRIVER_TASK_REVOKE_PROOF`, `DRIVER_TASK_SCHED_PROOF`, `DRIVER_TASK_ACTIVE_NET`, `DRIVER_TASK_BUDGET_OVERRUNS`, and `DRIVER_TASK_LATENCY_PROOFS`. Contract-only root-task compatibility evidence and declared `max_service_us` budgets are diagnostic and must not be counted as dedicated driver-task closure or latency proof.
    - responsiveness evidence under network load: `SERIAL_RESPONSIVE_PROOF=yes`, `USB_BURST_PROOF=yes`, `USB_BURST_DROPS=0`, and `HDMI_RESPONSIVE_PROOF=yes`.
    - wired 26a closure must show `NET_ACTIVE=wired`; Wi-Fi 26b closure still requires `active=wifi`, `addr_src=dhcp-lease`, `dhcp=bound`, `eapol_secure=1`, and non-zero TX/RX packet counters.
    - `netstatus: ip=<ipv4> gateway=<ipv4> src=<source> dhcp=<phase>`
  - `nettest` refusal detail must preserve the reason when the run cannot start:
    - `detail=dhcp-pending`
    - `detail=wifi-associating`, `detail=wifi-host-eapol-pending`, `detail=wifi-host-eapol-required`, `detail=wifi-association-failed`, or `detail=wifi-link-down`
    - `detail=not-ready:<root-ep|ipc-buffer|cspace-window|bootstrap-commit>`
    - `detail=policy-disabled` or `detail=selftest-disabled` when the profile/runtime disables self-test
  - explicit `wifi` now supports both `static` and `dhcp` through the HAL-backed CYW43455 path; `auto` remains DHCP-only with wired fallback limited to CYW43455 attach/join setup failure before DHCP ownership transfers to the active Wi-Fi stack, and final 26b completion still requires Pi 4 hardware captures proving join + DHCP and that attach/join fallback behavior.

### 7) Release bundle validation (macOS + Ubuntu)
Run Sections 3–5 using the extracted bundle in a clean temp directory (not the repo checkout).
- macOS bundle: `releases/Cohesix-0.9.0-beta-MacOS.tar.gz`
- Ubuntu bundle: `releases/Cohesix-0.9.0-beta-linux.tar.gz`
- Ensure headless Linux uses `xvfb-run` for SwarmUI.
- The release bundle includes Python tests and fixtures for running `python3 -m pytest tools/cohesix-py/tests`.

### 8) Final release gate (must pass)
- `scripts/ci/test_plan_stage_05_due_diligence.sh`
- `scripts/ci/due_diligence_gate.sh`
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings`
- `CARGO_INCREMENTAL=0 cargo check --workspace`
- `CARGO_INCREMENTAL=0 cargo test --workspace`
- Do not progress beyond this stage until all prior scripted stages have completion markers and the due-diligence gate is fully green.

## Trace replay limits
<!-- coh-rtc:trace-policy:start -->
### Trace replay limits (generated)
- `trace.format.version`: `1`
- `trace.hash`: `sha256`
- `trace.max_bytes`: `1048576`
- `trace.max_frame_bytes`: `8192`
- `trace.max_ack_bytes`: `256`

_Generated by coh-rtc (sha256: `c502a57721e43d5c38f5499767a8668eb593ac74f25cb2389632804c4d7f15f2`)._
<!-- coh-rtc:trace-policy:end -->

## Manifest fingerprints
- `out/manifests/root_task_resolved.json` — `sha256:9508011a97545c95df885b868516acd4a74ac4021e56b880035a1f9d0f1a8eaf`

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
- `tests/fixtures/transcripts/converge_v0/coh.txt` — `sha256:96b57611f848ef6f9691678df8b20f261dffd47db449cd63459f12f166c0f4a7`
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:dafd88f7d7e984454e12815ccffd203f98c446d0eb1e8a364d79805aa69de017`
- `tests/fixtures/transcripts/control_plane_v0/cohsh.txt` — `sha256:f43434e6b3071753596e919021e573cb7f6a9831123769dd7cefb5b0c115c1ef`
- `tests/fixtures/transcripts/run_demo_v0/cohsh.txt` — `sha256:d429aa09972892adaeabed60ef2a36e4fe366eb9e730a8467a85f27870957040`
- `tests/fixtures/transcripts/peft_roundtrip_v0/cohsh.txt` — `sha256:a761096db1c412e8b775f3bdb78a9aec79b95ef787d0e933406d23c20285f7db`
- `tests/fixtures/transcripts/trace_v0/cohsh.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/swarmui.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`
- `tests/fixtures/transcripts/trace_v0/coh-status.txt` — `sha256:56b97a2d8486ed783d7cb93d38ea67811d93df6efcc24d7ed97265a4df1b1c4f`

## Trace fixture hashes
- `tests/fixtures/traces/trace_v0.trace` — `sha256:0f5a1935e973fbdb57e73a952b9cd02d1060086167efb4b9e79b28169f308561`
- `tests/fixtures/traces/trace_v0.hive.cbor` — `sha256:977113ebcfad69272cbb15ddc57e7ce1ccd1df87baa6568704253cacc55e8e2d`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes, required scripted-stage references, and command alignment (`python3`, workspace/tests gates); `scripts/check-generated.sh` invokes it.
