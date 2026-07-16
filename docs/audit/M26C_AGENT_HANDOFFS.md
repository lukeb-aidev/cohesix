<!-- Author: Lukas Bower -->
<!-- Purpose: Consolidate Milestone 26c multi-agent lane handoffs and status decisions. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Agent Handoffs

Status: `WORKER-EXECUTION-REOPENED / PI4-GENET-HISTORICAL-PASS / DOC-SUITE-CORRECTED`

Milestone 26c requires multi-agent execution. This index records the lanes used
for the current run and whether their evidence is sufficient to advance the
milestone gate.

| Lane | Agent | Scope | Files Touched | Status | Handoff / Evidence |
| --- | --- | --- | --- | --- | --- |
| Runner | `019f08b4-39cf-7d51-9caa-9ff12cb09bb8` | Phase 0 target-qualified staged runner | `scripts/ci/test_plan_run.sh`, `scripts/ci/check_test_plan.sh`, `docs/TEST_PLAN.md`, `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` | PASS-contract / Stage 01 smoke closed | `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md`; `docs/audit/M26C_TARGET_RUNNER_BASELINE.md` |
| Docs provenance | `019f08b4-5355-7a82-8ad4-8ee35e5ca0a6` | Phase 1 Markdown/Mermaid/provenance inspection | none by agent | FINDINGS-RECORDED / NONBLOCKING-DEFERRED | Findings incorporated into inventory, Mermaid audit, drift ledger, and blocker ledger |
| Runtime/DMA | `019f08b4-8733-7b82-9403-acc7b40b95d0`; `019f0d04-3c17-70b1-b199-ca797fd9cf03` | Pi 4 runtime/DMA proof and DMA protection profile inspection | none by agents | PASS-PI4-GENET | Findings drove `PI4_RUNTIME_DMA_PROOF`, proof bundle, and Stage 05 Pi proof-artifact gates; final GENET proof is `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`. |
| Worker/cap/lifecycle/MCS | `019f08b4-6c0e-7c92-9fe0-4ae2051a2ce8`; `019f0d04-f56b-7440-b0e7-0316a944a221` | Worker architecture, endpoint caps, notifications, MCS evidence inspection and stale-doc closure | `M26C_RUNTIME_BOUNDARY_AUDIT.md`, `M26C_NINEDOOR_PARITY_MATRIX.md`, `M26C_REFACTOR_OWNERSHIP.md` | MODEL-ONLY / LIVE-CLOSURE-REOPENED | Historical tests characterized helper loops and generated records; they did not prove a loaded Worker TCB, live cap, notification delivery, or applied scheduling. |
| Compiler DMA profile | parent run | `m26c-dma-protection-profile-truth` | `tools/coh-rtc/src/ir.rs`, `tools/coh-rtc/src/codegen/{rust.rs,docs.rs}`, `configs/root_task*.toml`, generated artifacts, docs/audit ledgers | PASS-profile / proof-gap | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| QEMU closure/fix pass | parent run + subagents | Fresh QEMU/Pi builds, QEMU Stage 01-05 closure, and QEMU defect repair | runtime, root-task, cohsh scripts/tests, test-plan scripts/docs, audit registers | PASS-QEMU / PASS-PI4 | `out/test-plan/m26c-qemu`; `out/audit/gate/20260628T015332Z`; `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml`; final Pi closure `out/audit/gate/20260629T061204Z` |
| Pi hardware evidence | `019f104a-ea29-7500-a4cd-1eac8674ff1f` | Fresh Pi 4 serial/pcap proof-lane ledger | none by agent | PASS-BOOT-NET / DRIVER-PROOF-GREEN | Selected `/Users/lukasbower/pi4-serial-20260629-072441.log`; compared active pcap candidates and kept `/Users/lukasbower/tcpdump-usb-eth-20260629-072436.pcap` as the boot-paired evidence despite an older long-running capture having a newer mtime |
| Pi normalizer proof state | `019f104a-eaf1-7442-a9ce-feab11ee7549` | Current-state proof classifier inspection for appended Pi serial logs | none by agent | ROOT-CAUSE-IDENTIFIED / FIXED | Identified sticky DMA blocker, sticky post-first-byte USB blocker, and partial bootstrap-deferral resolution in `scripts/pi4_trace_normalize.py`; parent patch fixed and tested those cases |
| Pi WiFi-selected comparison | `019f105b-3b1c-7522-a603-7b669219e2fa` | Read-only latest WiFi-selected boot-segment blocker analysis | none by agent | ACCEPTANCE-GREEN / WIFI-EAPOL-BLOCKED | Confirmed the WiFi-selected boot later re-emitted `DRIVER_TASK_ACCEPTANCE dedicated_ready=yes`; current WiFi blocker was `host-eapol-required` / waiting keys, not driver-task owner-state or DMA |
| Pi latest evidence refresh | parent run | Fresh newest-log Pi 4 evidence after user reported Genet boot | `scripts/pi4_trace_normalize.py`, `scripts/pi4_gate_proof.sh`, `tests/test_pi4_trace_normalize.py`, `tests/test_pi4_gate_proof.py` | PASS-PI4-GENET | Newest non-empty serial is `/Users/lukasbower/pi4-serial-20260629-135454.log`; final closure uses the later wired GENET slice, paired USB-Ethernet pcap `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`, runtime/DMA proof `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`, and `192.168.10.50` TCP/REST proof. |
| Pi final closure reviewer | `019f11af-71f4-7853-a9ff-5a81e62a8d01` | Final BUILD_PLAN/audit reconciliation for Pi 4 M26c completion | none by agent | PASS-RECONCILED | Confirmed completion edits should wait for Stage 05 marker, then cite serial, pcap, runtime/DMA proof, TCP proof, Stage 03/04 logs, and Stage 05 due-diligence root. |

