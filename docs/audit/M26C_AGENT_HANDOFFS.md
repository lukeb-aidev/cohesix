<!-- Author: Lukas Bower -->
<!-- Purpose: Consolidate Milestone 26c multi-agent lane handoffs and status decisions. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Agent Handoffs

Status: `QEMU-IMPLEMENTED / PI4-LATEST-WIFI-RUNTIME-DMA-PASS / TCP-REST-OPEN`

Milestone 26c requires multi-agent execution. This index records the lanes used
for the current run and whether their evidence is sufficient to advance the
milestone gate.

| Lane | Agent | Scope | Files Touched | Status | Handoff / Evidence |
| --- | --- | --- | --- | --- | --- |
| Runner | `019f08b4-39cf-7d51-9caa-9ff12cb09bb8` | Phase 0 target-qualified staged runner | `scripts/ci/test_plan_run.sh`, `scripts/ci/check_test_plan.sh`, `docs/TEST_PLAN.md`, `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` | PASS-contract / Stage 01 smoke closed | `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md`; `docs/audit/M26C_TARGET_RUNNER_BASELINE.md` |
| Docs provenance | `019f08b4-5355-7a82-8ad4-8ee35e5ca0a6` | Phase 1 Markdown/Mermaid/provenance inspection | none by agent | FAIL-readiness | Findings incorporated into inventory, Mermaid audit, drift ledger, and blocker ledger |
| Runtime/DMA | `019f08b4-8733-7b82-9403-acc7b40b95d0`; `019f0d04-3c17-70b1-b199-ca797fd9cf03` | Pi 4 runtime/DMA proof and DMA protection profile inspection | none by agents | SEMANTICS-IMPLEMENTED / PI4-LIVE-OPEN | Findings drove `PI4_RUNTIME_DMA_PROOF`, proof bundle, and Stage 05 Pi proof-artifact gates |
| Worker/cap/lifecycle/MCS | `019f08b4-6c0e-7c92-9fe0-4ae2051a2ce8`; `019f0d04-f56b-7440-b0e7-0316a944a221` | Worker architecture, endpoint caps, notifications, MCS evidence inspection and stale-doc closure | `M26C_RUNTIME_BOUNDARY_AUDIT.md`, `M26C_NINEDOOR_PARITY_MATRIX.md`, `M26C_REFACTOR_OWNERSHIP.md` | PASS-QEMU / PI4-PROOF-OPEN | QEMU stale failures were reclassified; future cap-bundle and Pi proof remain open |
| Compiler DMA profile | parent run | `m26c-dma-protection-profile-truth` | `tools/coh-rtc/src/ir.rs`, `tools/coh-rtc/src/codegen/{rust.rs,docs.rs}`, `configs/root_task*.toml`, generated artifacts, docs/audit ledgers | PASS-profile / proof-gap | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| QEMU closure/fix pass | parent run + subagents | Fresh QEMU/Pi builds, QEMU Stage 01-05 closure, and QEMU defect repair | runtime, root-task, cohsh scripts/tests, test-plan scripts/docs, audit registers | PASS-QEMU / Pi hardware pending | `out/test-plan/m26c-qemu`; `out/audit/gate/20260628T015332Z`; `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml` |
| Pi hardware evidence | `019f104a-ea29-7500-a4cd-1eac8674ff1f` | Fresh Pi 4 serial/pcap proof-lane ledger | none by agent | PASS-BOOT-NET / DRIVER-PROOF-GREEN | Selected `/Users/lukasbower/pi4-serial-20260629-072441.log`; compared active pcap candidates and kept `/Users/lukasbower/tcpdump-usb-eth-20260629-072436.pcap` as the boot-paired evidence despite an older long-running capture having a newer mtime |
| Pi normalizer proof state | `019f104a-eaf1-7442-a9ce-feab11ee7549` | Current-state proof classifier inspection for appended Pi serial logs | none by agent | ROOT-CAUSE-IDENTIFIED / FIXED | Identified sticky DMA blocker, sticky post-first-byte USB blocker, and partial bootstrap-deferral resolution in `scripts/pi4_trace_normalize.py`; parent patch fixed and tested those cases |
| Pi WiFi-selected comparison | `019f105b-3b1c-7522-a603-7b669219e2fa` | Read-only latest WiFi-selected boot-segment blocker analysis | none by agent | ACCEPTANCE-GREEN / WIFI-EAPOL-BLOCKED | Confirmed the WiFi-selected boot later re-emitted `DRIVER_TASK_ACCEPTANCE dedicated_ready=yes`; current WiFi blocker was `host-eapol-required` / waiting keys, not driver-task owner-state or DMA |
| Pi latest evidence refresh | parent run | Fresh newest-log Pi 4 evidence after user reported Genet boot | `scripts/pi4_trace_normalize.py`, `scripts/pi4_gate_proof.sh`, `tests/test_pi4_trace_normalize.py`, `tests/test_pi4_gate_proof.py` | WIFI-RUNTIME-DMA-PASS / TCP-OPEN | Newest non-empty serial is `/Users/lukasbower/pi4-serial-20260629-135454.log`; it selected WiFi/CYW43, not GENET. Runtime/DMA proof passes with six selected hot-path DMA proofs in `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env`; TCP 31337/31339 timed out from the Mac. |

