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
- For target-qualified evidence, select the target explicitly:
  - `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<run-id>`
  - `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/<run-id>`
- For focused debugging, run one stage in iteration mode:
  - `scripts/ci/test_plan_run.sh --target qemu --stage 3 --iteration --state-dir out/test-plan/<run-id>`
  - `TEST_PLAN_ITERATION=1 scripts/ci/test_plan_run.sh --target pi4 --stage 3 --state-dir out/test-plan/<run-id>`
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
- A focused rerun may use `--iteration`; it writes `stage_01.inputs.sha256`
  style input fingerprints and `stage_01.<target>.iteration` markers, but it
  never writes `stage_XX.done` or `stage_XX.<target>.done`.
- Later stages may reuse earlier markers only when their stored input
  fingerprints still match the current test-plan scripts, docs, and regression
  fixtures. If a fingerprint is stale, rerun that earlier stage in the same
  state dir before treating later evidence as current.
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
- For target-qualified PASS, it is executed via `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<run-id>` or `scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/<run-id>`.
- Stages **01-05** complete successfully and create `stage_01.done` ... `stage_05.done` in the shared state dir.
- The state dir contains target metadata in `target.env`.
- Target-qualified runs also create `stage_01.qemu.done` ... `stage_05.qemu.done` or `stage_01.pi4.done` ... `stage_05.pi4.done`.
- Every completed stage records `stage_XX.inputs.sha256`; stale fingerprints
  prevent later-stage reuse until the affected earlier stage is rerun.
- Iteration markers such as `stage_01.qemu.iteration` or
  `stage_01.pi4.iteration` are useful evidence for debugging but do not count
  toward PASS.
- No stage wrote an INCOMPLETE marker (presence of any `stage_*.incomplete` or any files under `out/test-plan/<run-id>/incomplete/` means **FAIL**).
- Stage 05 runs `scripts/ci/due_diligence_gate.sh` and it is green.

Notes:
- Running individual stages (for example `--stage 2`) is for iteration only; it is not a "PASS" run.
- Subset selectors such as `COHSH_BATCH_GROUPS=base` are valid only with
  `--iteration`; a final Stage 03 or due-diligence run requires all regression
  groups.
- "NA" checks must still be logged, but they do not cause failure; INCOMPLETE always fails.

## Target-Qualified Runner Matrix
The staged runner owns target qualification. `--target qemu|pi4` writes `TEST_PLAN_TARGET` to `target.env`, passes `TEST_PLAN_TARGET` and `COHSH_BATCH_TARGET` to every stage script, and writes a target-qualified marker only after the stage exits successfully and the required target artifacts are present.