## README-Linked Documentation Remediation

| Lane | Agent task | Documents | Status | Handoff |
| --- | --- | --- | --- | --- |
| Core contracts | `/root/audit_core_contracts` | `ARCHITECTURE`, `INTERFACES`, `SECURE9P`, `ROLES_AND_SCHEDULING`, `DRIVERS` | PASS | Separated trust boundaries, external interfaces, protocol invariants, scheduling policy, and driver methodology; retained exact generated mirrors and passed focused `coh-rtc` guards. |
| Operator and host | `/root/audit_operator_host`; `/root/audit_operator_host/verify_operator_docs` | `USERLAND_AND_CLI`, `HOST_TOOLS`, `API_GUIDELINES`, `PYTHON_SUPPORT`, `FAILURE_MODES`, `OPERATOR_WALKTHROUGH` | PASS | Independently corrected gateway-versus-target bounds, scheduled provider coverage, filesystem-backed Python mock behavior, and local-Queen REST authority. |
| Platform and product | `/root/audit_platform_product` | `USE_CASES`, `HARDWARE_BRINGUP`, `BOOT_REFERENCE`, `GPU_NODES`, `BENCHMARKS` | PASS | Separated current QEMU evidence, historical GENET proof, current Pi revalidation, live GPU projection, simulation, and benchmark methodology. |
| Integration | `/root` | `README.md`, `BUILD_PLAN.md`, audit registers, and suite-wide validation | PASS | Resolved 502 local links across `README.md` and all 23 directly linked Markdown documents, rendered 76 of 76 Mermaid blocks, passed generated guards, Test Plan checks, metadata/H1/fence checks, and `cargo check --workspace`; corrected the OpenAPI description without changing runtime or release behavior. |
| Final semantic review | `/root/audit_core_contracts` plus evidence and operator reviewers | Cross-document status, inventories, OpenAPI, and diagram semantics | PASS after remediation | Found and closed stale Markdown inventory, unfinalized Mermaid status, hardware-proof diagram, OpenAPI placement, and 26b DPC-status defects before handoff. |
| Preservation remediation: core contracts | `/root/core_docs_restore` | `m26c-readme-linked-doc-suite-remediation`: `ARCHITECTURE`, `INTERFACES`, `ROLES_AND_SCHEDULING`, `DRIVERS`, `GPU_NODES` | PASS | Restored task-isolation semantics, the source-backed public namespace catalogue, shard derivation vector, HAL/MMIO/scheduling contracts, the release/test matrix, and the host-only GPU job schema. Driver coverage, generated drift, local-link, focused host-schema/GPU tests, and Mermaid compatibility checks passed. |
| Preservation remediation: operator and integration | `/root/operator_docs_restore` | `HOST_TOOLS`, `USERLAND_AND_CLI`, `PYTHON_SUPPORT`, `OPERATOR_WALKTHROUGH`, new `OPERATOR_RECIPES` | PASS | Restored evidence-pack, FUSE, host-ticket, federation, self-test, lifecycle, and PEFT procedures. Seventy-five focused Rust tests, 23 focused Python tests, CLI help, Markdown lint, local links, and diff checks passed. |
| Preservation remediation: platform and product | `/root/platform_docs_restore` | `HARDWARE_BRINGUP`, `BENCHMARKS`, `USE_CASES` | PASS | Restored first-boot and recovery procedures, U-Boot smoke, benchmark report/lane contracts, and six AI hive patterns. REST harness, Mermaid, CLI render, help, and diff checks passed. |
| Preservation remediation: integrated semantic review | `/root/integrated_docs_review` | Read-only cross-suite review against current source and CLI behavior | PASS-findings-closed | Caught and closed the Quickstart anchor, per-hive manifest selection in local/federated ticket recipes, executable driver build paths, planning-status sequencing, and stale link/render totals. |
| Preservation remediation: final validation | `/root/final_validation`; `/root` | Generated guards, Test Plan, driver coverage, focused contracts, target feature builds, workspace, and diff integrity | PASS | Two draft selectors were corrected rather than treated as evidence: the complete `secure9p-codec` suite passed 9 tests, and `root-task --test worker_authority` passed 6. All remaining listed gates passed. |

