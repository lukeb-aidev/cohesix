<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c as-built blockers, owners, evidence, and closure state. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C As-Built Blockers

Status: `QEMU-IMPLEMENTED / PI4-LATEST-WIFI-RUNTIME-DMA-PASS / TCP-REST-OPEN`

Milestone 26c QEMU implementation gaps are closed for the worker/capability,
notification, non-MCS scheduling, and QEMU validation lanes. Full milestone
closure still requires live Pi 4 Stage 01-05 evidence. The latest Pi 4 boot is
WiFi/CYW43, not GENET: it now proves DHCP plus runtime/DMA owner-state, but TCP
`cohsh`, TCP smoke, REST, and ordered full-Pi Stage 01-05 remain open for the
latest boot. Older wired/GENET proof remains comparison evidence only unless a
new coherent wired run refreshes the full stage chain.

## Current Gate Summary

| Gate | Status | Evidence |
| --- | --- | --- |
| Target-qualified runner contract | PASS | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `docs/audit/M26C_AGENT_RUNNER_HANDOFF.md` |
| Markdown inventory | PASS | `docs/audit/M26C_MARKDOWN_INVENTORY.csv`; diff against `git ls-files '*.md'` passed |
| Active Mermaid compatibility | PASS with release warnings | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` |
| Secure9P codec blocker probe | PASS | `cargo test -p secure9p-codec` |
| DMA protection profile truth | PASS | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh` |
| Runtime/DMA proof closure | PASS-PI4-LATEST-WIFI | Latest boot `/Users/lukasbower/pi4-serial-20260629-135454.log` with paired WiFi pcap `/Users/lukasbower/tcpdump-wifi-20260629-135504.pcap`; `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env`; `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect NET_ACTIVE=wifi --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` |
| Direct TCP `cohsh` | PASS-OLDER-WIRED / LATEST-WIFI-BLOCKED | Older wired evidence `out/test-plan/m26c-pi4-live/cohsh-tcp-proof.txt`; latest WiFi `nc -vz -G 3 192.168.86.154 31337` and `31339` timed out, and latest serial reports `tcp_accepts=0`, `tcp_auth=0` |
| Worker/cap/notification/MCS implementation | PASS-QEMU | Worker loops, generated endpoint badges, notification badges, and non-MCS scheduling evidence are implemented for QEMU closure |
| Post-behavior baseline freeze | QEMU-FROZEN / PI4-OPEN | QEMU post-behavior baseline can be frozen; Pi live runtime/DMA and target-qualified Stage 01-05 remain open |
| Full QEMU staged Test Plan | PASS | `out/test-plan/m26c-qemu` has Stage 01-05 `.done` and `.qemu.done` markers with no incomplete markers; Stage 05 evidence `out/audit/gate/20260628T015332Z` |
| Full Pi 4 staged Test Plan | BLOCKED-CURRENT-TCP-REST | Fresh Pi 4 image build and latest WiFi runtime/DMA proof passed, but the latest boot has no current TCP/REST reachability. Do not combine older wired Stage 03/04 evidence with the latest WiFi proof to mark Stage 05 complete. |

## Blocking Items