| Target | Allowed stages | Required target-specific evidence |
| --- | --- | --- |
| `qemu` | 01-05 | Stage 03 archives QEMU TCP regression logs under `qemu-regression-logs/`; Stage 04 may start the self-contained local QEMU plus hive-gateway path when no gateway URL is supplied. |
| `pi4` | 01-05 | Stage 03 requires `COHSH_TCP_HOST` or `COHSH_HOST` for the live Pi 4 TCP console and runs the cohsh batch with `COHSH_BATCH_TARGET=pi4`; Stage 04 requires `COHESIX_GATEWAY_URL`, `HIVE_GATEWAY_URL`, `COHSH_REST_URL`, or `COH_REST_URL` for an existing REST gateway so it cannot silently create QEMU evidence; Stage 05 requires `PI4_RUNTIME_DMA_PROOF_FILE` or `out/test-plan/<run-id>/pi4-runtime-dma-proof.env` containing `PI4_RUNTIME_DMA_PROOF=fresh-pi` and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`. |

Unsupported target/stage combinations fail before the stage starts. A Pi 4 Stage 03 run must not default to loopback unless `TP_PI4_ALLOW_LOOPBACK=1` documents an intentional local tunnel. A Pi 4 Stage 04 run without an existing gateway URL is **FAIL**, because the stage's self-contained local-QEMU fallback would be QEMU evidence, not Pi 4 evidence.

## Purpose
Validate the full Cohesix stack end-to-end: generated artifacts, QEMU boot, TCP console reliability and performance, deterministic replay, and every shipped host tool.

## Goals
- Pre-existing features continue to work; new features are validated against documented behaviour.
- QEMU boots the VM and exposes Secure9P/TCP console without protocol drift.
- TCP console remains reliable under load (no unexpected disconnects/resets/partial writes).
- Performance baselines are captured with reviewable `*.summary.json` artifacts
  and interpreted in `docs/BENCHMARKS.md` for any changes affecting
  throughput/latency. Raw artifacts normally live under `logs/bench/` or
  `out/bench/`; commit them under `docs/bench/` only when the active milestone
  explicitly requires checked-in evidence.
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
- seL4 15 QEMU artifact trees must be configured with
  `ElfloaderRootserversLast=ON` and an embedded QEMU `virt` DTB generated with
  `virtualization=on`, so PSCI records `method = "smc"` for the Cohesix QEMU
  launcher.
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
  - Preserve the canonical harness `*.summary.json` plus associated logs or
    target proof under `logs/bench/`, `out/bench/`, or a milestone-approved
    committed evidence path; and
  - Index/interpret the result in `docs/BENCHMARKS.md` when it becomes a
    baseline, comparison point, or release claim.
- Do not use "last local run" as a baseline. If you need a new baseline,
  preserve the artifact path and update `docs/BENCHMARKS.md` in the same change.

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
- `cargo test -p cohsh --test pooling`
- `cargo test -p cohsh` (REST transport is enabled by default; use `--no-default-features` to verify minimal builds)
- `cargo test -p secure9p-core --test session_limits`
- `cargo check -p swarmui --bin swarmui` (Tauri 2 command/context wiring; SwarmUI keeps the Tauri binary out of Cargo test harnesses)
- `python3 scripts/ci/check_swarmui_dependencies.py` (default REST projection may use `ureq`; `--no-default-features` must not pull HTTP clients)
- `cargo test -p swarmui --test dependency_policy`
- `cargo test -p pi4-driver-abi`
- `cargo test -p pi4-driver-runtime -- --test-threads=1`
- `cargo check -p pi4-driver-runtime --target aarch64-unknown-none`
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
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ninedoor::tests`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net -- --test-threads=1`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib net::stack`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::flush_tx_backpressure_does_not_count_as_budget_overrun`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_write_moves_bytes_without_root_port_pointer`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_poll_moves_rx_bytes_without_root_port_pointer`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_defers_buffered_network_console_lines_for_driver_task_turn`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::nettest_reports_wifi_host_eapol_pending_detail`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::netstats_emits_compact_status_line`
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::`
- `cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance`
- `cargo test -p sel4-sys --lib`
- `SEL4_BUILD_DIR=$REPO/seL4/SMP_build cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu`
- `SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4`
- `CARGO_INCREMENTAL=0 cargo test --workspace --exclude swarmui` (the stage
  exercises SwarmUI through the explicit binary check and focused tests above;
  the Tauri binary must stay out of the generic Cargo test harness)
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

Stage 02 includes host-safe 1000-worker pressure coverage for Secure9P tag/window/fid churn, `cohsh` session-pool fan-out, localhost TCP framed logical sessions, NineDoor worker namespace listing/telemetry retention, and mixed Pi 4 driver-task ring scheduling under serial/USB/HDMI plus GENET/CYW43 pressure. The local-seat coverage also guards the root-owned HDMI terminal history, prompt/input echo shape, viewport redraw coalescing, and no-reply backpressure policy so stale HDMI payload replay cannot regress behind green host tests. These tests are regression guards for bounded control-plane and scheduling behavior; they are not Pi 4 Wi-Fi/GENET hardware throughput proof and do not replace Stage 03/04 QEMU runs or fresh Pi 4 hardware benchmarks.

Pi 4 trace evidence remains a post-capture host workflow. `scripts/pi4-image-build.sh` stages USB/Wi-Fi trace helpers, but fast host tests invoke `scripts/pi4_trace_normalize.py` and `scripts/pi4_gate_proof.sh` tests directly and do not require a flashed SD card or serial log. The same normalizer also provides `--gate-summary` plus repeated `--expect KEY=VALUE` checks for narrow USB/Wi-Fi hardware runs, so a serial capture can fail fast on regressions such as `USB_BLOCKER=cmd-submit-proof-timer-preempted`, `USB_BLOCKER=usbcmd-run-preserved-reset-bit`, `USB_POST_FIRST_BYTE_BLOCKER=usb-post-first-byte-queue-collapse`, `USB_POST_FIRST_BYTE_BLOCKER=usb-post-first-byte-recovery-failed`, `WIFI_BLOCKER=armcr4-prereset-fgc-cmd53-r5-rejected`, `WIFI_BLOCKER=ht-clock-timeout`, `BOOT_HALTED=yes`, `PANIC_SEEN=yes`, `PANIC_REASON=bootinfo-snapshot-corrupted`, or `TIMER_IRQ27_SEEN=yes`. For USB and Wi-Fi, `*_GATE` records the last proven gate; `*_BLOCKER` names the failed or blocked next gate when acceptance is not complete. Gate 10 records USB command-ready parser admission, while `USB_FIRST_REPORT_READY`, `USB_LOCAL_SEAT_STATE`, `USB_BUSY_AFTER_READY`, and `USB_POST_FIRST_BYTE_BLOCKER` separately guard local-seat HID proof and sustained keyboard acceptance. `USB_STARTUP_BLOCKER_SEEN` is diagnostic pre-command churn; `USB_ACTIVE_BLOCKER_SEEN` and `USB_RECOVERED_FROM_BLOCKER` are post-ready health evidence and can block perfect local-seat proof. The Pi 4 local-seat driver coverage module is part of Stage 02 because it owns USB keyboard input proof contracts, Caps/Num/Scroll LED bitmaps, post-seal LED-sync enablement, HDMI progress refresh cadence, and Wi-Fi progress suppression while USB boot activity is active.

For host-EAPOL Wi-Fi captures, byte-level isolated runtime RX evidence must win
over downstream prompt symptoms. `CYW43_DRIVER_TASK_HOST_EAPOL_STATUS` records
with `last_rx_idle_detail=0x570a`, `0x570b`, `0x5709`, `0x570c`, `0x570d`, or
`rx_firstread_decode_miss>0` normalize to `cyw43-data-rx-firstread-empty`,
`cyw43-data-rx-firstread-invalid-sdpcm`,
`cyw43-data-rx-firstread-failed`,
`cyw43-data-rx-firstread-remainder-failed`,
`cyw43-data-rx-firstread-remainder-too-large`, or
`cyw43-data-rx-sdpcm-decode-miss` respectively, even when later `wifi diag`,
`nettest`, or `netstats` lines report the coarse `host-eapol-required`
net-disabled cause. The same status records expose the association-gated linked
receive window through `event_rx`, `control_rx`, `associated`, `link_up`,
`assoc_event`, `assoc_poll`, `post_assoc_polls`, and
`control_rx_firstread_*`; empty first-read records also expose `rxsrc_*` and
`control_rxsrc_*` fields decoded from the packed runtime result so tests and
captures can distinguish missing firmware RX source, masked CCCR/SDIO
interrupts, absent SDHCI card-int, and Function 2 readiness. Those fields
diagnose missing association/control events but do not override a more precise
nonzero `rx_firstread_*` data-RX blocker.
`CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE` detail lines must be self-contained:
the standalone line carries the RX trace flags, detail, request length, CMD53
shape, transfer result, queue pressure, IRQ-preserve decision, and the compact
VCNT-backed timing chain (`trace_seq`, `start_ticks_lo`,
`pre_sample_delta_ticks`, `transfer_delta_ticks`,
`post_sample_delta_ticks`). `scripts/pi4_trace_normalize.py --gate-summary`
promotes those fields to `WIFI_RXTRACE_*` and `WIFI_RX_IRQ_PRESERVE_*` so a
failed Pi boot can distinguish first-read transfer shape, runtime queue
pressure, interrupt preserve/clear policy, and RX timing without depending on a
large status line staying untruncated. Runtime RX IRQ preservation is valid only
for concrete queued work such as a cached SDPCM next-frame length or a nonzero
Function 1 RFRAME count; the old preserve reason code `5` is normalized as
`deprecated-source-asserted` because a pre-ACK reread of `I_HMB_FRAME_IND` can
be stale source evidence, not fresh RX admission proof. Repeated pending status
records with no semantic progress may be suppressed; the next full
`CYW43_DRIVER_TASK_HOST_EAPOL_STATUS` row reports `suppressed_status=<count>`.
Terminal `required`/`secure` rows and edge rows for events, RX observation,
EAPOL progress, admission refresh/rescue, and post-secure handling must remain
unsuppressed.
For isolated CYW43 control failures, `CYW43_DRIVER_TASK_COMMAND_NO_REPLY` records
with `reason=cyw43-runtime-command-no-reply` must normalize to Gate 7
`control-plane-reply-idle-loop`. If the cached child progress marker matches the
active request sequence and CYW43 aux tag, the marker phase is preserved as
`WIFI_EXACT` / `WIFI_PHASE`; otherwise the exact reason remains the generic
runtime no-reply. Split-control `poll-complete` and nonmatching reply samples are
context only until the same attempt emits a terminal
`CYW43_DRIVER_TASK_CONTROL_SPLIT` / `CYW43_DRIVER_TASK_COMMAND_FAULT` pair.
Valid nonmatching CDC replies may also be preserved for a later exact `(cmd,id)`
match; when that happens the later attempt must emit `cached-matched-reply` and
`cached-response-ready` split-control evidence, and must still validate CDC
status and response length through the normal matched-reply path. These cached
reply records are proof of correlation repair only, not a longer reply deadline
or a relaxed host-EAPOL `wsec_key` gate.

Reopened Milestones 26a/26b also require HAL driver-task contract coverage before hardware claims: `hal::driver_task` must validate the serial, USB/local-seat, HDMI, GENET, CYW43, SDIO host, PCIe root, RTL8139, and virtio-net contracts. Historical M26B completion evidence remains a compatibility baseline, not reopened acceptance proof. Reopened Pi 4 captures must include compact `DRIVER_TASK_*`, `SCHED_CONTRACT`, `BUDGET_OVERRUN`, observed per-driver latency, `SERIAL_ECHO`, `USB_BURST`, and `HDMI_RESPONSIVE` evidence; `scripts/pi4_trace_normalize.py --gate-summary` exposes those as machine-checkable hardware proof fields.

Dedicated-driver-task closure is stricter than contract declaration: `DRIVER_TASK_DEDICATED` must cover the required active roles, `DRIVER_TASK_COMPATIBILITY` must be `0`, `DRIVER_TASK_DEDICATED_READY=yes` must be present, `DRIVER_TASK_FAILED_COUNT=0` must be present, serial, USB/local-seat, display, selected network, selected-role SDIO (`DRIVER_TASK_SDIO_DEDICATED=yes`) for Wi-Fi, and PCIe role booleans must all be `yes`, and substrate/capset/fault/revoke/scheduling/per-driver-affinity/VSpace plus pointer-free IPC, owner-state proof, sealed runtime descriptor proof, and active-network identity fields must all be `yes` when `scripts/pi4_gate_proof.sh --require-driver-task-proof` is used. Physical Pi bootstrap is limited to the selected generated isolated runtime hardware contracts; RTL8139 and virtio-net remain QEMU compatibility contract coverage only. Owner-state proof requires one `DRIVER_TASK_OWNER_STATE ... hot_path=<exact> owner_state=driver-owned descriptor=present descriptor_version=4 descriptor_seal=valid artifact_hash=nonzero root_pointer=no` line for each current acceptance hot path: `serial-console`, `usb-keyboard`, `hdmi-text`, `pcie-root`, and the selected network path (`genet-nic` for wired or `cyw43-wifi` plus `sdio-host` for Wi-Fi). The canonical sealed descriptor fragment is `DRIVER_TASK_OWNER_STATE ... descriptor=present descriptor_version=4 descriptor_seal=valid`. Split clients must carry `bus_link_seal=valid` for USB-to-PCIe or CYW43-to-SDIO while non-split roles report `bus_link_seal=none`. Aggregate owner-state text, inferred hot paths, inactive-network hot paths, truthy aliases such as `owner_state=yes`, or pre-seal `descriptor=present root_pointer=no` logs without descriptor-seal fields must fail current closure.

For `scripts/pi4_gate_proof.sh --require-driver-task-proof`, SDIO dedication is
mandatory for Wi-Fi and full-ready closure, but a wired-only
`--require-wired-ready` capture closes the selected network path with GENET and
must not be failed solely because `DRIVER_TASK_SDIO_DEDICATED=no`.

CYW43/SDIO host tests must prove the shared owner command page remains SPSC:
root submission and staging are admitted before handoff, handoff is rejected
while the root slot is active or its completion is undrained, successful
handoff deletes/zeros root's SDIO endpoint authority, all later root SDIO
submission and staging fail before copying bytes, and delegation cannot return
to root. Live Wi-Fi proof must contain the successful one-way handoff marker
before the first CYW43 transport/firmware command. Power-sequence coverage must
also prove that an already root-preseeded mailbox frame remains available for a
child capability copy after fresh device-untyped coverage is consumed; no
second retype is permitted. Runtime tests must drive the retained Linux-ordered
GET_GPIO_CONFIG/polarity, output-low, power-off, 2 ms wait, power-up, 10 ms
wait, release-high, startup-clock, 10 ms wait, and finalize phases one turn at
a time. Pending turns publish no completion or owner notification, wait phases
must not repeat physical writes, firmware GPIO success requires the returned
zero GPIO token, and generation reset must remain poisoned/masked with no
pending epoch until the terminal phase. Each GET_CONFIG, SET_CONFIG,
ASSERT_LOW, and RELEASE_HIGH firmware-property operation must post exactly
once, retain the DMA request page across later reply-poll turns, use a
virtual-counter deadline, and publish distinct begin/done progress plus an
operation-specific terminal detail. Property requests must carry a zero
request/response-size word. SET replies with the response bit and a bounded
zero-byte returned length must be accepted, while missing response bits,
oversized returned lengths, nonzero returned GPIO tokens, wrong tags, and bad
end tags must fail; GET_CONFIG still requires its complete polarity payload.
Root must extend same-request retention
only while one of those exact begin phases is current; mismatched sequence,
aux, contract, mode, done phase, or unrelated progress retains the ordinary
SDIO bound. Tests must prove timeout retention cannot permit a new request to
replace the firmware-owned page.

Host tests must prove the fixed-layout pointer-free command/completion records remain primitive-only and bounded, including primitive aux fields for service-turn arguments, nonzero-progress/frame-ready-only hot-path credit, owner-state descriptor rejection when the matching runtime spec is not acceptance-eligible, owner-state acceptance requiring the explicit owner hot-path mask plus acceptance-eligible runtime images, the separate root-context diagnostic versus pointer-free selector registration classes, the common `DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE` bit forced onto transitional root-context ring commands, and the one-way command flag used by send-only bootstrap/background turns so isolated runtimes do not call `Reply` without a reply cap. Runtime-init records must carry primitive MMIO/DMA/shared physical page metadata, fixed virtual bases, semantic resource ranges for large apertures and large buffer arenas, bus-address policy, optional IRQ descriptors, optional bus-link descriptors, and framebuffer metadata without root pointers.

The physical Pi profile now requires isolated child VSpaces for driver bootstrap, loads isolated `pi4-driver-*` runtime image payloads only from the raw driver-runtime CPIO embedded into the Pi 4 root-task image by `scripts/pi4-image-build.sh`, maps all bounded `PT_LOAD` pages declared by generated `code-pages`, and uses fixed command/completion rings instead of shared-root service TCBs. The staged U-Boot CPIO remains audit/packaging evidence and is not a runtime fallback on the physical Pi profile. `scripts/pi4-image-build.sh` strips the root-task ELF copy injected into the seL4 archive, and Pi packaging must still pass `scripts/ci/size_guard.sh seL4/build_UBOOT/elfloader/archive.archive.o.cpio`. Seven generated runtime specs are acceptance-eligible (`root_context_required=false`, `hardware_state_migrated=true`); `sdio-host` is generated with one HAL-declared SDHCI MMIO page, one noncontiguous HAL-declared firmware-mailbox MMIO page, one private low DMA request page, and 32 shared pages. Host coverage must prove the generated `root_task.driver_images` table covers all seven hot paths, declares at least the 16-page xHCI minimum aperture, reserves the descriptor-backed runtime budgets (`usb` 64 DMA/32 shared, `hdmi` 0 DMA/16 shared plus framebuffer, `genet` 64 DMA/32 shared, `cyw43` 0 DMA/64 shared, `sdio` 2 MMIO/1 DMA/32 shared, `pcie` 16 shared, `serial` 4 shared), and checks the separate `pi4-driver-*` runtime package for host and `aarch64-unknown-none`.

Milestone 26c Pi runtime/DMA proof states are machine-checkable and must not be inferred from adjacent evidence. `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-kernel-source-dir "$HOME/seL4_15"` writes `out/pi4-sd/pi4-runtime-dma-proof.env` with `PI4_RUNTIME_DMA_PROOF=target-build`, `PI4_RUNTIME_DMA_PROFILE=bounded-no-iommu`, manifest hash, runtime CPIO hash, runtime uImage hash, and staged image hash; this proves source freshness and packaging only. Under Milestone 26d, the Pi build tree must also be the accepted seL4 15.0.0 `bcm2711` profile with `KernelArmExportVCNTUser=ON`, physical counter/timer-control exports off, `TIMER_CLOCK_HZ=54000000`, and no retained one-domain `KernelDomainSchedule` cache entry. `scripts/pi4_trace_normalize.py --gate-summary` emits `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF`, `DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER`, and `PI4_RUNTIME_DMA_PROOF=absent`, `diagnostic`, `qemu-or-stale-log`, or `fresh-pi` from serial evidence. `scripts/pi4_gate_proof.sh --require-driver-task-proof --runtime-dma-proof-out out/test-plan/<run-id>/pi4-runtime-dma-proof.env` writes the live proof bundle only after normalization passes. Only `fresh-pi` counts as live hardware runtime/DMA proof, and it requires driver-task dedicated readiness, cap/fault/revoke/scheduling/affinity proof, isolated VSpace, pointer-free IPC, per-hot-path `DRIVER_TASK_OWNER_STATE ... descriptor=present root_pointer=no`, sealed descriptor version/hash/identity proof for every active hot path, sealed bus-link proof for USB and CYW43 split clients, per-hot-path `DRIVER_TASK_DMA_PROOF` with bounded no-IOMMU profile and cache/bus-address policy, aggregate `DRIVER_TASK_DMA_BLOCKER=none`, no compatibility service roles, no unresolved ring timeouts/deferred bootstrap, no resource blockers, a fresh Pi cold-boot marker, and a live prompt. Raw `DRIVER_TASK_RING_CALL_TIMEOUT` events remain diagnostic, but `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT` must be `0` after later return proof closes any bounded keep-active turn. It also emits `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified` only when `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, and at least one valid `DRIVER_TASK_COUNTER` snapshot with `DRIVER_TASK_COUNTER_INVALID=0` are present.