## Planner Decision

QEMU Phase 2 behavior-changing work is implemented and the QEMU post-behavior
baseline is frozen. Broad Phase 4 cleanup remains constrained to QEMU-scoped
surfaces with named characterization. The latest Pi 4 boot proves WiFi/CYW43
DHCP plus runtime/DMA owner-state, but it is not a GENET boot and does not have
current TCP `cohsh` or REST proof. Full 26c closure remains blocked until one
coherent latest Pi target-qualified Stage 01-05 run passes without substituting
QEMU or stale board evidence.

## Commands Observed In Parent Run

- `scripts/ci/check_test_plan.sh` - PASS.
- `scripts/ci/test_plan_run.sh --list` - PASS.
- `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` - PASS with 32 release-snapshot warnings.
- `scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt --out out/audit/m26c-mermaid-rendered` - PASS extraction; `mmdc` unavailable.
- `cargo test -p secure9p-codec` - PASS.
- `cargo test -p coh-rtc --lib dma` - PASS.
- `scripts/check-generated.sh` - PASS.
- `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke` - PASS.
- `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` - PASS.
- `scripts/cohesix-build-run.sh --clean --transport tcp --no-run --sel4-build "$PWD/seL4/SMP_build" --out-dir out/cohesix --profile release --root-task-features cohesix-dev --cargo-target aarch64-unknown-none` - PASS fresh QEMU build.
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml` - PASS fresh Pi 4 stage-only build.
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu --stage 1` through `--stage 5` - PASS; full rerun also reported completed stages 5.
- `scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/m26c-qemu --stage 5` - PASS after ratchet/exception repair; due-diligence log root `out/audit/gate/20260628T015332Z`.
- Current edit batch adds `DRIVER_TASK_DMA_PROOF`, `PI4_RUNTIME_DMA_PROOF`, runtime/DMA proof bundles, and Pi Stage 05 proof-artifact enforcement; final validation is intentionally batched after edits.
- `python3 scripts/pi4_trace_normalize.py --gate-summary --summary /Users/lukasbower/pi4-serial-20260629-072441.log` - PASS parse after current-state normalizer fixes: `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, `DRIVER_TASK_DMA_BLOCKER=none`, `USB_POST_FIRST_BYTE_BLOCKER=none`.
- `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py` - PASS, 437 tests.
- Earlier same-image wired boot: `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-072441.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof.env --require-wired-ready --require-driver-task-proof --require-input-responsive --expect DRIVER_TASK_ACTIVE_NET=genet --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` - PASS before the minicom file accumulated the later reboot.
- Latest wired reboot: `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-072441.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof.env --require-wired-ready --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=genet --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` - PASS; wrote current live proof artifact. The stricter `--require-input-responsive` gate is not current-pass on the latest reboot until a fresh sustained USB burst proof is captured.
- `target/debug/cohsh --transport tcp --tcp-host 192.168.10.50 --tcp-port 31337 --auth-token bootstrap --role queen --script <ping/netstats>` - PASS; saved at `out/test-plan/m26c-pi4-live/cohsh-tcp-proof.txt` with `OK AUTH`, `OK ATTACH`, `OK PING`, and `tcp_auth=2`.
- `PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof.env scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4-live --stage 5` - BLOCKED as designed; runner refused missing `out/test-plan/m26c-pi4-live/stage_01.pi4.done`.
- `SEL4_BUILD_DIR=$PWD/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4` - PASS.
- `git diff --check` - PASS.
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT --flash-disk /dev/disk6` - PASS rebuild/reflash; script reported `Flash complete and unmounted: /dev/disk6`.
- `scripts/check-generated.sh` - PASS after canonical artifact restoration.
- Latest Pi evidence after `/Users/lukasbower/pi4-serial-20260629-135454.log`: `python3 scripts/pi4_trace_normalize.py --gate-summary --summary /Users/lukasbower/pi4-serial-20260629-135454.log` - PASS WiFi/CYW43 summary with `WIFI_GATE=10`, `NET_ACTIVE=wifi`, `NET_DHCP=bound`, `DRIVER_TASK_ACTIVE_NET=cyw43`, `PI4_RUNTIME_DMA_PROOF=fresh-pi`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`.
- `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect NET_ACTIVE=wifi --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` - PASS; wrote latest WiFi runtime/DMA artifact.
- `nc -vz -G 3 192.168.86.154 31337` and `nc -vz -G 3 192.168.86.154 31339` - BLOCKED; both timed out from the Mac, so latest WiFi boot does not close TCP `cohsh`, TCP smoke, REST, or full Pi Stage 01-05.
- User reported a live GENET boot after the latest captured WiFi proof; direct probes to `192.168.10.50`, `192.168.10.42`, and `192.168.10.2` on `31337` and `31339` all timed out, `arp -an` had no `192.168.10.*` Pi entry, and host `en8` remained `192.168.10.1/24`. This does not refresh the older GENET proof lane.
- `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py` - PASS, 440 tests.
- `cargo fmt --all -- --check` - PASS; `scripts/check-generated.sh` - PASS; `scripts/ci/check_test_plan.sh` - PASS; risk ratchet counts remain at or below baseline.
- `cargo clippy --workspace --all-targets -- -D warnings` - PASS.

## Residual Gaps

- Full Pi 4 Stage 01-05 closure remains pending and cannot be replaced by the
  stage-only Pi image build, standalone Stage 05 proof artifact, QEMU evidence,
  or a mixed run that combines older wired TCP/REST markers with latest WiFi
  runtime/DMA proof.
- USB old-good replay remains separate from M26c runtime/DMA closure:
  current log has `USB_OLDGOOD_REPLAY=no` and
  `USB_OLDGOOD_MISSING=root-port-live-reset` despite USB gate 10 and
  input-responsive proof passing.
- Direct TCP `cohsh` is exercised and passes on the older wired Pi 4 proof
  lane. The latest WiFi boot has no current TCP reachability from the Mac:
  TCP 31337 and 31339 timed out against `192.168.86.154`.
- A reported live GENET boot was not reachable on known wired targets
  `192.168.10.50`, `192.168.10.42`, or `192.168.10.2`; until a fresh
  non-empty serial and paired USB-Ethernet pcap prove GENET, the latest
  captured boot remains WiFi/CYW43 for closure decisions.
- The latest wired reboot has USB first-report/first-byte command-ready proof
  but not a current sustained USB burst line, so current latest-boot
  `--require-input-responsive` remains open until fresh local input is captured.