| ID | Severity | Owner | State | Evidence Command / Source | Dependency Impact | Closure Requirement |
| --- | --- | --- | --- | --- | --- | --- |
| M26C-BLOCK-001 | P0 | runner-owner | closed | `scripts/ci/test_plan_run.sh --list`; `scripts/ci/check_test_plan.sh`; `scripts/ci/test_plan_run.sh --target qemu --stage 1 --state-dir out/test-plan/m26c-runner-qemu-smoke`; `scripts/ci/test_plan_run.sh --target pi4 --stage 1 --state-dir out/test-plan/m26c-runner-pi4-smoke` | Target-qualified runner contract and Stage 01 markers exist for QEMU and Pi 4. | Full Stage 01-05 target runs remain part of M26C-BLOCK-010 after Phase 2 blockers close. |
| M26C-BLOCK-002 | P0 | runtime-dma-owner | closed for latest WiFi runtime/DMA / TCP-REST open | Latest Pi evidence `/Users/lukasbower/pi4-serial-20260629-135454.log`; `/Users/lukasbower/tcpdump-wifi-20260629-135504.pcap`; `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env`; `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py`; latest WiFi boot passed `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect NET_ACTIVE=wifi --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` | Latest Pi runtime/DMA, timer, WiFi DHCP, and substrate proof lanes pass. It does not prove GENET, latest TCP `cohsh`, TCP smoke, REST, USB old-good replay, or full Pi Stage 01-05. | Keep WiFi proof separate from older wired proof. Close the full Pi gate only after a coherent latest boot also passes TCP/cohsh and REST stages, or after a fresh Genet boot refreshes the full wired proof chain. |
| M26C-BLOCK-003 | P0 | compiler-owner | closed | `cargo test -p coh-rtc --lib dma`; `scripts/check-generated.sh`; `rg -n "DmaProtectionProfile|bounded-no-iommu|smmu" tools/coh-rtc/src configs apps/root-task/src/generated docs/snippets/root_task_manifest.md` | Compiler now owns `dma.protection_profile`, but this does not close runtime proof or malicious-device DMA confinement. | Keep `none` for virt profiles, `bounded-no-iommu` for Pi-family profiles, and reject SMMU profiles until generated per-device DMA-domain state exists. |
| M26C-BLOCK-004 | P0 | worker-owner | closed-QEMU | `cargo test -p worker-heart -p worker-gpu -p worker-lora`; QEMU worker-runtime code | Implemented heartbeat/GPU/LoRA worker loops replace placeholder-only QEMU behavior. | Keep worker-bus deferred and do not cite this as Pi runtime/DMA proof. |
| M26C-BLOCK-005 | P0 | capability-owner | closed-QEMU | `cargo test -p root-task --test worker_authority`; `cargo test -p coh-rtc worker_runtime` | Generated endpoint-badge authority is enforced for implemented roles. | Future full cap-bundle authority remains out of 26c QEMU scope. |
| M26C-BLOCK-006 | P0 | lifecycle-owner | closed-QEMU | Worker loop tests; `worker_authority` notification badge checks | Generated notification badges and worker-loop lifecycle events exist for QEMU. | Live Pi notification evidence and future full cap-bundle isolation are not claimed. |
| M26C-BLOCK-007 | P0 | scheduling-owner | closed-QEMU | `cargo test -p coh-rtc worker_runtime`; `cargo test -p root-task --test worker_authority` | Generated non-MCS scheduling evidence is profile-qualified and MCS claims are rejected on non-MCS profiles. | Consumed MCS budget evidence remains future/profile-specific. |
| M26C-BLOCK-008 | P1 | docs-owner | open | `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` warnings | Release snapshot diagrams retain raw HTML labels, but release snapshots are update-by-release-flow only. | Fix through release-cut flow or keep recorded as release-derived warning; do not hand-edit snapshots for style. |
| M26C-BLOCK-009 | P1 | docs-owner | open | `out/audit/m26c_ai_fingerprint_rg.txt` | AI-fingerprint audit has findings, including generic file-purpose headers and "world-class" wording. | Classify each finding as generated, accepted-specific, rewrite, delete, release-derived, vendored, or deferred before cleanup. |
| M26C-BLOCK-010 | P1 | validation-owner | open / QEMU closed / Pi latest TCP blocked | QEMU Stage 01-05 PASS in `out/test-plan/m26c-qemu`; Stage 05 due diligence PASS at `out/audit/gate/20260628T015332Z`; fresh Pi 4 stage-only image build PASS; latest WiFi runtime/DMA artifact `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env`; older wired TCP proof `out/test-plan/m26c-pi4-live/cohsh-tcp-proof.txt`; latest WiFi TCP 31337/31339 timed out | QEMU validation has accepted evidence; latest Pi runtime/DMA proof is ready for Stage 05, but latest Pi TCP/cohsh and REST stages are not current-pass. | Run Pi 4 Stage 01-05 in order against one coherent latest boot, with `PI4_RUNTIME_DMA_PROOF_FILE` pointing to that same boot's proof artifact. |

## Pi 4 Hardware Evidence - 2026-06-29