The isolated runtime engines contain production service turns for serial mini-UART init/RX/TX, HDMI framebuffer rendering, PCIe MMIO turns, direct-root-port xHCI boot-keyboard polling, GENET MDIO/MAC/RX/TX rings, CYW43 shared-control SDPCM command records, and SDIO fixed-layout CMD52/CMD53/POLL_IRQ service turns. Physical Pi root starts each isolated runtime with `TCB.WriteRegisters(resume=1)` at a shell-safe bootstrap priority while preserving the contract MCP, then raises it to contract priority after the pre-root descriptor handoff plus the mandatory serial bootstrap reply proof, or after a prompt-side reply proof for roles that were intentionally skipped because pointer-free runtime proof was unavailable; it may emit `DRIVER_TASK_BOOTSTRAP_DEFERRED ... reason=root-shell-before-first-service-proof` so an unproved child cannot starve the root shell. Captures may show SDIO host, PCIe root, GENET, and CYW43 `DRIVER_TASK_BOOTSTRAP_DEFERRED ... reason=root-shell-before-first-service-proof` lines before the prompt; those markers are acceptable only as shell-preserving fail-closed evidence, and the retained descriptor must later replay with `DRIVER_TASK_RUNTIME_INIT_DEFERRED ... status=resumed owner=linked-runtime proof_effect=deferred-proof-retry-enabled` before matching call/return service proof can close SDIO, PCIe, or network acceptance. Wi-Fi descriptor replay is prompt-safe: when physical Pi pointer-free IPC proof is present, root emits `[net-console] deferred resume scheduled reason=driver-startup-before-root-prompt action=delay-interactive-prompt`, starts SDIO/CYW43 replay with `[net-console] deferred resume reason=before-root-prompt action=start-wifi`, and publishes `cohesix>` only after replay returns ready, pending, or fail-closed. A no-reply runtime must emit `DRIVER_TASK_RING_CALL_TIMEOUT` and `DRIVER_TASK_RUNTIME_INIT_DEFERRED ... status=pending owner=linked-runtime action=serial-shell proof_effect=acceptance-red-until-replayed`, not continue hidden synchronous Wi-Fi work behind an already-published prompt. If pointer-free proof is absent, root must emit `[net-console] deferred resume skipped reason=driver-task-net-runtime-unproved action=serial-diagnostics-only`, preserve serial diagnostics, and leave Wi-Fi acceptance red until an explicit diagnostic command retries the failed stage. Physical Pi local-seat captures must show HDMI as an independent display sink without making it a pre-prompt serial dependency: the framebuffer hint must be available before driver-task bootstrap, `hdmi-text` may receive a bounded nonblocking bootstrap engine-init retry after descriptor load, and the early `Starting HDMI` submit-frame proof must be bounded/nonblocking. A no-reply HDMI render path is display-red evidence and must emit `DRIVER_TASK_RING_CALL_TIMEOUT`, but it must not prevent `cohesix>` from becoming responsive on UART. No HAL-mapped framebuffer diagnostic mirror is allowed; HDMI output must come from the isolated `hdmi-text` runtime only, and UART remains the complete log stream. USB keyboard runtime attach may run before the prompt only after pointer-free isolated runtime proof; otherwise, after `cohesix>`, local-seat must defer if the USB runtime already has an active command, emit one `prompt-settle attach deferred reason=usb-runtime-active` summary, and retry after the normal quiet window before replaying the deferred PCIe root descriptor and one bounded USB engine-init proof. It must then advance HID discovery only through the explicit keyboard-enumeration aux while ordinary background polls report the current frontier without re-entering enumeration. No-reply background USB polls must produce `DRIVER_TASK_RING_CALL_TIMEOUT` plus `[local-seat] isolated USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell` rather than repeated blocking `usb-local-seat` calls. `usb probe-kbd` must execute a bounded progress-driven keyboard-enumeration burst and must not replay the whole isolated local-seat attach/init chain; each extra slice is permitted only while the child USB enumeration marker advances, and the burst stops at the finite cap, keyboard readiness, or no new marker. Root-console startup must emit UART-visible `[mark] root-console.start.begin`, publish `cohesix>` only after bounded driver startup settles or fails closed, and emit `[mark] root-console.start.ok` before `/log/queen.log` or NineDoor log-stream handoff; `Cohesix console ready` is emitted before deferred Wi-Fi EAPOL/DHCP settle, so host-EAPOL waits cannot hold the serial shell hostage. Once `cohesix>` is published and USB polling is armed, serial UART and USB keyboard input must both feed the shared parser concurrently after USB proof succeeds. Steady physical Pi root submits serial/network service turns through bounded ring calls; HDMI submits are limited to high-impact progress lines, while init, deferred-resume, timeout, and proof turns keep UART breadcrumbs. USB keyboard auto-poll uses bounded nonblocking sends until the runtime proves it can reply without risking serial. The isolated runtime `_start` entry must preserve root's task key, install the mapped driver-local IPC buffer before receiving commands, skip `Reply` for commands marked with the one-way flag, and emit replies only for call-delivered commands. Hardware captures should show `DRIVER_TASK_RING_CALL_BEGIN` and the matching `DRIVER_TASK_RING_CALL_RETURN` for init/deferred-resume/proof turns; routine steady console data turns may be suppressed to keep interactive serial latency bounded. Any `DRIVER_TASK_RING_CALL_TIMEOUT` or positive `DRIVER_TASK_BOOTSTRAP_DEFERRED` keeps driver-task acceptance red until later service proof closes it. A role boolean is credited only from a line proving both `live_tcb=yes` and `hot_path=dedicated`; static contract isolation, callback-pointer live-TCB service turns, shared-root ring service turns, runtime-image declarations, runtime-region mapping, runtime-image smoke loops, runtime-init descriptor commands, and any ring command marked root-context or init-descriptor non-acceptance are diagnostic until the driver state boundary is owned by an isolated ring-backed task, VSpace proof is `yes`, pointer-free IPC is `yes`, and `owner_state=driver-owned` is present. Pre-root bootstrap turns, including the serial bootstrap reply proof, must not sample timer registers. Later ring latency telemetry may sample the EL0 virtual counter only when the profile enables `timers-arch-counter`; dummy-timer Pi captures must suppress latency proof rather than reading CNT registers.

