<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c as-built blockers, owners, evidence, and closure state. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C As-Built Blockers

Status: `WORKER-EXECUTION-REOPENED / PI4-GENET-STAGE-01-05-HISTORICAL-PASS`

The seL4 15 as-built audit invalidated the former QEMU closure claims for live
Worker execution, endpoint capabilities, lifecycle notifications, and applied
Worker scheduling. The saved tests prove bounded Worker helper/model behavior
only: root-task does not load or resume a Worker TCB. The final Pi 4 hardware
closure remains historical evidence for one coherent wired GENET boot in the freshest non-empty serial log,
paired with the USB-Ethernet pcap and refreshed target-qualified Stage 03,
Stage 04, and Stage 05 markers in `out/test-plan/m26c-pi4-live`. Older WiFi and
wired observations remain comparison evidence only and are not mixed into the
final board closure.

## Current Gate Summary

| Gate | Status | Evidence |
| --- | --- | --- |
| Target-qualified runner contract | PASS | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` |
| Markdown inventory | PASS | `docs/audit/M26C_MARKDOWN_INVENTORY.csv`; diff against `git ls-files '*.md'` passed |
| Active Mermaid compatibility | PASS with release warnings | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` |
| Secure9P codec blocker probe | PASS | `cargo test -p secure9p-codec` |
| DMA protection profile truth | PASS | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| Runtime/DMA proof closure | PASS-PI4-GENET | Final boot `/Users/lukasbower/pi4-serial-20260629-135454.log` with paired USB-Ethernet pcap `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`; `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`; gate required `DRIVER_TASK_ACTIVE_NET=genet`, `NET_ACTIVE=wired`, `NET_DHCP=bound`, `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, `DRIVER_TASK_DMA_BLOCKER=none`, and arch-counter timer proof. |
| Direct TCP `cohsh` | PASS-PI4-GENET | `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt` contains `OK AUTH`, `OK ATTACH`, `OK PING reply=pong`, and GENET `netstats` for `192.168.10.50`; `nc -vz -G 3 192.168.10.50 31337` and `31339` passed. |
| Worker/cap/notification/MCS implementation | REOPENED / MODEL-ONLY | Helper loops and reserved generated metadata exist, but no Worker TCB, installed endpoint cap, delivered lifecycle notification, or applied Worker scheduling has target evidence. |
| Post-behavior baseline freeze | PI4-HISTORICAL / WORKER-CLAIMS-SUPERSEDED | QEMU Stage 01-05 and final Pi GENET Stage 01-05 proof remain recorded, but they do not close live Worker execution. |
| Full QEMU staged Test Plan | PASS | `out/test-plan/m26c-qemu` has Stage 01-05 `.done` and `.qemu.done` markers with no incomplete markers; Stage 05 evidence `out/audit/gate/20260628T015332Z` |
| Full Pi 4 staged Test Plan | PASS | `out/test-plan/m26c-pi4-live` has Stage 01-05 `.done` and `.pi4.done` markers, no incomplete markers, refreshed Stage 03/04/05 on final GENET state, REST/gateway validation, and Stage 05 due-diligence evidence at `out/audit/gate/20260629T061204Z`. |

## Blocking Items

| ID | Severity | Owner | State | Evidence Command / Source | Dependency Impact | Closure Requirement |
| --- | --- | --- | --- | --- | --- | --- |
| M26C-BLOCK-001 | P0 | runner-owner | closed | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke`; `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` | Target-qualified runner contract and Stage 01 markers exist for QEMU and Pi 4. | Closed together with M26C-BLOCK-010 after QEMU and Pi Stage 01-05 passes. |
| M26C-BLOCK-002 | P0 | runtime-dma-owner | closed | Final Pi GENET evidence `/Users/lukasbower/pi4-serial-20260629-135454.log`; `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`; `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`; `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py`; `cargo test -p pi4-driver-runtime --lib`; Stage 05 workspace tests at `out/audit/gate/20260629T061204Z/workspace-tests.log`. | Final Pi runtime/DMA, timer, wired DHCP, GENET TCP, and Stage 03/04/05 proof lanes pass for one coherent boot. | Keep WiFi and older wired evidence as comparison only; final 26c board closure uses the GENET proof chain. |
| M26C-BLOCK-003 | P0 | compiler-owner | closed | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh`; `rg -n "DmaProtectionProfile|bounded-no-iommu|smmu" tools/coh-rtc/src configs apps/root-task/src/generated docs/snippets/root_task_manifest.md` | Compiler now owns `dma.protection_profile`, but this does not close runtime proof or malicious-device DMA confinement. | Keep `none` for virt profiles, `bounded-no-iommu` for Pi-family profiles, and reject SMMU profiles until generated per-device DMA-domain state exists. |
| M26C-BLOCK-004 | P0 | worker-owner | reopened | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; source and image-packaging audit | Helper loops and some build artifacts exist, but root-task has no Worker image load/resume path. | Package each selected image, create and resume a separate Worker TCB/CSpace/VSpace, and capture QEMU plus Pi evidence. |
| M26C-BLOCK-005 | P0 | capability-owner | reopened | `cargo test -p root-task --test worker_authority`; `cargo test -p coh-rtc worker_runtime` | Current profiles disable endpoint authority; reserved badge ranges are not minted or delivered caps. | Prove a live role/epoch-badged endpoint invocation from each executable Worker task. |
| M26C-BLOCK-006 | P0 | lifecycle-owner | reopened | Worker helper tests; generated Worker metadata | Current profiles disable lifecycle notifications; helper event handling is not notification-cap delivery evidence. | Prove notification creation, cap delivery, handling, and bounded revocation/fault behavior on target. |
| M26C-BLOCK-007 | P0 | scheduling-owner | reopened-worker-evidence | `cargo test -p coh-rtc worker_runtime`; `cargo test -p root-task --test worker_authority` | The non-MCS record is configuration metadata only because no Worker TCB exists. | Prove applied priority/affinity for live Workers; MCS remains a separate profile decision requiring scheduling-context evidence. |
| M26C-BLOCK-008 | P1 | docs-owner | deferred-nonblocking | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` warnings; Stage 05 `scripts/ci/check_test_plan.sh` PASS | Release snapshot diagrams retain raw HTML labels, but release snapshots are update-by-release-flow only. | Deferred to release-cut flow; not a 26c closure blocker and not hand-edited for style. |
| M26C-BLOCK-009 | P1 | docs-owner | deferred-nonblocking | `out/audit/m26c_ai_fingerprint_rg.txt`; Stage 05 due-diligence PASS | AI-fingerprint audit retained generated, release-derived, vendored, and broader cleanup findings outside the accepted 26c refactor waves. | Deferred to future characterized cleanup waves; no behavior or closure blocker remains for 26c. |
| M26C-BLOCK-010 | P1 | validation-owner | closed | QEMU Stage 01-05 PASS in `out/test-plan/m26c-qemu`; QEMU Stage 05 due diligence PASS at `out/audit/gate/20260628T015332Z`; Pi Stage 01-05 markers in `out/test-plan/m26c-pi4-live`; final Pi GENET runtime/DMA proof `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`; final Pi Stage 05 due diligence PASS at `out/audit/gate/20260629T061204Z` | QEMU and Pi 4 target-qualified validation both have accepted evidence with no incomplete markers. | Closed. |