| Lane | Evidence | Result |
| --- | --- | --- |
| Selected boot artifacts | Serial `/Users/lukasbower/pi4-serial-20260629-135454.log`; pcaps `/Users/lukasbower/tcpdump-wifi-20260629-135504.pcap` and `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap` | Freshest non-empty serial selected. It is WiFi-selected; older wired `/Users/lukasbower/pi4-serial-20260629-120342.log` and `/Users/lukasbower/tcpdump-usb-eth-20260629-120338.pcap` remain comparison only. |
| Boot/timer | `python3 scripts/pi4_trace_normalize.py --gate-summary --summary /Users/lukasbower/pi4-serial-20260629-135454.log` | PASS boot: `ROOT_CONSOLE_READY=yes`, `ROOT_PROMPT_SEEN=yes`, `SERIAL_CLEAN=yes`, `BOOT_HALTED=no`, `PANIC_SEEN=no`; PASS timer: `TIMER_BACKEND=arch-counter`, `TIMER_CLOCK_HZ=54000000`, `TIMER_EL0_COUNTER=vct`, `DUMMY_TIMER_SEEN=no` |
| WiFi CYW43/DHCP | Same serial summary plus WiFi pcap DHCP frames for Pi WiFi MAC `88:a2:9e:66:59:10` | PASS latest WiFi: `WIFI_GATE=10`, `WIFI_BLOCKER=none`, `NET_ACTIVE=wifi`, `NET_ADDR_SRC=dhcp-lease`, `NET_DHCP=bound`, serial lease `192.168.86.154/24`; USB-Ethernet pcap has host ARP only. |
| USB/local-seat | Serial summary and Pi gate proof | PARTIAL on latest reboot: `USB_GATE=10`, `USB_BLOCKER=none`, `USB_POST_FIRST_BYTE_BLOCKER=none`, `HDMI_RESPONSIVE_PROOF=yes`, and first-report/first-byte command-ready lines are present. The stricter current latest-boot input gate remains open with `SERIAL_RESPONSIVE_PROOF=no`, `USB_BURST_PROOF=no`, `USB_BURST_DROPS=-1`; an earlier same-image wired boot in this file passed `SERIAL_RESPONSIVE_PROOF=yes`, `USB_BURST_PROOF=yes`, `USB_BURST_DROPS=0`. Strict USB old-good replay remains separate with `USB_OLDGOOD_REPLAY=no`. |
| Driver-task runtime/DMA | Serial summary and `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env` | PASS latest WiFi Pi: `PI4_RUNTIME_DMA_PROOF=fresh-pi`, `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, `DRIVER_TASK_ACTIVE_NET=cyw43`, `DRIVER_TASK_DMA_PROOFS=6`, `DRIVER_TASK_DMA_BLOCKER=none`, `DRIVER_TASK_BUDGET_OVERRUNS=0`, `DRIVER_TASK_BOOTSTRAP_DEFERRED=0`, `DRIVER_TASK_RESOURCE_CURRENT_BLOCKER=none` |
| Direct TCP `cohsh` | Latest WiFi `nc -vz -G 3 192.168.86.154 31337`; older wired `out/test-plan/m26c-pi4-live/cohsh-tcp-proof.txt` | LATEST BLOCKED: TCP 31337 timed out on WiFi and serial reports `tcp_accepts=0`, `tcp_auth=0`. Older wired proof remains PASS comparison only. |
| TCP smoke | Latest WiFi `nc -vz -G 3 192.168.86.154 31339` | LATEST BLOCKED: TCP 31339 timed out from the Mac. |
| Reported live GENET probe | User reported Pi 4 booted with GENET after the latest captured WiFi proof; direct probes checked `192.168.10.50`, `192.168.10.42`, and `192.168.10.2` on ports `31337` and `31339`; `arp -an` had no `192.168.10.*` Pi entry; host `en8` remained `192.168.10.1/24` | NOT CURRENT-PASS: all wired TCP probes timed out, and newest non-empty serial/paired pcaps still show WiFi/CYW43 selection. Treat older GENET proof as comparison only until a new non-empty serial plus paired USB-Ethernet pcap proves `DRIVER_TASK_ACTIVE_NET=genet` and current TCP/REST reachability. |
| Prompt-side USB proof re-emission | `apps/root-task/src/local_seat.rs`; subagent WiFi-selected segment comparison | FIXED / REBUILD-PENDING: prompt-side USB first-report owner-state registration now invokes the canonical boot-contract proof re-sample immediately, matching the sibling owner-ready path. This should reduce transient red acceptance windows after rebuild; it is not yet live hardware evidence. |
| Fix batch validation | `.venv/bin/python -m pytest tests/test_pi4_trace_normalize.py tests/test_pi4_gate_proof.py`; latest WiFi `scripts/pi4_gate_proof.sh --normalize-only --log /Users/lukasbower/pi4-serial-20260629-135454.log --runtime-dma-proof-out out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-wifi-latest.env --require-driver-task-proof --expect DRIVER_TASK_ACTIVE_NET=cyw43 --expect NET_ACTIVE=wifi --expect NET_DHCP=bound --expect ROOT_PROMPT_SEEN=yes --expect SERIAL_CLEAN=yes --expect USB_BOOTLOADER_HANDOFF_SEEN=no --expect USB_COLD_BOOT_SEEN=yes` | PASS focused parser/gate tests: 440 passed; PASS latest WiFi runtime/DMA gate; proof artifact written. Full-ready old-good replay and TCP/REST remain open. |
| Rebuild/reflash | `scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml --sel4-build-dir seL4/build_UBOOT --flash-disk /dev/disk6`; `scripts/check-generated.sh`; `diskutil info /dev/disk6`; `diskutil list /dev/disk6`; `out/pi4-sd/pi4-runtime-dma-proof.env` | PASS flash proof only: rebuilt root-task and isolated driver runtimes, staged image hash `473564a1cc6f7bd2ab54fef907bbd7b7d6e66a4eaaa3b06e810d1c6fe07e1f23`, runtime U-Boot image hash `003dbe9e64d4e10dc2f63952273af83c4634c9a83a72f6f0cda4ba036b5ec55f`, script reported `Flash complete and unmounted: /dev/disk6`; disk remains an unmounted `FDisk_partition_scheme` with `DOS_FAT_32 COHESIX` partition. This is not next-boot proof. |

## Non-Blocking Context

- QEMU port `31339` appears in documented QEMU self-test hostfwd paths and in
  `apps/root-task/src/net/stack.rs`; no hidden alternate in-VM service was
  established by this audit.
- HAL/MMIO searches still show legacy QEMU virtual drivers and physical serial
  code. These require the Phase 1 runtime-boundary and Phase 3 no-std/HAL gates
  before any structural cleanup.
- Fresh QEMU build and QEMU Stage 01-05 close the QEMU validation lane only.
  They do not satisfy Pi 4 runtime/DMA or fresh hardware proof blockers.