The Pi 4 manifest defaults place both `bcmgenet-v5` and `cyw43455` on core `3`; hardware captures must show `DRIVER_TASK_BOOT ... contract=bcmgenet-v5 ... affinity_core=3` and `DRIVER_TASK_BOOT ... contract=cyw43455 ... affinity_core=3` before claiming fourth-core driver placement. Physical Pi owner-state boots apply the same working `seL4_TCB_SetAffinity` path already used for NineDoor and worker TCBs; any `DRIVER_TASK_AFFINITY_DEFERRED ... reason=pi4-child-tcb-affinity-boot-stall-guard` line is stale mitigation evidence and must fail placement proof. Captures may still emit `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED ... reason=pi4-early-tcb-notification-bind-boot-stall-guard`; that keeps notification lifecycle proof red while allowing endpoint-backed command-ring startup to proceed. QEMU virtio compatibility boots may prove isolated VSpace/ASID allocation, runtime-image transport-region mapping, and pointer-free ring transport after virtio networking is online, but that is transport-substrate evidence only. Fresh Pi hardware proof is still required before claiming Wi-Fi/DHCP, GENET/DHCP, USB keyboard, HDMI, or strongest isolated-driver hardware acceptance.

Strict Pi SDIO command/data calls, fixed-layout SDIO CMD52/CMD53 descriptors, CYW43 firmware/NVRAM/SDPCM command records, direct-root-port xHCI keyboard polling, GENET RX/TX descriptor-ring service, and PCIe port read/write/flush helpers now compile in isolated runtime code before any root hardware execution; host coverage must keep proving those ring turns while preserving the fresh-Pi board-proof boundary.