## Planner Decision

QEMU Phase 2 behavior-changing work is implemented and the QEMU post-behavior
baseline is frozen. The final Pi 4 decision uses the coherent wired GENET proof
chain in `out/test-plan/m26c-pi4-live`: final runtime/DMA proof, TCP `cohsh`,
REST/gateway Stage 04, and Stage 05 due diligence all pass without substituting
QEMU or stale board evidence. Broad cleanup beyond the characterized 26c waves
is deferred outside the milestone.

The 2026-07-16 seL4 15 audit supersedes only the former Worker-execution part of
that decision. QEMU helper/model tests remain useful characterization evidence,
but live Worker execution, endpoint-cap delivery, lifecycle notifications, and
applied scheduling are reopened.

## Commands Observed In Parent Run

- `scripts/ci/check_test_plan.sh` - PASS.
- `scripts/ci/test_plan_run.sh --list` - PASS.
- `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt` - PASS with 32 release-snapshot warnings.
- `npm exec --offline --package=@mermaid-js/mermaid-cli -- scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt --out out/audit/m26c-doc-remediation-mermaid-rendered` - PASS with Mermaid CLI 11.16.0; 76 source blocks produced 76 SVG files.
- `cargo test -p secure9p-codec` - PASS.
- `cargo test -p coh-rtc --test observability_docs --test cas_docs` - PASS, 6 tests.
- `cargo test -p cohsh-core --test doc_snippets` - PASS, 2 tests.
- `cargo test -p root-task --test worker_authority` - PASS, 6 tests.
- `cargo test -p hive-gateway` - PASS, 28 tests.
- `SEL4_BUILD_DIR="$PWD/seL4/SMP_build" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu` - PASS.
- `SEL4_BUILD_DIR="$PWD/seL4/build_UBOOT" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4` - PASS.
- `cargo check --workspace` - PASS.
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
- `PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof.env scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4-live --stage 5` - earlier negative gate behaved as designed; runner refused missing `out/test-plan/m26c-pi4-live/stage_01.pi4.done`.
- `SEL4_BUILD_DIR=$PWD/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4` - PASS.
- `git diff --check` - PASS.
- `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT --flash-disk /dev/disk6` - PASS rebuild/reflash; script reported `Flash complete and unmounted: /dev/disk6`.
- `scripts/check-generated.sh` - PASS after canonical artifact restoration.
- Latest Pi evidence after `/Users/lukasbower/pi4-serial-20260629-135454.log`: `python3 scripts/pi4_trace_normalize.py --gate-summary --summary /Users/lukasbower/pi4-serial-20260629-135454.log` - PASS WiFi/CYW43 summary with `WIFI_GATE=10`, `NET_ACTIVE=wifi`, `NET_DHCP=bound`, `DRIVER_TASK_ACTIVE_NET=cyw43`, `PI4_RUNTIME_DMA_PROOF=fresh-pi`, and `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`.
- `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect NET_ACTIVE=wifi --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` - PASS; wrote latest WiFi runtime/DMA artifact.
- `nc -vz -G 3 192.168.86.154 31337` and `nc -vz -G 3 192.168.86.154 31339` - earlier WiFi comparison timed out from the Mac; final closure uses the later GENET proof chain instead.
- User reported a live GENET boot after the latest captured WiFi proof; direct probes to `192.168.10.50`, `192.168.10.42`, and `192.168.10.2` on `31337` and `31339` all timed out, `arp -an` had no `192.168.10.*` Pi entry, and host `en8` remained `192.168.10.1/24`. This does not refresh the older GENET proof lane.
- `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py` - PASS, 440 tests.
- `cargo fmt --all -- --check` - PASS; `scripts/check-generated.sh` - PASS; `scripts/ci/check_test_plan.sh` - PASS; risk ratchet counts remain at or below baseline.
- `cargo clippy --workspace --all-targets -- -D warnings` - PASS.
- Final GENET proof: `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env --require-wired-ready --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=genet --expect NET_ACTIVE=wired --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` - PASS.
- `target/debug/cohsh --transport tcp --tcp-host 192.168.10.50 --tcp-port 31337 --auth-token bootstrap --role queen --script <ping/netstats>` - PASS; saved at `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt`.
- `COHSH_TCP_HOST=192.168.10.50 COHSH_TCP_PORT=31337 COHSH_AUTH_TOKEN=bootstrap PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4-live --stage 3` - PASS.
- `COHESIX_GATEWAY_URL=http://127.0.0.1:48080 HIVE_GATEWAY_REQUEST_AUTH_TOKEN=m26c-pi4-rest-token COHSH_TCP_HOST=192.168.10.50 COHSH_TCP_PORT=31337 COHSH_AUTH_TOKEN=bootstrap PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4-live --stage 4` - PASS.
- `env -u COHSH_TCP_HOST -u COHSH_HOST -u COHSH_TCP_PORT -u COHSH_AUTH_TOKEN -u COH_AUTH_TOKEN PI4_RUNTIME_DMA_PROOF_FILE=out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/m26c-pi4-live --stage 5` - PASS; due-diligence root `out/audit/gate/20260629T061204Z`.

## Residual Gaps

- No residual 26c blocker remains after final GENET Stage 03/04/05 refresh and
  Stage 05 due-diligence PASS.
- USB old-good replay remains a separate 26b/diagnostic lane and is not used as
  a substitute for M26c runtime/DMA, TCP, REST, or staged-run closure.
- Full future cap-bundle authority and broader host/root/HAL cleanup waves
  remain outside 26c unless a later milestone accepts and characterizes them.