## Pi 4 Hardware Evidence - 2026-06-29

| Lane | Evidence | Result |
| --- | --- | --- |
| Selected boot artifacts | Serial `/Users/lukasbower/pi4-serial-20260629-135454.log`; pcap `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap` | Freshest non-empty serial selected. It contains earlier WiFi evidence and the final wired GENET boot; the final closure lane is paired with the USB-Ethernet pcap by boot time, not with older WiFi or stale wired captures. |
| Boot/timer | `python3 scripts/pi4_trace_normalize.py --gate-summary --summary /Users/lukasbower/pi4-serial-20260629-135454.log` | PASS boot: `ROOT_CONSOLE_READY=yes`, `ROOT_PROMPT_SEEN=yes`, `SERIAL_CLEAN=yes`, `BOOT_HALTED=no`, `PANIC_SEEN=no`; PASS timer: `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no` |
| GENET/DHCP | Serial summary plus USB-Ethernet pcap DHCP/ARP frames for Pi MAC `02:43:4f:48:58:31` | PASS final GENET: `DRIVER_TASK_ACTIVE_NET=genet`, `NET_ACTIVE=wired`, `NET_ADDR_SRC=dhcp-lease`, `NET_DHCP=bound`, lease `192.168.10.50/24`, gateway/server `192.168.10.1`. |
| USB/local-seat | Serial summary and Pi gate proof | PASS for 26c operator-readiness evidence: USB gate and HDMI/local-seat proof are present; strict USB old-good replay remains a separate 26b/diagnostic lane and is not required to close 26c. |
| Driver-task runtime/DMA | Serial summary and `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env` | PASS final GENET Pi: `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, `DRIVER_TASK_ACTIVE_NET=genet`, `DRIVER_TASK_DMA_PROOFS=5`, `DRIVER_TASK_DMA_BLOCKER=none`, `DRIVER_TASK_DEDICATED_READY=yes`, `DRIVER_TASK_OWNER_STATE_PROOF=yes`. |
| Direct TCP `cohsh` | `target/debug/cohsh --transport tcp --tcp-host 192.168.10.50 --tcp-port 31337 --auth-token bootstrap --role queen --script <ping/netstats>`; saved at `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt` | PASS final GENET: `OK AUTH`, `OK ATTACH`, `OK PING reply=pong`, and `netstats` shows DHCP-bound wired GENET with TCP accepts/auth. |
| TCP smoke | `nc -vz -G 3 192.168.10.50 31337`; `nc -vz -G 3 192.168.10.50 31339` | PASS final GENET: both console and TCP smoke ports accepted connections from the Mac. |
| Target-qualified Stage 03/04/05 | `out/test-plan/m26c-pi4-live/stage_03.done`, `stage_04.done`, `stage_05.done`, matching `.pi4.done` markers, logs under `out/test-plan/m26c-pi4-live/logs`, and due-diligence root `out/audit/gate/20260629T061204Z` | PASS final Pi state: Stage 03 TCP regression batch, Stage 04 REST/gateway core and parity, and Stage 05 due diligence all passed with no incomplete markers. |
| Prompt-side USB proof re-emission | `apps/root-task/src/local_seat.rs`; final rebuilt image; final Pi Stage 05 workspace tests | CLOSED for 26c: rebuilt/reflashed image was exercised through final Pi validation; proof-emission repair remains classified as proof sampling, not USB behavior expansion. |
| Fix batch validation | `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py`; `cargo test -p pi4-driver-runtime --lib`; final Stage 05 `cargo test --workspace`, `cargo audit`, `cargo deny check advisories`, `scripts/check-generated.sh`, and `scripts/ci/check_test_plan.sh` under `out/audit/gate/20260629T061204Z` | PASS: focused parser/gate tests passed, Pi runtime library tests passed, and Stage 05 due-diligence passed all required checks. |
| Rebuild/reflash | `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT --flash-disk /dev/disk6`; `scripts/check-generated.sh`; `diskutil info /dev/disk6`; `diskutil list /dev/disk6`; `out/pi4-sd/pi4-runtime-dma-proof.env` | PASS flash proof only: rebuilt root-task and isolated driver runtimes, staged image hash `473564a1cc6f7bd2ab54fef907bbd7b7d6e66a4eaaa3b06e810d1c6fe07e1f23`, runtime U-Boot image hash `003dbe9e64d4e10dc2f63952273af83c4634c9a83a72f6f0cda4ba036b5ec55f`, script reported `Flash complete and unmounted: /dev/disk6`; disk remains an unmounted `FDisk_partition_scheme` with `DOS_FAT_32 COHESIX` partition. This is not next-boot proof. |

## Non-Blocking Context

- QEMU port `31339` appears in documented QEMU self-test hostfwd paths and in
  `apps/root-task/src/net/stack.rs`; no hidden alternate in-VM service was
  established by this audit.
- HAL/MMIO searches still show legacy QEMU virtual drivers and physical serial
  code. Future structural cleanup still needs its own runtime-boundary and
  no-std/HAL gates.
- Fresh QEMU build and QEMU Stage 01-05 close the QEMU validation lane.
  Final Pi 4 GENET Stage 01-05 closes the board lane separately.