Current Wi-Fi acceptance also requires one exact `CYW43_SDIO_DPC generation=<n> captures=<n> published=<n> consumed=<n> rearms=<n> overruns=<n> epoch_errors=<n> sequence_errors=<n> ack_failures=<n> poisoned=yes|no masked=yes|no` diagnostic in the current boot slice and `WIFI_DPC_PROOF=yes` from `scripts/pi4_trace_normalize.py --gate-summary`. `wifi diag` emits this line only after a stable, valid read of the admitted SDIO owner ring and a same-generation v9 CYW43 client-counter sample. Acceptance fails closed with `WIFI_DPC_REASON=no-activity` unless the current exact proof has both `captures > 0` and `published > 0`; it also fails when the line is missing, poisoned, or masked, any overrun/epoch/sequence/ack failure is nonzero, captured and published totals differ, consumed and published totals differ, or the final IRQ service state is unrearmed. Exploratory summaries and wired-only historical evidence remain readable without this Wi-Fi-only proof.

### 3) QEMU boot + TCP console baseline
- `scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh`
- Stage 03 sets resilient defaults for clean hosts: `TP_STAGE3_READY_TIMEOUT=900`, `TP_STAGE3_PORT_TIMEOUT=60`, `TP_STAGE3_AUTH_READY_TIMEOUT=120`, `TP_STAGE3_QUIT_CLOSE_TIMEOUT=60` (override as needed).
- `scripts/cohsh/run_regression_batch.sh` keeps Cargo build cache by default; set `COHSH_BATCH_CLEAN_TARGET=1` only for deliberate clean-rebuild validation.
- `scripts/cohsh/run_regression_batch.sh` defaults to `COHSH_BATCH_TARGET=qemu`, boots fresh QEMU instances for the base, telemetry, shard, and gated groups, and is invoked by stage 03.
- Pi 4 hardware bring-up uses the same official runner against an already-booted TCP console: `COHSH_BATCH_TARGET=pi4 COHSH_TCP_HOST=<pi4-ip> COHSH_TCP_PORT=31337 scripts/cohsh/run_regression_batch.sh`. Pi mode archives a full per-script ledger, runs lifecycle resume before/after groups and scripts, continues after failures by default, and writes a unique `out/regression-logs/pi4-full-<utc>/summary.log` unless `COHSH_LOG_ROOT` is set.
- Stage 03 archives per-script logs under the stage state dir (for example `out/test-plan/<run-id>/qemu-regression-logs/`).
- Manual runs of `scripts/cohsh/run_regression_batch.sh` default to `out/regression-logs/` unless `COHSH_LOG_ROOT` is set.
- Focused Stage 03 iteration may use `COHSH_BATCH_GROUPS=base`,
  `base-telemetry`, `base-shard`, or `gated` with `--iteration`. Without
  `--iteration`, any subset writes an INCOMPLETE record and cannot produce
  Stage 03 PASS evidence.
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
  - Any performance regression claim must be backed by reviewable baseline
    artifacts and indexed in `docs/BENCHMARKS.md` when applicable; do not
    compare against unpublished local runs.
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
- `coh` (TCP console; requires `configs/generated/coh_policy.toml`):
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
    - `HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS=120000`
    - `HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS=120000`
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
    - `curl -sS 'http://127.0.0.1:8080/v1/fs/tail?path=/log/queen.log&max_bytes=512&lines=64' | jq .`
    - Failure classification is part of the contract: `HTTP 429` means bounded broker queue backpressure, `HTTP 503` means transport unavailable, and `HTTP 504` means the broker accepted work but the backend response exceeded its response timeout.
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
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-10mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-100mb --error-budget-rate 0.01`
- `python3 scripts/rest_perf_harness.py --mode simulate --rest-url http://127.0.0.1:8080 --no-retries --fast-ramp --scenario telemetry-1gb --error-budget-rate 0.01`

Pass criteria:
- Every run exits `0`.
- Summary artifacts exist (`*.summary.json`, `*.ops.csv`, `*.ramp.csv`, `*.ramp.svg`).
- `error_budget_pass=true` and `error_rate <= 0.01` in each summary JSON.
- `no_retries=true`, `fast_ramp=true`, and `scenario` equals the requested preset in each summary JSON.

Failure policy:
- Any scenario above the error budget is a release-blocking defect.
- Do not use retry flags or ad-hoc rerun wrappers to mask failures; tune/fix code and re-run the same matrix.
- On slower physical targets, keep `--ready-timeout-secs` greater than the gateway broker response timeout and pass explicit harness overrides such as `--gateway-broker-control-response-timeout-ms 120000 --gateway-broker-telemetry-response-timeout-ms 120000` rather than lowering the error budget or enabling retries.

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
  - `python3 scripts/rest_perf_harness.py --mode simulate --multi-hive --hives 3 --workers-per-hive 1000 --no-retries --error-budget-rate 0.01`
  - Summary JSON must report `multi_hive=true`, `hives=3`, `workers_per_hive=1000`, and pass error budget.

Pass criteria:
- No split-brain writes: mutation authority remains single-writer per hive.
- Relay retries are deterministic and idempotent across restarts.
- No ACK/ERR/END grammar drift versus existing fixtures.
- Any failed mandatory federation check is release-blocking.

### 6d) Pi 4 DHCP + U-Boot policy compatibility and reopened driver-task proof (Milestones 26a/26b)
Run this matrix in addition to the staged runner when Milestone 26a or 26b files change. Older checked-in M26B Wi-Fi/DHCP captures prove the retained compatibility baseline only; reopened 26a/26b closure additionally requires fresh USB/serial/HDMI responsiveness evidence under wired and Wi-Fi load plus the driver-task scheduling fields below.

- Compiler + docs gate:
  - `cargo test -p coh-rtc`
  - `cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json`
- Host DHCP/policy gate:
  - `cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture`
  - Confirms the bounded DHCP core plus runtime policy override plumbing without changing QEMU grammar.
- Driver hot-path budget gate:
  - `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib log_buffer::tests::cursor_reads_retained_lines_in_order_across_batches`
  - `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::cat_queen_log_streams_full_payload_after_ack`
  - `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::tail_queen_log_honors_default_and_requested_line_counts`
  - `cargo test -p cohsh --lib log_dump`
  - `cargo test -p swarmui --test log_dump`
  - `python3 -m pytest -q tests/test_rest_perf_harness.py`
  - `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib wired_nic_steady_dataplane_trace_is_suppressed_for_benchmarks`
  - `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_driver_task_firmware_ready_is_not_dhcp_ready`
  - `cargo test -p pi4-driver-runtime --lib cyw43_data_tx_is_credit_gated_and_preserves_sequence_on_no_credit -- --test-threads=1`
  - `cargo test -p pi4-driver-runtime --lib cyw43_control_tx_is_credit_gated_and_preserves_sequence_on_no_credit -- --test-threads=1`
  - `cargo test -p pi4-driver-runtime --lib cyw43_rx_queue_removes_matching_channel_without_reordering_data -- --test-threads=1`
  - `cargo test -p pi4-driver-runtime --lib genet_rx_drain_budget_caps_one_service_turn -- --test-threads=1`
  - `cargo test -p pi4-driver-runtime --lib genet_tx_completion_reclaim_budget_caps_one_service_turn -- --test-threads=1`
  - `cargo test -p pi4-driver-runtime --lib genet_service_reports_budget_exhaustion_before_dataplane_work -- --test-threads=1`
  - Confirms routine wired NIC ring traces are suppressed during benchmark-mode dataplane turns, runtime CYW43 data TX is credit-admitted instead of spin-wait admitted, Wi-Fi DHCP/data release still requires secure carrier, and GENET service turns stay budget-capped while dedicated task proof is pending.
- Pi 4 image / U-Boot gate:
  - `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`
  - `scripts/uboot/qemu-uboot-smoke.sh --net user`
  - Confirm U-Boot env control remains deterministic (`ipaddr`, `serverip`, `coh_net_mode`, `coh_net_interface`), generic persistent `uboot.env` import is disabled with `CONFIG_ENV_IS_NOWHERE`, `CONFIG_PREBOOT` stays on the serial/video console path, the staged Pi 4 boot script owns the first menu/input USB bootstrap, reloads `cohesix.env`, mirrors `coh_net_*` values into the staged padded `bcm2711-rpi-4-b.dtb`, and boots the seL4 elfloader through U-Boot `bootm` with that DTB.
- QEMU compatibility gate:
  - `scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none`
  - Existing QEMU hostfwd defaults (`127.0.0.1:{31337,31338,31339}`) and ACK/ERR/END fixtures must remain unchanged.
  - QEMU virtio compatibility logs may first show `DRIVER_TASK_BOOT status=skipped reason=qemu-virtio-pre-net-resource-guard`; that preserves virtio TCP resources before network init.
  - Optional QEMU driver-task smoke uses `cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu,qemu-driver-task-smoke` plus a deliberate QEMU boot such as `scripts/cohesix-build-run.sh --cargo-target aarch64-unknown-none --root-task-features kernel,serial-console,net-console,net-backend-virtio,cache-maintenance,qemu-driver-task-smoke --raw-qemu --tcp-port 31347`. The no-USB profile is the preferred local boot attempt while the full USB smoke image remains above the current elfloader placement ceiling. After virtio networking is ready, those logs must show a console-visible `DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu status=summary configured=9 failed=0 live_tcb_count=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes runtime_image_declared=7 runtime_transport_mapped=7 runtime_acceptance=7 runtime_declared_hot_paths=0x7f runtime_mapped_hot_paths=0x7f owner_state=not-proven` line. HAL-only per-contract boot lines may also include `runtime_image=<transport-mapped|none>`, `runtime_declared=<mask>`, `runtime_mapped=<mask>`, `runtime_acceptance=<yes|no>`, and `owner_state_reason=<reason>`, but the console-visible summary is the required QEMU proof surface. `cargo test -p sel4-sys --lib` must cover the host-stub invocation shape for AArch64 page-map/unmap and ASID-pool calls before any driver VSpace work is claimed. That proves QEMU live-TCB/cap/affinity, isolated VSpace mapping, runtime-image transport mapping, pointer-free transport readiness, and current isolated runtime acceptance eligibility only; it is not Pi 4 driver-task proof and must still leave full dedicated-driver-task hardware acceptance fail-closed until hardware roles and hot-path ownership are proved on Pi. If the run fails before root-task with `image load address overlaps with ELF-loader`, record that as a QEMU image-placement blocker, not a driver-task proof.
- Pi 4 runtime evidence gate:
  - Build-only/stage-only validation is useful but is not Pi 4 acceptance. A reopened 26a/26b hardware run must include a fresh serial capture from the reflashed image, not an older checked-in or operator-provided transcript.
  - The May 20 Wi-Fi capture is triage evidence only: it showed `WIFI_GATE=10` and DHCP bound, but `DRIVER_TASK_SUBSTRATE_READY=no`, `DRIVER_TASK_FAILED_COUNT=9`, and `live_tcb_count=0`. The next hardware run must show no `DRIVER_TASK_BOOT ... status=failed err=seL4_DeleteFirst`, `DRIVER_TASK_SUBSTRATE_READY=yes`, `DRIVER_TASK_FAILED_COUNT=0`, and live hot-path ownership before any dedicated-driver-task claim.
  - The minimum 26a wired/GENET closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-usb-ready --require-wired-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=genet --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - The minimum 26b Wi-Fi closure command is:
    - `scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes`
  - `--require-usb-ready`, `--require-wifi-ready`, and `--require-ready` are stricter than gate/blocker success. They require the isolated runtime old-good replay fields from `scripts/pi4_trace_normalize.py --gate-summary`: `USB_OLDGOOD_REPLAY=yes`, `USB_OLDGOOD_MISSING=none`, `WIFI_OLDGOOD_REPLAY=yes`, and `WIFI_OLDGOOD_MISSING=none` for the selected full-ready path. USB ready proof also requires `USB_LOCAL_SEAT_STATE=ready`, `USB_COMMAND_READY=yes`, `USB_FIRST_REPORT_READY=yes`, and `USB_BUSY_AFTER_READY=no` so parser admission cannot hide missing first-report or post-ready busy evidence. A replay miss reports the first missing translated May/U-Boot/Linux behavior through `*_OLDGOOD_MISSING`; gate 10 without replay remains triage evidence only. USB replay requires distinct ordered endpoint, interrupt-IN, first-report, first-byte, and runtime-gate proof, and the first report/byte must be isolated runtime HID sourced. Wi-Fi replay rejects failed readiness, failed join, generic EAPOL message tokens, and started-only nettest output.
  - Existing logs may be normalized for triage only:
    - `scripts/pi4_gate_proof.sh --normalize-only --log <existing-log> --allow-summary-only`
    - `--allow-summary-only` is not acceptance proof and must not be combined with any `--require-*` hardware acceptance flag.
  - `scripts/pi4_trace_normalize.py --boot-summary` is a fail-closed boot ledger, not an alternative proof path. A `pass` slice requires clean serial, prompt/root-console readiness, arch-counter timer proof, dedicated driver-task owner/DMA/counter proof, selected network proof, `NET_TCP_READY=yes` or `NETTEST_PROOF=yes`, USB cold-boot and old-good local-seat proof, USB burst proof, and HDMI/serial responsiveness. Console-only boots, DHCP-only wired boots, and Wi-Fi boots without `WIFI_OLDGOOD_REPLAY=yes` remain failed slices even when the prompt is usable.
  - When `cohsh` reaches the Pi over Wi-Fi/TCP, keep the raw serial log and the `cohsh` transcript together in the Pi 4 evidence directory. TCP `cohsh` output is not mirrored back into the UART log, so the normalizer may be run over a combined serial-plus-`cohsh` evidence file for the final `netstats`/`netstatus` assertions while retaining the raw serial log as the boot source of truth.
  - Capture boot evidence showing:
    - `manifest.hw.network.mode=<static|dhcp>`; Pi 4 manifest-default boots must show `dhcp`
    - `manifest.hw.network.interface=<wired|wifi|auto>`; Pi 4 manifest-default boots must show `auto`
    - `[net-policy] source=<manifest|dtb> ...` or `[net-policy] source=dtb rejected reason=<reason> ...`
    - explicit `wifi` boots may now emit `[net-console] pending-link backend=<driver> active=<iface> detail=wifi-associating ...` before later association / DHCP progress
    - when saved Cohesix policy exists, the U-Boot wizard defaults to `Continue with existing config`; otherwise it defaults to `Boot with manifest defaults`
    - for static boots sourced from the U-Boot wizard, `/chosen/cohesix,static-ipv4`, `/chosen/cohesix,static-prefix-len`, and optional `/chosen/cohesix,static-gateway` appear in the U-Boot handoff log
    - for DHCP boots, `[net-console] pending-dhcp ...` followed by `[dhcp] lease bound ...`; DHCP-bound evidence is address proof only, while acceptance still requires listener/command evidence (`netstatus ... tcp_ready=yes`, authenticated `cohsh`, or successful `nettest`).
    - USB cold-boot proof shows `USB_BOOTLOADER_HANDOFF_SEEN=no` and `USB_COLD_BOOT_SEEN=yes`; any U-Boot xHCI handoff, stop-seed, preserve-state, bootloader-authorized reset, or `run-uboot` label fails the Pi 4 USB gate.
    - USB keyboard proof reaches `USB_GATE=10` / `USB_BLOCKER=none` with `USB_COMMAND_READY=yes`, `USB_FIRST_REPORT_READY=yes`, `USB_LOCAL_SEAT_STATE=ready`, and `USB_BUSY_AFTER_READY=no`, and hardware acceptance also reaches `USB_OLDGOOD_REPLAY=yes` / `USB_OLDGOOD_MISSING=none` for the isolated hub-keyboard sequence before claiming the local-seat keyboard experience is complete. The first HID report and first byte must be sourced from `linked-runtime-hid`; parser ingress reported as `local-seat-queue-diagnostic`, local-seat queue text, or `source=first-byte` is diagnostic only. A printable-key line such as `runtime keyboard first-printable-byte ...` remains required user-experience evidence. Sustained USB acceptance additionally requires `USB_POST_FIRST_BYTE_BLOCKER=none`, no `recovery-failed` report status, no post-first-byte queue collapse, and no growing no-reply/runtime-skipped pressure during typing, arrow-history, and lock-key bursts.
    - if the attached keyboard exposes lock LEDs, Caps Lock, Num Lock, and Scroll Lock testing either proves the preallocated EP0 OUT DMA path (`xhci-control-out-prealloc` plus `pi4 keyboard led sync ready ...`) or cleanly logs `keyboard led sync unavailable ... action=disabled` without blocking input.
    - HDMI local-seat acceptance observes typed USB keyboard bytes echoing at parser ingress on the live prompt row, boot/progress messages refreshing at the documented 5-10 s cadence, and new output scrolling the isolated HDMI viewport like a serial terminal without full-screen blink. USB up/down arrow escape sequences navigate the bounded root-owned HDMI history and trigger cursor-home redraws from canonical scrollback; redraws must use the framebuffer-derived safe-area row count even when the payload spans multiple bounded HDMI service turns. Each rendered row must use clear-to-end-of-line and the final chunk must use clear-to-end so framebuffer-derived wide modes cannot retain stale text on the right or below the viewport. Redraws must leave the cursor at the real end of the prompt/input text, not after padding spaces, and overflow recovery must not collapse into a stale or jumbled top-of-screen block. Arrow bytes must not enter the command parser or starve ordinary keyboard bytes. Linked HDMI submit misses, ring busy states, and queue backpressure must coalesce to one pending canonical redraw and supersede stale queued bytes rather than replaying raw payload tails; a capture with repeated `hdmi-text` no-reply growth, saturated `pending_bytes`, or jumbled/repeated screen content is not HDMI acceptance even if USB reaches Gate 10. Stage 02 driver coverage guards the cadence constants, serial runtime ring RX/TX turns, HDMI prompt/input/history/no-reply behavior, and Wi-Fi progress suppression during USB boot activity and after USB first-byte proof. Pi 4 manifest-default boots must use `hw.local_seat.enabled=true`, `hw.local_seat.required=true`, and matching `usb-kbd0`/`hdmi0` `hw.devices[] required=true` declarations so missing declared devices fail visibly. Runtime backend attach failures may degrade with `required=yes action=serial-shell`; that keeps the UART root shell reachable but does not satisfy HDMI/USB acceptance.
  - `netstats` must report:
    - `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`; the normalizer exposes the selected state as `NET_ACTIVE`, `NET_ADDR_SRC`, and `NET_DHCP`, and separately exposes command/listener proof as `NET_TCP_READY` and `NETTEST_PROOF`.
    - `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`; on CYW43, `tx_complete` is credit-backed SDPCM completion proof and `tx_submit > tx_complete` is a Wi-Fi TX credit anomaly until host TCP/cohsh evidence proves the path recovered.
    - `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1>`
    - driver-task scheduling evidence for the active hardware path in reopened 26a/26b acceptance captures: contract name, service class, isolation mode, poll/service count, budget exhaustion/yield count, RX/TX queue depth, drop count, manifest-selected affinity core, observed service latency, and timer backend proof. The normalizer exposes this as `TIMER_BACKEND`, `TIMER_CLOCK_HZ`, `TIMER_EL0_COUNTER`, `DUMMY_TIMER_SEEN`, `DRIVER_TASK_CONTRACTS`, `DRIVER_TASK_DEDICATED`, `DRIVER_TASK_COMPATIBILITY`, `DRIVER_TASK_DEDICATED_READY`, `DRIVER_TASK_SERIAL_DEDICATED`, `DRIVER_TASK_USB_DEDICATED`, `DRIVER_TASK_DISPLAY_DEDICATED`, `DRIVER_TASK_NET_DEDICATED`, `DRIVER_TASK_SDIO_DEDICATED`, `DRIVER_TASK_PCIE_DEDICATED`, `DRIVER_TASK_SUBSTRATE_READY`, `DRIVER_TASK_FAILED_COUNT`, `DRIVER_TASK_CAPSET_PROOF`, `DRIVER_TASK_FAULT_PROOF`, `DRIVER_TASK_REVOKE_PROOF`, `DRIVER_TASK_SCHED_PROOF`, `DRIVER_TASK_AFFINITY_PROOF`, `DRIVER_TASK_AFFINITY_CONFIGURED`, `DRIVER_TASK_AFFINITY_APPLIED`, `DRIVER_TASK_AFFINITY_MANIFEST_PROOF`, `DRIVER_TASK_AFFINITY_MANIFEST_MATCHES`, `DRIVER_TASK_AFFINITY_MANIFEST_MISSING`, `DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES`, `DRIVER_TASK_VSPACE_PROOF`, `DRIVER_TASK_POINTER_FREE_IPC_PROOF`, `DRIVER_TASK_OWNER_STATE_PROOF`, `DRIVER_TASK_DMA_PROOFS`, `DRIVER_TASK_DMA_BLOCKER`, `PI4_RUNTIME_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF_REASON`, `PI4_RUNTIME_DMA_COUNTER_PROOF`, `DRIVER_TASK_ACTIVE_NET`, `DRIVER_TASK_BUDGET_OVERRUNS`, `DRIVER_TASK_LATENCY_PROOFS`, `DRIVER_TASK_RING_CALL_BEGIN`, `DRIVER_TASK_RING_CALL_RETURN`, `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_TIMEOUT`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, `DRIVER_TASK_RESOURCE_INIT`, `DRIVER_TASK_RESOURCE_BLOCKER`, and `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER`. `DRIVER_TASK_OWNER_STATE_PROOF=yes` must be backed by per-hot-path owner-state descriptor lines for serial, USB, HDMI, PCIe, and the selected network owner set (`cyw43-wifi` plus `sdio-host` when `DRIVER_TASK_ACTIVE_NET=cyw43`, or `genet-nic` when `DRIVER_TASK_ACTIVE_NET=genet`). Pi 4 performance evidence must report `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no`, `DRIVER_TASK_DMA_BLOCKER=none`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`; otherwise latency proof is red even if driver-task owner-state proof is present. `DRIVER_TASK_RESOURCE_BLOCKER` is the first lost resource proof in the capture; `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER` is the latest non-ready resource-init blocker. The source `DRIVER_TASK_RESOURCE_INIT` line carries the current isolated runtime owner/action, active request, `expected_request_valid` / `expected_aux0_valid`, expected aux/request values when present, same-request flag, and child progress marker needed to diagnose the live turn. Any positive `DRIVER_TASK_RING_CALL_OUTSTANDING`, `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT`, `DRIVER_TASK_BOOTSTRAP_DEFERRED`, or non-`none` resource blocker is an isolated runtime no-reply/deferred-proof frontier; raw `DRIVER_TASK_RING_CALL_TIMEOUT` counts remain diagnostic when a later return closes the same request. Contract-only root-task compatibility evidence, resource-init breadcrumbs, and declared `max_service_us` budgets are diagnostic and must not be counted as dedicated driver-task closure or latency proof.
    - driver-task counter evidence for performance triage is separate from owner-state proof. Activity-gated `DRIVER_TASK_COUNTER` lines are normalized as `DRIVER_TASK_COUNTER_SNAPSHOTS`, `DRIVER_TASK_COUNTER_INVALID`, `DRIVER_TASK_COUNTER_BUSY`, `DRIVER_TASK_COUNTER_SAME_REQUEST`, `DRIVER_TASK_COUNTER_TIMEOUTS`, `DRIVER_TASK_COUNTER_KEEP_ACTIVE`, `DRIVER_TASK_COUNTER_ABORTS`, `DRIVER_TASK_COUNTER_STAGED_BYTES`, `DRIVER_TASK_COUNTER_CACHE_OPS`, `DRIVER_TASK_COUNTER_CACHE_BYTES`, `DRIVER_TASK_COUNTER_RX_FRAMES`, `DRIVER_TASK_COUNTER_TX_FRAMES`, `DRIVER_TASK_COUNTER_RX_BYTES`, and `DRIVER_TASK_COUNTER_TX_BYTES`. Reopened 26b performance evidence must keep `DRIVER_TASK_COUNTER_INVALID=0`; empty zero-activity snapshots, truncated lines, or non-root-ring sources are invalid telemetry and must be fixed before benchmark claims.
    - CYW43 runtime RX proof must include bounded glom/data service counters: Function 2 runtime block reads, glom descriptor/subframe counts, queued/dropped frames, budget yields, and runtime RX oversize recoveries. Control-plane reply reads and runtime data/glom reads must be reported separately when diagnosing Wi-Fi latency.
    - responsiveness evidence under network load: `SERIAL_RESPONSIVE_PROOF=yes`, `USB_BURST_PROOF=yes`, `USB_BURST_DROPS=0`, `USB_POST_FIRST_BYTE_BLOCKER=none`, and `HDMI_RESPONSIVE_PROOF=yes`.
    - wired 26a closure must show `NET_ACTIVE=wired`; Wi-Fi 26b closure still requires `active=wifi`, `addr_src=dhcp-lease`, `dhcp=bound`, `eapol_secure=1`, non-zero TX/RX packet counters, and `WIFI_OLDGOOD_REPLAY=yes` / `WIFI_OLDGOOD_MISSING=none`. The Wi-Fi replay contract requires SDIO and CYW43 owner-state, isolated SDIO engine readiness, transport/firmware/Function 2 readiness, matched Linux-shaped control setup, primary join request, association plus link-up proof, explicit host-EAPOL M1/M2/M3/M4, PTK/GTK install, secure release, DHCP, nettest, and final netstats. Readiness and Function 2 proof must be positive, the primary join result must be success, nettest must report pass/success, and a condensed `join complete ... m1=yes m2=yes m3=yes m4=yes` line alone is not replay proof.
    - `netstatus: ip=<ipv4> gateway=<ipv4> src=<source> dhcp=<phase> tcp_ready=<yes|no>`
  - `nettest` refusal detail must preserve the reason when the run cannot start:
    - `detail=dhcp-pending`
    - `detail=wifi-associating`, `detail=wifi-host-eapol-pending`, `detail=wifi-host-eapol-required`, `detail=wifi-association-failed`, or `detail=wifi-link-down`
    - `detail=not-ready:<root-ep|ipc-buffer|cspace-window|bootstrap-commit>`
    - `detail=policy-disabled` or `detail=selftest-disabled` when the profile/runtime disables self-test
  - explicit `wifi` now supports both `static` and `dhcp` through the HAL-backed CYW43455 path; `auto` remains DHCP-only and single-active-interface. On the physical driver-task profile, bounded credentials select CYW43 and selected-CYW43 attach/join/runtime failure is fatal driver evidence rather than wired fallback; QEMU/host compatibility profiles may retain absent-device fallback coverage. Final 26b compatibility evidence still requires Pi 4 hardware captures proving join + DHCP and documenting which fallback profile, if any, was exercised.

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
- When Stage 05 is invoked through `scripts/ci/test_plan_run.sh`, the wrapper
  may reuse fresh Stage 03 regression evidence by passing
  `DD_REUSE_REGRESSION_BATCH_FROM=<state-dir>/qemu-regression-logs` into the
  due-diligence gate. Set `TP_STAGE5_REUSE_REGRESSION=0` to force a fresh
  regression batch inside Stage 05.
- Direct standalone `scripts/ci/due_diligence_gate.sh` remains exhaustive and
  reruns the regression batch unless `DD_REUSE_REGRESSION_BATCH_FROM` is
  supplied explicitly. `DD_REGRESSION_GROUPS` or inherited `COHSH_BATCH_GROUPS`
  values other than `all` mark the gate INCOMPLETE.
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
- `configs/generated/root_task_resolved.json` — `sha256:726441bd837cd419d81451de3e13b84ac152a9090bb47a0a66b233fc8307f315`

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
