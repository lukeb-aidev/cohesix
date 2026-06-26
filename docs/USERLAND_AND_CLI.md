<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe Cohesix userland command surfaces, CLI usage, and console workflows. -->
<!-- Author: Lukas Bower -->
# Cohesix Userland & CLI

## Philosophy
`cohsh` is the canonical operator shell for the entire hive: one Queen orchestrating many workers via a shared Secure9P namespace.

## At a glance
- `cohsh` is the authoritative CLI for control-plane actions and `/proc` observability.
- The root console (`cohesix>`) is for boot-time diagnostics only.
- TCP console is single-client; one of `cohsh`, `coh`, `swarmui`, or `hive-gateway` at a time.
- Policy gates may require approvals in `/actions/queue` before writes to `/queen/ctl`.
- `/gpu` and `/host` appear only after the host bridges publish.

## Overview
Cohesix userland exposes two operator entry points:
- **Root console** on the PL011 UART via QEMU `-serial mon:stdio`, showing the `cohesix>` prompt for on-box bring-up and bootinfo sanity checks.
- **Local diagnostics seat** on the Pi 4 U-Boot profile family (`pi4-uboot-aarch64` with legacy alias `uefi-aarch64`) using USB keyboard plus an optional HDMI text mirror, feeding the same root-console parser and command semantics as PL011.
- **`cohsh` host CLI** (`coh>` prompt) running on the host, speaking to the Cohesix instance over TCP (QEMU for development, Pi 4 U-Boot hardware in deployment) or the mock/QEMU transports for development. `cohsh` never executes inside the VM and follows the same pattern on physical hardware.

Use the root console for low-level validation (bootinfo, capability layout, untyped counts) and quick liveness checks. Use `cohsh` for day-to-day operator workflows and NineDoor interactions.

**Related docs**
- `docs/HOST_TOOLS.md` — host tool semantics, policy gates, and mounts.
- `docs/INTERFACES.md` — canonical control-path schemas and `/proc` nodes.
- `docs/ROLES_AND_SCHEDULING.md` — role-to-namespace rules.
- `docs/API_GUIDELINES.md` — REST gateway mapping and constraints.
- `docs/OPERATOR_WALKTHROUGH.md` — lifecycle flow and recovery.

## Operator rules of engagement
- The TCP console is single-client; do not run `cohsh`, `swarmui`, `hive-gateway`, `coh`, `gpu-bridge-host`, or `host-sidecar-bridge` concurrently.
- Policy gating (when `/policy/rules` exists) requires approvals in `/actions/queue` for writes to `/queen/ctl`.
- `--mock` uses an in-process backend and does not talk to the VM; do not mix mock and live flags in one session.
- `/gpu/*` appears only after `gpu-bridge-host --publish` runs; `/host/*` appears only after `host-sidecar-bridge` runs.

## Root Console (PL011 / QEMU Serial)
### Access and purpose
- Brought up once PL011 initialises; exposed on QEMU `-serial mon:stdio`.
- On `pi4-uboot-aarch64` (legacy alias `uefi-aarch64`), local-seat keyboard input is routed into the same parser and HDMI mirrors the same output lines with bounded truncation and bounded in-place scroll when framebuffer setup succeeds. Local-seat input remains a distinct physical-console source rather than being collapsed into UART input; switching between UART and USB keyboard clears any unfinished line on the other source, while accepted local-seat command output is still mirrored to both HDMI and the serial transcript. HDMI mailbox/framebuffer failure degrades only the mirror; USB diagnostics and keyboard probing remain available. Kernel debug-syscall text remains serial/UART only.
- Prompt: `cohesix>` from the in-kernel event-pump console loop.【F:apps/root-task/src/event/mod.rs†L2785-L2816】
- Intended for local debug/bring-up: verify seL4 bootinfo, CSpace layout, untyped enumeration, and that the root task is alive.

### Commands (current behaviour)
- `help` – list available commands.【F:apps/root-task/src/event/mod.rs†L2818-L2879】
- `bi` – bootinfo summary (node bits, empty window, IPC buffer if present).【F:apps/root-task/src/event/mod.rs†L2892-L2914】
- `caps` – key capability slots (root CNode, endpoint, UART).【F:apps/root-task/src/event/mod.rs†L2916-L2947】
- `smp [activity]` – `smp` dumps seL4 scheduler/CPU info only in debug-kernel builds (prints `ERR reason=unsupported` otherwise). In debug builds, plain `smp` preflushes pending serial output, probes each configured affinity core as a manifest assignment bucket, and emits `tasks=` with every role/driver allocated to that core before the raw seL4 debug dump. `smp activity` is always userspace-owned: it does not require kernel benchmark builds, emits no cycle claims, and reports bounded event-pump, serial, local-seat/HDMI, network, driver-contract, driver-task proof, and affinity diagnostics for the selected active runtime set. On physical Pi 4, `smp activity` filters contract, affinity, and per-core rows through the selected boot NIC policy: Wi-Fi-selected boots report serial, USB, HDMI, PCIe bus ownership, SDIO, and CYW43; wired-selected boots report serial, USB, HDMI, PCIe bus ownership, and GENET; inactive alternates such as RTL8139, Virtio, and the non-selected Pi NIC remain absent from activity rows. Repeated `smp activity` runs add an htop-ish per-core counter-delta section: each `core` row lists only active roles/drivers assigned to that core, splits pressure into `serial_drop_s`, `seat_drop_s`, `seat_no_reply_s`, `hdmi_drop_s`, and `net_drop_s`, and aggregates only userspace counters that can be attributed to that assignment bucket. `cpu_pct` remains `unavailable`; the view is activity/rates, not kernel CPU utilization. Event-pump `smp activity` lines are mirrored through the local-seat HDMI path when active and do not emit the full boot-contract proof dump; raw seL4 debug dump bodies from plain `smp` remain serial/UART only.【F:apps/root-task/src/event/mod.rs†L2064-L2200】
- `mem` – untyped cap counts with RAM vs device breakdown.【F:apps/root-task/src/event/mod.rs†L2949-L2970】
- `ping` – replies `pong` as a liveness check.【F:apps/root-task/src/event/mod.rs†L10716-L10717】
- `usb <help|status|dump-state|diag|enable-kbd|probe-kbd>` – Pi 4 USB local-seat diagnostics on the serial/local diagnostics console only, including when HDMI has degraded to headless mode. `usb status` and `usb probe-kbd` report whether the isolated local-seat backend is attached, whether keyboard polling is deferred, the current `verdict=... focus=...`, `runtime_gate ...`, `runtime_contract ...`, `linked_runtime_snapshot ...`, and `runtime_progress ...` rows. When an isolated runtime call times out after the child observed it, `usb status`, `usb diag`, and `usb probe-kbd` also emit `usb: linked_runtime_progress ...` with the marker phase, mapped 10-gate frontier, blocker, and next action. The progress cache preserves the last valid child marker across later blank retry probes, and engine-init resource validation now distinguishes descriptor identity, hot-path match, resource totals, xHCI MMIO, DMA arena, shared pages, and the USB-to-PCIe pointer-free bus link before xHCI register access can count as progress. xHCI init markers now split DCBAAP, CRCR, ERSTBA, and ERDP into low-written, high-written, and high-flushed edges and include a scratchpad-publication edge so the command output can identify an exact 64-bit base-register, posted-write-flush, or DCBAA slot-0 frontier. `usb status` reports `first_byte_source=linked-runtime-hid|local-seat-queue-diagnostic|none` so parser ingress is visible without being promoted into isolated HID acceptance proof, and `runtime_queue ... report_status=...` distinguishes idle reports, short payloads, decode failures, flexible fallback, produced bytes, and filtered keys while Gate 10 remains first-byte proof. `usb diag` is passive: it emits one `action=diag-passive` status block, skips live xHCI probing with `reason=linked-runtime-only`, and renders `usb: diag recorder=startup-blackbox ...` plus one `usb: gate <n> name=<gate> status=<pass|fail|blocked> evidence=... next=...` line for all ten gates, followed by `usb: evidence ...` and `usb: next_action=...`. `usb enable-kbd` is the persistent arming command. `usb probe-kbd` runs a bounded one-shot isolated runtime keyboard probe burst: it temporarily arms polling for the probe, restores deferred polling unless it was already enabled or the keyboard comes online, then continues only while the child USB enumeration progress marker advances or the same active request remains observable, stopping at the small prompt-side burst limit, keyboard readiness, or no usable marker. Stable prompt-slice timeout/progress/keep-active repeats keep only the first raw UART line for a stable key; phase changes remain visible, and the command transcript still carries the cached progress rows. If the current marker is an EP0 descriptor doorbell/wait, data-event, descriptor event-ring diagnostic marker, hub descriptor class-control marker, hub-port GET_STATUS doorbell/wait/data/status marker, or hub traversal marker, root keeps the same active request for separate finite descriptor or hub budgets so diagnostics can distinguish a late event, empty event slot, event-cycle mismatch, ignored event, hub set-configuration, hub descriptor, hub context, downstream port status, downstream port power/reset, fallback speed, real transfer timeout, or status no-reply; it does not construct a root-owned xHCI path or emit the retired root-side preflight/golden-path block. Stop-seed, preserve-state, bootloader-authorized reset, and U-Boot handoff evidence is diagnostic-only and is rejected by the proof loop through `USB_BOOTLOADER_HANDOFF_SEEN=yes`; it is not a fallback path. `irq27`/`IRQ 27` is the seL4 virtual-timer PPI on Pi 4 and is never reported as a USB/xHCI interrupt source; the Pi 4 trace normalizer reports such entries separately as `TIMER_IRQ27_SEEN` / `BOOT_HALTED` gate evidence.
- Current USB proof refinement: the high-BAR Pi 4 command proof publishes `CONFIG.MaxSlots`, `DCBAAP`, `CRCR`, `ERSTSZ`, `ERSTBA`, initial `ERDP` without `EHB`, scratchpad DCBAA slot 0, U-Boot's `DNCTRL=0`, and `USBCMD.RUN` on the fresh platform-reset lane with HAL-drained low-plus-high ownership writes, requires `USBSTS.HCH==0`, applies U-Boot's poll-only `IMOD=0` / `IMAN=0` state, rings doorbell 0, then proves completion only by polling the event ring for the U-Boot-shaped command-event budget while PCIe INTx/MSI/MSI-X delivery remains masked. The proof command is Enable Slot; No Op is diagnostic-only. Current-boot PSC events are preserved until after the Enable Slot doorbell, then skipped and HAL-flushed with `ERDP.EHB` inside the command wait before accepting only the matching completion. On success it records command-ring readiness and leaves cleanup to later bounded enumeration/service turns so cleanup latency cannot hide gate-4 proof. A timeout is reported as `command_probe=enable-slot-timeout` / `USB_BLOCKER=cmd-event-ring-timeout` unless the RUN store preserved stale controller command bits, in which case the normalizer reports `USB_BLOCKER=usbcmd-run-preserved-reset-bit`; neither blocker authorizes Linux-captured root-port enumeration.
- Current USB scratchpad refinement: `usb status`, `usb diag`, and `usb probe-kbd` may report `usb-scratchpad-slot0-written`, `usb-scratchpad-slot0-cleaned`, `usb-scratchpad-array-filled`, or `usb-scratchpad-array-cleaned` before root-port gate 5. These markers prove how far the isolated runtime got after gate 4 and distinguish DMA descriptor coverage from DCBAA slot-0 publication and scratchpad-array clean faults.
- Current USB proof refinement: the Pi 4 USB proof is cold-boot-only. `scripts/pi4_trace_normalize.py` emits `USB_BOOTLOADER_HANDOFF_SEEN=yes` when USB logs contain stop-seed, bootloader-owned, bootloader-authorized, seeded cold-start, preserve-state, `run-uboot`, or `uboot-first` evidence; `scripts/pi4_gate_proof.sh` rejects that by default. The normalizer also emits `USB_COLD_BOOT_SEEN=yes` only when the trace carries explicit Cohesix-owned cold-boot evidence from the boot script or runtime high-BAR cold path. A passing USB proof must therefore show `USB_BOOTLOADER_HANDOFF_SEEN=no`, `USB_COLD_BOOT_SEEN=yes`, and reach the high-BAR path through live HAL PCIe/VL805 ownership, not U-Boot xHCI handoff.
- Current USB proof refinement: the prompt-safe command proof performs bounded event-ring CPU-sync polls after doorbell 0; the U-Boot-shaped PSC lane also performs one fresh ring recovery before returning to the shell, using the same prompt-safe `CRCR` low-bit seed when no trusted snapshot or live read is available. Doorbell 0 and later endpoint doorbells now follow the May 18 working discipline inside the isolated runtime: publish the U-Boot value with DMA/store barriers and skip the toxic same-window xHCI readback. `usb status`, `usb diag`, and `usb probe-kbd` name the command-proof edge with `usb-command-proof-submit-begin`, `usb-command-proof-trb-written`, `usb-command-proof-doorbell-begin`, `usb-command-proof-doorbell-done`, `usb-command-proof-poll-begin`, `usb-command-proof-event-peek-begin`, `usb-command-proof-event-read-begin`, `usb-command-proof-event-dma-load-done`, `usb-command-proof-event-invalidate-done`, `usb-command-proof-event-read-done`, `usb-command-proof-event-slot-empty`, `usb-command-proof-event-cycle-mismatch`, `usb-command-proof-poll-pending`, `usb-command-proof-poll-ready`, or `usb-command-proof-poll-failed`, mapped to gate 4 blocker/action text. Address Device uses U-Boot's full-speed EP0 ordering (`64` first, `8` fallback), and isolated runtime descriptor diagnostics now split `usb-device-addressed`, full-speed `usb-device-descriptor-prime-*`, final `usb-device-descriptor-*`, `usb-config-descriptor-header-*`, `usb-config-descriptor-full-*`, descriptor transfer/status `*-event-slot-empty`, `*-event-cycle-mismatch`, `*-event-ignored` markers, and later HID blockers before any keyboard-ready claim. SET_CONFIGURATION is sent only after the active configuration endpoint contexts are programmed with xHCI Configure Endpoint, and HID interrupt-IN normal TRBs use U-Boot's `IOC|ISP` completion flags. PCIe INTx/MSI/MSI-X delivery remains masked, and IRQ27 remains the seL4 virtual timer, never USB progress.
- HDMI status now separates USB keyboard detection from proof of usable input. Gate 8 enumeration writes `local-seat USB keyboard detected; waiting for first key`; the exact `local-seat USB keyboard online` banner is emitted only after the existing first-byte runtime proof reaches Gate 10.
- Gate 10 is first-byte proof, not a full printable-key proof. Pi 4 local-seat HID diagnostics now emit one-shot first-non-empty-report, first-printable-byte, and first-unmapped-usage lines so USB keyboard tests can distinguish interrupt-IN delivery from printable key decode.
- Physical Pi 4 HDMI is a high-impact progress surface with bounded console/scrollback frame attempts, not a root-owned framebuffer fallback. On the owner-state path, the HDMI framebuffer descriptor is published before driver-task bootstrap, the `hdmi-text` isolated runtime receives a bounded nonblocking engine-init retry after descriptor load, and the early Cohesix HDMI frame clears the whole mapped framebuffer before drawing inside the safe text rectangle. Normal command-output and keyboard-feedback frame submits now keep the isolated display retryable after a no-reply and emit sampled raw-UART plus `/log/queen.log` `HDMI_FRAME_SUBMIT ...` summaries with reason, byte length, completion status, and cross-sink `console_seq`, `telemetry_sinks`, and `prompt_refresh` fields; detailed `HDMI_FRAME_RING ...`, `HDMI_FRAME_QUEUE ...`, `HDMI_FRAME_COUNTERS ...`, and `HDMI_FRAME_PROGRESS ...` rows are retained for pre-prompt, fatal, and sparse sustained no-reply samples, and all diagnostic rows emitted for one HDMI frame submission share the same `console_seq`. One frame timeout no longer marks the isolated display permanently failed. When driver-task pointer-free proof is present, root submits bounded isolated runtime USB local-seat service before publishing `Cohesix console ready`, records the selected Wi-Fi replay, runs the bounded replay before publishing the interactive prompt, and then publishes `Cohesix console ready` after the replay returns ready, pending, or fail-closed; if that proof is missing, it publishes the prompt with fail-closed blockers and does not start hidden post-prompt Wi-Fi replay. HDMI no longer waits for USB keyboard attach or an unproved local-seat dependency; serial remains the complete authority console if HDMI engine init or framebuffer proof is unavailable. If isolated HDMI does not reply, root does not write the HAL-mapped firmware framebuffer. USB keyboard runtime attach is pre-root only after pointer-free proof and otherwise prompt-side bounded: if the isolated USB runtime already has an active command, local-seat attach defers with one `prompt-settle attach deferred reason=usb-runtime-active` summary before retrying after the normal quiet window; otherwise local-seat first replays the deferred PCIe root descriptor, then submits one bounded USB engine-init call. Ongoing keyboard polls remain nonblocking and, on no reply, leave `[local-seat] isolated USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell` instead of holding serial behind repeated local-seat calls. Root-console startup emits raw UART `[mark] root-console.start.*` breadcrumbs when the startup banner is published; deferred Wi-Fi descriptor replay may delay `Cohesix console ready` until ready, pending, or fail-closed, while later EAPOL/DHCP settle and diagnostics remain on the serial UART so missing `/log/queen.log` or 9P access cannot hide the debug stream. After root-console handoff, serial and USB keyboard input remain concurrent physical console sources feeding the same parser when USB proof succeeds: the event pump flushes serial echo before runtime Wi-Fi work, drains USB keyboard input before and after runtime work once keyboard polling has been enabled, and yields both not-ready `wifi-host-eapol-pending` / `wifi-host-eapol-required` runtime work and ready NIC data polling to active serial input, active USB keyboard input, or pending serial output. Deferred EAPOL remains limited to one runtime Wi-Fi poll per event turn, and ready DHCP/UDP/TCP work is capped by fixed per-poll quanta so a live Wi-Fi connection cannot drain an arbitrary backlog in one compatibility turn. HID boot keyboards are programmed with idle duration `0` to avoid periodic idle-report backlog, while HID polling still drains a bounded burst of interrupt-IN reports in one pass so press/release/next-key sequences are requeued before the next event-loop turn. HDMI boot/progress lines are rate-limited to a 5-10 s visible cadence after display proof, normal glyph/scroll writes are clipped to the safe area, and network-origin TCP console output mirrors the first 16 lines to HDMI before sampling every 256th line so REST/hive-gateway bursts do not consume isolated HDMI service turns while serial and local-seat command output remain complete.
- TCP console dispatch preserves isolated runtime boundaries while copying Linux's immediate-progress rule: after each accepted network-origin command in a bounded receive burst, root runs a bounded TCP response flush before dispatching the next queued command. Data-ready Genet keeps its existing three bounded receive/dispatch rounds in one event-pump turn; data-ready CYW43 Wi-Fi may run up to six bounded rounds and a Wi-Fi-only deeper post-dispatch flush window that first services the linked runtime RX pre-poll before flushing TCP. Pre-association Wi-Fi and host-EAPOL replay keep their existing Wi-Fi-specific budgets and physical-console yield behavior.
- The isolated HDMI text renderer follows U-Boot's stream-console shape: terminal state persists across frame chunks, CSI cursor and erase sequences are parsed as a byte stream, tab stops use 8-column alignment, and framebuffer overflow scrolls by the configured 10-row batch on full-height Pi displays while clearing only the exposed text rows. Root-task scrollback and recovery redraws remain explicit snapshots; ordinary prompt, input, and command output stay incremental.
- HDMI frame diagnostic rows stay in serial and `/log/queen.log`, but post-prompt `HDMI_FRAME_*` rows are emitted with `prompt_refresh=no` so they do not redraw the interactive `cohesix>` prompt or USB input shadow. Routine successful queued-output and keyboard-scrollback rows are sampled after the initial proofs, and routine post-prompt samples emit only the `HDMI_FRAME_SUBMIT` summary. No-reply submit rows remain visible for the first misses and then at sparse sustained samples without being reset by intermittent ready frames; detailed ring, queue, counter, and child-progress rows are kept for pre-prompt, fatal, and sustained no-reply samples. `HDMI_FRAME_SUBMIT` carries `payload_sig`, `chunk_index`, `chunk_count`, and `completion_sequence`; `HDMI_FRAME_QUEUE` carries the redraw snapshot `generation`. A missed redraw chunk restarts from a fresh full-screen `ESC[H`/`ESC[J` snapshot, and content changes while a redraw is materialized supersede the stale chunk tail instead of replaying stale bytes.
- Pi 4 local-seat lock keys maintain independent Caps Lock, Num Lock, and Scroll Lock software state. When the keyboard exposes the standard HID LED output report, Cohesix updates those LEDs with a preallocated one-byte EP0 OUT DMA buffer, so lock visibility works after the xHCI runtime DMA pool is sealed. If the optional LED report is unavailable, the first failure logs `keyboard led sync unavailable ... action=disabled`; ordinary USB input and software lock state continue without retry-induced typing lag.
- `wifi <help|dump-state|probe-ht|diag|load-fw|retry>` – Pi 4 WiFi bring-up diagnostics on the serial/local diagnostics console only. `wifi dump-state` now emits a compact `golden_path ...` route summary before the existing `verdict=... focus=...` line so operators can see the strict HT/IOR2 production gate, the current step, and the next expected control-plane edge. It now also emits a single `wifi: contract ...` line that pairs the current control-plane step with the expected edge, the observed low-level state, and the blocker class, plus firmware/HT contract lines (`wifi: firmware_contract ...`, `wifi: firmware_ht_req ...`, `wifi: firmware_ht_state ...`), SDHCI evidence lines (`wifi: sdhci_contract ...`, `wifi: sdhci_live ...`, `wifi: sdhci_preserved ...`), and raw control-plane evidence (`wifi: cccr ...`, `wifi: sdio_shadow ...`, `wifi: preserved_failure ...`). Together they expose current vs preserved SDHCI transaction evidence (`cmd`, `arg`, `present-state`, `int-status`), firmware reset-vector and upload verification state, ALP/HT request values, observed `CHIPCLKCSR`, F1/F2 state, CCCR/F1 register state, backplane window bytes, and the strongest cached failure without overrunning the fixed shell line budget. `wifi diag` is the compact prompt-side view: it emits the readiness/network summary, HT-probe stage lines, and a passive `wifi: diag recorder=startup-blackbox ...` ten-gate block with `wifi: gate <n> name=<gate> status=<pass|fail|inferred|blocked> evidence=... next=...`, plus `wifi: evidence ...` and `wifi: next_action=...`. When SDIO or CYW43 isolated runtime init has not reached card/transport readiness, `wifi diag` includes retained `wifi: sdio linked_runtime_progress ...` and `wifi: cyw43 linked_runtime_progress ...` markers and maps descriptor/resource subchecks, SDIO card select, nested CYW43-to-SDIO-owner send/wait/reply/timeout edges, CYW43 F1/F2 block-size, Function 1 enable, host-clock, and backplane edges to the current Wi-Fi gate instead of reporting generic DHCP failure. The isolated CYW43 transport command advances one known-good card-select/early-transport phase per runtime turn, re-notifies the isolated SDIO owner while polling the owner completion slot, and relies on cache-coherent shared command/descriptor invalidation before the owner decodes CYW43-produced records; a missed owner edge should surface as a precise owner/card-select blocker rather than a generic CYW43 no-reply. SDIO owner terminal faults are split into bounded `wifi: evidence sdio_cmd53 ...`, `wifi: evidence sdio_status ...`, and `wifi: evidence sdio_payload ...` lines so command shape, decoded transfer reason, host status/R5/retry state, and owner-side payload fingerprint remain visible under the fixed shell line budget; the CMD53/payload lines include `effective`, `chunk_off`, and `payload_off` fields that map a Function 1 backplane failure back to the producer firmware slice. Cached CYW43 isolated runtime failures are rendered through the same block even when the HAL debug handle is unavailable, and no-reply command turns emit/cache `CYW43_DRIVER_TASK_COMMAND_NO_REPLY` with stage/op/target/payload and control cmd/id/header bounds. Transport-init no-reply is reported as a CYW43 transport frontier rather than `runtime-power-reset`; firmware-upload faults such as `0x5103` descriptor-transfer failures or `0x5329` retry exhaustion are reported as CMD53 firmware-transfer blockers rather than generic disabled-network failures; isolated runtime control frames now emit bounded begin/ready/fail markers per bootstrap command, and WPA2-PSK explicitly emits `cyw43-host-eapol required` before DHCP/data remain blocked. Gates before a fault are marked `inferred` instead of `pass` when direct CCCR/FBR, HT, backplane, EAPOL, or DHCP evidence is absent. `wifi diag` skips the long live HT re-probe once a terminal boot/control-plane failure has been preserved, and leaves full transport snapshots to `wifi dump-state`; `wifi probe-ht`, `wifi load-fw`, and `wifi retry` do not construct root-owned SDIO/CYW43 state and return a bounded `ERR WIFI ... pi4-wifi-driver-task-runtime-required` when isolated runtime evidence cannot satisfy the request. A bounded `wifi diag` / `probe-ht` pass backed by isolated runtime evidence may add `diagnostic-force-ht-*` lines after the exact strict `0x50` HT timeout shape, but that probe is diagnostic-only and does not authorize production Function 2. These commands do not extend the shared TCP/`cohsh` grammar; TCP clients still reject raw `wifi ...` lines as parse errors.
- CYW43 isolated runtime transport status labels are fail-closed: partial transport details are reported as `progress` with `result=0`, and only `DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY` is reported as `ready` with `result=1`.
- CYW43 isolated runtime command no-reply records now include request, resume count, reason, and latest progress-marker fields in addition to stage/op/target/payload and control cmd/id/header bounds. When the progress marker matches the active CYW43 request, `wifi diag` reports that child marker as the exact blocker; otherwise the no-reply remains `cyw43-runtime-command-no-reply` at the control stage rather than DHCP or host-EAPOL. Split-control reply logs add expected command/id/header/iovar, reply-match, and nonmatching/malformed counts so terminal control failures identify the CDC edge to fix.
- Pi 4 local-seat Wi-Fi boots publish only the startup banner before starting the deferred Wi-Fi replay. Root-task still records the bounded pre-root deferral decision and rejects the replay with `[net-console] deferred resume skipped reason=driver-task-net-runtime-unproved action=serial-diagnostics-only` when physical-Pi pointer-free proof is absent, but when proof is present it emits `[net-console] deferred resume scheduled reason=driver-startup-before-root-prompt action=delay-interactive-prompt`, starts SDIO bus-owner and CYW43 Wi-Fi descriptor replay with `[net-console] deferred resume reason=before-root-prompt action=start-wifi`, then prints `Cohesix console ready` / `cohesix>` after the replay returns ready, pending, or fail-closed. The TCP console attaches only after the isolated runtimes return enough proof for DHCP/data; until then serial remains authoritative and Wi-Fi work is bounded by the event-pump budgets. Prompt-side `wifi diag`, `nettest`, and `netstats` report the live deferred state while DHCP/data stay blocked until association, host-EAPOL, and runtime data proof succeed. A trace that stops at `wifi-net-console-pending-before-root-console` before the pre-prompt replay starts remains blocked and the proof loop reports `WIFI_BLOCKER=boot-waiting-for-wifi`.
- The `wifi: firmware_contract ... nvram=...` field reports the Linux-shaped normalized NVRAM upload length, not the raw captured text file length. For the Pi 4 CYW43455 capture, raw NVRAM remains 2074 bytes, the uploaded normalized payload is 1744 bytes, and the separate Broadcom tail token brings the downloaded NVRAM region to 1748 bytes.
- Current Wi-Fi card-select refinement: before CCCR/FBR evidence exists, `wifi diag` maps CYW43 transport-init progress for `cyw43-card-host-config-begin`, `cyw43-card-cmd0-begin`, `cyw43-card-cmd5-ocr-begin`, `cyw43-card-cmd5-ready-begin`, `cyw43-card-cmd3-rca-begin`, and `cyw43-card-cmd7-select-begin` to gate 2 `sdio-card-select`. Card-select fault evidence is rendered as `wifi: evidence sdio_command command=<cmd0|cmd5-ocr|cmd5-ready|cmd3-rca|cmd7-select> attempt=<n> card_bits=... stage=... detail=... result=...` so the isolated CYW43 runtime can identify the failed SDIO owner edge without constructing a root-owned SDIO probe.
- `quit` – currently prints `quit not supported on root console`; the loop continues (no session exit).【F:apps/root-task/src/event/mod.rs†L10719-L10725】

### Example boot and probe
```
[cohesix:root-task] [uart] init OK
[console] PL011 console online
cohesix> help
Commands:
  help  - Show this help
  bi    - Show bootinfo summary
  caps  - Show capability slots
  smp [activity] - Show SMP scheduler info or userspace activity
  mem   - Show untyped summary
  ping  - Respond with pong
  usb <help|status|dump-state|diag|enable-kbd|probe-kbd> - USB local-seat diagnostics (serial/local only)
  wifi <help|dump-state|probe-ht|diag|load-fw|retry> - WiFi bring-up diagnostics (serial/local only)
  quit  - Exit the console session
cohesix> bi
[bi] node_bits=12 empty=[0x0010..0x0100) ipc=0x7f000000
cohesix> caps
[caps] root=0x0001 ep=0x0002 uart=0x0003
cohesix> mem
[mem] untyped caps=16 ram_ut=14 device_ut=2
cohesix> ping
pong
cohesix> wifi diag
wifi: debug subcommand=diag action=begin profile=bounded mode=one-shot
...
OK WIFI detail=subcommand=diag scope=serial-local
```
Use this surface to confirm boot-time state before bringing up TCP or NineDoor; it is not the operator-facing control plane.

## `cohsh` Shell (Host CLI)
### What it is
- Rust CLI at `apps/cohsh`, installed to `out/cohesix/host-tools/cohsh` by the build script.【F:scripts/cohesix-build-run.sh†L402-L442】
- Pure client: runs on the host, never inside QEMU.
- Supports transports: `tcp` (primary), `rest` (hive-gateway multiplexer), `mock` (in-process NineDoor stub), `qemu` (dev convenience to spawn QEMU). Default is `tcp` when built with the TCP feature.【F:apps/cohsh/src/main.rs†L44-L132】
- The TCP console accepts a single client at a time; `cohsh`/SwarmUI take an exclusive host-side lock by default to prevent concurrent attachments.
- `tail <path> [lines]` is shared console grammar. The default tail window is 64 lines, matching the historical hardcoded behavior, and explicit counts are capped at 256 lines.
- `log` is the 64-line default tail of `/log/queen.log`. `cat /log/queen.log` and `log dump <file.txt> [--force]` read the retained `/log/queen.log` window, currently up to 2048 lines, through the active transport. `log dump` is host-only: it writes retained payload lines to a local text file, adds no root-console verb, and does not put `OK`/`END` transcript framing into the file.

### Transport selection
| Transport | Requires | Best for | Notes |
| --- | --- | --- | --- |
| `tcp` | QEMU or hardware console | Live ops | Single-client console. |
| `rest` | `hive-gateway` | Multiplexed ops | Queen-only; uses gateway REST projection. |
| `mock` | none | CI, demos | Deterministic in-process backend. |
| `qemu` | QEMU binary + artifacts | Dev convenience | Spawns QEMU and attaches. |

### CLI flags (current)
Key options from `--help`:
- `--role <role>` and `--ticket <ticket>` to auto-attach on startup.
- `--mint-ticket` to emit a host-side ticket and exit; requires `--role`, accepts `--ticket-subject` (required for worker roles), `--ticket-config` (or `COHSH_TICKET_CONFIG`) and `--ticket-secret` (or `COHSH_TICKET_SECRET`) to override config secrets.
- `--script <file>` to execute commands non-interactively.
- `--record-trace <file>` to record Secure9P frames + ACKs to a trace file (requires `--transport mock`).
- `--replay-trace <file>` to replay a trace file deterministically (requires `--transport mock`; rejects tampered traces).
- `--transport <mock|qemu|tcp|rest>` to choose backend; TCP exposes `--tcp-host` / `--tcp-port` (defaults `127.0.0.1:31337`). REST uses `--rest-url` (or `COHSH_REST_URL` / `COH_REST_URL` / `HIVE_GATEWAY_URL`) and supports `--rest-auth-token` (or `COHSH_REST_AUTH_TOKEN` / `COH_REST_AUTH_TOKEN` / `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`).【F:apps/cohsh/src/main.rs†L44-L132】
- `--mock-seed-gpu` to seed mock transport sessions with GPU namespaces (useful for mock GPU demos/scripts).【F:apps/cohsh/src/main.rs†L44-L170】
- QEMU helpers: `--qemu-bin`, `--qemu-out-dir`, `--qemu-gic-version`, `--qemu-arg` (dev/CI convenience).【F:apps/cohsh/src/main.rs†L52-L131】
- `--auth-token` forwards the TCP console authentication secret; live TCP use requires a real token from `--auth-token`, `COHSH_AUTH_TOKEN`, or `COH_AUTH_TOKEN`. Placeholder tokens such as `changeme` are rejected outside mock/test paths.
- `--policy <file>` (or `COHSH_POLICY`) selects the manifest-derived client policy TOML; `cohsh` fails fast if the policy hash mismatches compiled defaults. Defaults to `configs/generated/cohsh_policy.toml`.
- Pool sizing overrides: `--pool-control-sessions`, `--pool-telemetry-sessions` (env `COHSH_POOL_CONTROL_SESSIONS`, `COHSH_POOL_TELEMETRY_SESSIONS`).
- Retry/heartbeat overrides: `--retry-max-attempts`, `--retry-backoff-ms`, `--retry-ceiling-ms`, `--retry-timeout-ms`, `--heartbeat-interval-ms` (env `COHSH_RETRY_MAX_ATTEMPTS`, `COHSH_RETRY_BACKOFF_MS`, `COHSH_RETRY_CEILING_MS`, `COHSH_RETRY_TIMEOUT_MS`, `COHSH_HEARTBEAT_INTERVAL_MS`).
- `COHSH_CONSOLE_LOCK=0` disables the exclusive TCP console lock (debug-only; concurrent clients will churn).

Manifest-derived policy defaults are emitted by `coh-rtc` into `configs/generated/cohsh_policy.toml` and embedded into the CLI at build time. The CLI refuses to start if the policy or manifest hash drifts.

**Auth token vs ticket**
- Auth token authenticates the console session.
- Ticket authorizes the role and namespace slice during `attach`.
- A session may require both; missing either yields deterministic `ERR`.

<!-- coh-rtc:cohsh-policy:start -->
### cohsh client policy (generated)
- `manifest.sha256`: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`
- `policy.sha256`: `4a21ce01ad4af67f164620e44427daec3c92ec60273fb3f52a6fdc4f26cd3e12`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `24`
- `cohsh.tail.poll_ms_default`: `1000`
- `cohsh.tail.poll_ms_min`: `250`
- `cohsh.tail.poll_ms_max`: `10000`
- `cohsh.host_telemetry.nvidia_poll_ms`: `1000`
- `cohsh.host_telemetry.systemd_poll_ms`: `2000`
- `cohsh.host_telemetry.docker_poll_ms`: `2000`
- `cohsh.host_telemetry.k8s_poll_ms`: `5000`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`)._
<!-- coh-rtc:cohsh-policy:end -->

Manifest-derived CohClient defaults (paths and Secure9P bounds) are emitted by `coh-rtc`.

<!-- coh-rtc:cohsh-client:start -->
### cohsh client defaults (generated)
- `manifest.sha256`: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `trace.max_bytes`: `1048576`
- `client_paths.queen_ctl`: `/queen/ctl`
- `client_paths.queen_lifecycle_ctl`: `/queen/lifecycle/ctl`
- `client_paths.queen_schedule_ctl`: `/queen/schedule/ctl`
- `client_paths.queen_lease_ctl`: `/queen/lease/ctl`
- `client_paths.queen_export_ctl`: `/queen/export/ctl`
- `client_paths.policy_ctl`: `/policy/ctl`
- `client_paths.log`: `/log/queen.log`
- `telemetry_ingest.max_segments_per_device`: `4`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `telemetry_ingest.eviction_policy`: `evict-oldest`

_Generated from `configs/root_task.toml` (sha256: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`)._
<!-- coh-rtc:cohsh-client:end -->

Shared console grammar and ticket policy are emitted by `coh-rtc` from `cohsh-core` so CLI and console stay aligned.

<!-- coh-rtc:cohsh-grammar:start -->
### cohsh console grammar (generated)
- `help`
- `bi`
- `caps`
- `smp [activity]`
- `mem`
- `ping`
- `test`
- `nettest`
- `netstats`
- `reboot`
- `log`
- `cachelog [n]`
- `quit`
- `tail <path> [lines]`
- `cat <path>`
- `ls <path>`
- `echo <path> <payload>`
- `attach <role> [ticket]`
- `spawn <payload>`
- `kill <worker>`

_Generated from cohsh-core verb specs (20 verbs)._
<!-- coh-rtc:cohsh-grammar:end -->

### Lifecycle control (cohsh)
- `lifecycle <cordon|drain|resume|quiesce|reset>` — writes to `/queen/lifecycle/ctl`.
- The CLI reads `/proc/lifecycle/state` and rejects invalid transitions locally with deterministic `ERR`.
- Successful commands still flow through the append-only control file and emit `/log/queen.log` audit lines.

### Authenticated reboot
- `reboot` — requests a platform reboot and returns `OK REBOOT detail=scheduled` before the reset backend fires.
- TCP `cohsh reboot` uses the existing TCP console auth token (`--auth-token`, `COHSH_AUTH_TOKEN`, or `COH_AUTH_TOKEN`) plus an attached Queen session; no new network listener or reboot-specific secret is introduced.
- Serial/local-seat reboot requires a Queen session attached with a Queen ticket minted from the same manifest Queen secret. A bare `attach queen` remains usable for ordinary diagnostics but is refused for reboot with `ERR REBOOT reason=policy detail=secret-required`.
- On Pi 4 U-Boot profiles, authenticated reboot best-effort marks one-shot BCM2711 PM RSTS fast-boot state before arming reset, using both the existing high marker and a low marker bit that is disjoint from the Raspberry Pi firmware partition selector. Root-task orders and drains the marker write before arming reset, but PM RSTS readback is not reliable enough to gate reset on live hardware. The generated `boot.cmd` forces serial input before loading Cohesix policy or checking the marker, accepts either marker, clears both when present, and boots saved `cohesix.env` policy or manifest defaults without showing the U-Boot wizard. Cold boots still enter the splash/menu path, where the default first action continues with saved policy or manifest defaults.
- If the marker is not visible to U-Boot, the generated script emits one bounded `fast boot marker absent` line with the observed RSTS/high/low marker values before entering the normal cold-boot menu. This diagnostic does not bypass the menu and does not create persistent boot policy.
- If the selected profile has no registered platform reset backend, root-task returns `ERR REBOOT reason=policy detail=reboot-backend-unavailable` and does not alter lifecycle state.

<!-- coh-rtc:cohsh-ticket-policy:start -->
### cohsh ticket policy (generated)
- `ticket.max_len`: `224`
- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through.
- `worker-*` tickets are required; role must match and subject identity is mandatory.

_Generated from cohsh-core ticket policy._
<!-- coh-rtc:cohsh-ticket-policy:end -->

<!-- coh-rtc:ticket-quotas:start -->
### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `131072` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `1b869521f68c26d43c1ad278fbc557f2442e438ab12d443a142e53a33e4466fb`)._
<!-- coh-rtc:ticket-quotas:end -->


## coh Host Bridges (Mount / GPU / Run / Evidence / Telemetry Pull)
- Host-only CLI at `apps/coh` with subcommands `mount`, `gpu`, `run`, `evidence`, `telemetry pull`, `peft`, and `doctor`.
- `coh mount` provides a FUSE view over Secure9P namespaces; `--mock` uses an in-process NineDoor backend, while live mounts require FUSE (enabled by default on Linux builds; macOS defaults to FUSE disabled and requires `--features fuse` plus MacFUSE).
- `coh gpu` exposes list/status/lease UX over `/gpu/*` and `/queen/ctl`; `--mock` provides deterministic CI output, `--nvml` (Linux-only, feature-gated) mirrors the host NVML inventory.
- Live `/gpu/models` and `/gpu/telemetry/schema.json` require a host GPU bridge publish (`gpu-bridge-host --publish` or `coh peft import --publish`).
- `coh run` validates `/gpu/<id>/lease`, executes a host command, and appends `gpu-breadcrumb/v1` lifecycle entries to `/gpu/<id>/status` (bounded by manifest policy). Use `--receipt-out <path>` to emit a JSON receipt (ACK lines + bounded `/proc/lease/*` snapshot) suitable for audit/chargeback pipelines.
- `coh gpu lease` can emit a JSON receipt via `--receipt-out <path>` (request parameters + ACK line + bounded `/proc/lease/*` snapshot). Receipts never include console auth tokens or raw capability tickets.
- `coh evidence pack` exports a deterministic evidence directory (manifest/policy fingerprint, `bounds.json`, bounded snapshots under `proc/`, `log/`, `audit/`, `replay/`, optional `telemetry/`). The `log/queen.log` capture reads the retained log window after the run rather than tailing the 64-line default. Exported audit JSONL hashes `ticket` fields (`ticket` → `sha256:<hex>`) so packs do not leak raw capability tickets.
- `coh evidence timeline` generates `timeline.ndjson` and `timeline.md` offline from an evidence pack directory (no network access).
- `coh telemetry pull` pulls `/queen/telemetry/*` segments into host storage; resumable and idempotent (per-segment files).
- `coh peft export` pulls `/queen/export/lora_jobs/<job_id>` into host storage with bounded reads.
- `coh peft import` stages adapters into the host registry (`coh.peft.import.registry_root`); use `--publish` to refresh `/gpu/models` in the live VM via `/gpu/bridge/ctl`.
- `coh peft activate` swaps `/gpu/models/active` and records rollback state under the registry root; `coh peft rollback` reverts to the previous pointer.
- `host-ticket-agent` is a host-only worker for `/host/tickets/*`: it tails `spec`, executes allowlisted adapters, and appends bounded lifecycle receipts to `status`/`deadletter`.
- Ticket idempotency for host control is `id + idempotency_key`; terminal states (`succeeded`, `failed`, `expired`) are deduped for replay-safe reprocessing.
- `coh doctor` validates tickets, mount capability, NVML (unless `--mock`), and runtime prerequisites.
- `--rest-url` (or `COH_REST_URL` / `HIVE_GATEWAY_URL`) routes live operations through `hive-gateway` without attaching to the TCP console (queen role only). REST request-auth uses `--rest-auth-token` (or `COH_REST_AUTH_TOKEN` / `COHSH_REST_AUTH_TOKEN` / `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`). `coh mount --rest-url` is exclusive: one REST mount per gateway URL.
- TCP console auth token uses `--auth-token` (env `COH_AUTH_TOKEN`, fallback `COHSH_AUTH_TOKEN`); live mode rejects missing or placeholder values.
- Policy enforcement is manifest-driven; `COH_POLICY` (or default `configs/generated/coh_policy.toml`) must hash-match compiled defaults.

**Common live prerequisites**
- QEMU running and console reachable at `127.0.0.1:31337`.
- `gpu-bridge-host --publish` for `/gpu/*` visibility.
- `host-sidecar-bridge --watch` for `/host/*` visibility.
- `host-ticket-agent` for ticket-driven host orchestration via `/host/tickets/spec`.
- For REST mode, run `hive-gateway`, set `COH_REST_URL`, and set request-auth (`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` or tool-specific `--rest-auth-token`) so mutating writes succeed through the gateway.
- Policy approvals queued in `/actions/queue` if `/policy/rules` gates `/queen/ctl`.

PEFT registry layout (host-side, file-native):
- `<registry_root>/available/<model_id>/manifest.toml` plus staged adapter files.
- `<registry_root>/active` holds the current active model pointer.
- `<registry_root>/active_state.toml` stores `current`/`previous` for rollback.

Manifest-derived coh policy defaults are emitted by `coh-rtc`.
<!-- coh-rtc:coh-policy:start -->
### coh policy defaults (generated)
- `manifest.sha256`: `2e64f09fb17eafce52fe3e7a29fa7eb11f2299022ca7d13eabf9b31c809b4234`
- `policy.sha256`: `9465e51f6b247269539107387e570bcdc873aa03e88076fad6c710659b7728db`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `32768`
- `coh.telemetry.max_total_bytes_per_device`: `131072`
- `coh.run.lease.schema`: `gpu-lease/v1`
- `coh.run.lease.active_state`: `ACTIVE`
- `coh.run.lease.max_bytes`: `1024`
- `coh.run.breadcrumb.schema`: `gpu-breadcrumb/v1`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.run.breadcrumb.max_command_bytes`: `256`
- `coh.peft.export.root`: `/queen/export/lora_jobs`
- `coh.peft.export.max_telemetry_bytes`: `131072`
- `coh.peft.export.max_policy_bytes`: `8192`
- `coh.peft.export.max_base_model_bytes`: `1024`
- `coh.peft.import.registry_root`: `out/model_registry`
- `coh.peft.import.max_adapter_bytes`: `67108864`
- `coh.peft.import.max_lora_bytes`: `65536`
- `coh.peft.import.max_metrics_bytes`: `65536`
- `coh.peft.import.max_manifest_bytes`: `8192`
- `coh.peft.activate.max_model_id_bytes`: `128`
- `coh.peft.activate.max_state_bytes`: `4096`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
<!-- coh-rtc:coh-policy:end -->

## coh doctor
- `coh doctor` runs deterministic host checks for tickets, mount capability, NVML (unless `--mock`), and runtime prerequisites.
- On Jetson-class NVML, it falls back to CUDA discovery and reports `status=degraded backend=cuda`.
- Use `--mock` on fresh hosts to skip NVML and QEMU checks when running mock demos.

<!-- coh-rtc:coh-doctor:start -->
### coh doctor checks (generated)
- `check=policy` validates `coh_policy.toml` against manifest + policy hashes.
- `check=ticket` uses `ticket.max_len=224` and TCP policy (queen tickets optional, worker tickets required).
- `check=mount` validates allowlist under `coh.mount.root` and requires FUSE when not `--mock`.
- `check=nvml` prefers NVML when not `--mock`; Jetson-class NVML falls back to CUDA discovery.
- `check=runtime` checks `python3` and `qemu-system-aarch64` (QEMU skipped with `--mock`).
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`

_Generated by coh-rtc (sha256: `66febf7b6dae0625c6a004490655dfcea1dd5777fe6792ecf027164df8f2ab4f`)._
<!-- coh-rtc:coh-doctor:end -->

## cohesix Python client
- Python package under `tools/cohesix-py/` with filesystem (`coh mount`) and TCP console backends.
- Examples live under `tools/cohesix-py/examples/` and emit artifacts under `out/examples/`.
- Milestone 25c adds `CohesixOrchestrator` (typed schedule/lease/export/approval APIs), host integration adapters, and `cohesix-playbook` for high-impact fleet playbooks.
- Milestone 25g adds typed host-ticket helpers (`HostTicketRequest`) and RBAC->ticket K8s coexistence translation (`K8sRbacIntent`, `enqueue_k8s_rbac_tickets`).
- The client is non-authoritative: it mirrors existing console/9P semantics and enforces manifest-derived bounds.

Manifest-derived cohesix defaults are emitted by `coh-rtc`.
<!-- coh-rtc:cohesix-py:start -->
### Cohesix Python defaults (generated)
- `manifest.sha256`: `2e64f09fb17eafce52fe3e7a29fa7eb11f2299022ca7d13eabf9b31c809b4234`
- `cohesix.defaults.sha256`: `ced521e47dd16ec53301d6d0c16c0681525be03a653ef2357434a7976e338cb6`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `console.max_line_len`: `256`
- `console.max_path_len`: `96`
- `console.max_json_len`: `192`
- `console.max_echo_len`: `224`
- `telemetry_ingest.max_bytes_per_segment`: `32768`
- `telemetry_ingest.max_total_bytes_per_device`: `131072`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.peft.import.registry_root`: `out/model_registry`

_Generated by coh-rtc (sha256: `ef522e0341d65fb59287879364b7c0f066eaddb4121dc18463c8d07abd7ba07d`)._
<!-- coh-rtc:cohesix-py:end -->


## SwarmUI Desktop (Host UI)
- Host-only Tauri app at `apps/swarmui`.
- Default transport is the TCP console (`cohsh` transport); set `SWARMUI_TRANSPORT=9p` only for a configured host/in-process or profile-scoped Secure9P endpoint, or `SWARMUI_TRANSPORT=rest` for hive-gateway. This does not add an in-VM 9P/TCP listener.
- Transport endpoint is `SWARMUI_9P_HOST` / `SWARMUI_9P_PORT` (defaults `127.0.0.1:31337`) for console/9p, or `SWARMUI_REST_URL` (fallback `COH_REST_URL`) for REST. REST request-auth uses `SWARMUI_REST_AUTH_TOKEN` (fallback `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`).
- REST transport is enabled by default in SwarmUI; use `--no-default-features` to strip it and rebuild with `--features rest` when needed.
- Console auth token resolution order is `SWARMUI_AUTH_TOKEN`, `COHSH_AUTH_TOKEN`, `COH_AUTH_TOKEN`, then the queen ticket secret from `SWARMUI_TICKET_CONFIG` / `COHSH_TICKET_CONFIG` (default `configs/root_task.toml`). If the resolved token is the placeholder `changeme`, SwarmUI warns in the attach transcript, but deployments that reject insecure console auth will still refuse the session.
- SwarmUI enables CSP `script-src 'unsafe-eval'` to support the PixiJS Live Hive renderer.
- SwarmUI self-hosts a vendored Spectrum Web Components shell (`sp-theme`, `sp-button`, `sp-textfield`, `sp-picker`, `sp-divider`) so Tauri and release bundles stay offline-safe without CDN dependencies.
- Presentation-only frontend: no retries, caching policy, or background polling logic.
- SwarmUI includes an interactive cohsh console panel for live console transport and trace replay. Snapshot replay (`--replay`) disables the prompt because it only carries Hive state, not a full console transcript.
- SwarmUI help lists only the SwarmUI console commands and directs users to `cohsh` for CLI-only features.
- Offline mode reads cached CBOR snapshots from `$DATA_DIR/snapshots/` and never touches the network.
- Trace replay uses `--replay-trace <file>` (relative paths resolved under `$DATA_DIR/traces/`) and keeps the embedded console available over the trace-backed shell transport.
- `--mint-ticket` emits a host-side ticket and exits; accepts `--role`, `--ticket-subject`, `--ticket-config`, `--ticket-secret` (env `SWARMUI_TICKET_CONFIG` / `SWARMUI_TICKET_SECRET`, fallback to `COHSH_*`).
- The "Mint ticket" UI button uses the same host-only secrets and places the token into the Ticket field for reuse.
- When caching is enabled, successful panel reads persist CBOR transcripts for offline replay.

Manifest-derived SwarmUI defaults are emitted by `coh-rtc`.
<!-- coh-rtc:swarmui-defaults:start -->
### SwarmUI defaults (generated)
- `manifest.sha256`: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`
- `swarmui.defaults.sha256`: `b3f0a457e045eee5d2c489c757b22ce4b2d0e91aec44270e1d046199069c0926`
- `swarmui.ticket_scope`: `per-ticket`
- `swarmui.cache.enabled`: `false`
- `swarmui.cache.max_bytes`: `262144`
- `swarmui.cache.ttl_s`: `3600`
- `swarmui.hive.frame_cap_fps`: `30`
- `swarmui.hive.step_ms`: `16`
- `swarmui.hive.lod_zoom_out`: `0.7`
- `swarmui.hive.lod_zoom_in`: `1.25`
- `swarmui.hive.lod_event_budget`: `512`
- `swarmui.hive.snapshot_max_events`: `4096`
- `swarmui.hive.overlay_lines`: `3`
- `swarmui.hive.detail_lines`: `50`
- `swarmui.hive.line_cap_bytes`: `160`
- `swarmui.hive.per_worker_bytes`: `2048`
- `swarmui.hive.pending_lines_per_worker`: `64`
- `swarmui.hive.pending_event_cap`: `4096`
- `swarmui.hive.poll_workers_per_tick`: `32`
- `swarmui.hive.status_poll_ms`: `500`
- `swarmui.hive.degrade_pressure`: `1.0`
- `swarmui.paths.telemetry_root`: `/worker`
- `swarmui.paths.proc_ingest_root`: `/proc/ingest`
- `swarmui.paths.worker_root`: `/worker`
- `swarmui.paths.namespace_roots`: `/proc, /queen, /worker, /log, /gpu`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `3b19c14c97f59749a1b5b06dcd90af57e97f79a2c98964fe25fca9c2f41b8b65`)._
<!-- coh-rtc:swarmui-defaults:end -->

### Interactive shell surface
Startup banner and prompt:
```
Welcome to Cohesix. Type 'help' for commands.
detached shell: run 'attach <role>' to connect
coh>
```

Commands and status:
- `help` – show the command list.【F:apps/cohsh/src/lib.rs†L1125-L1162】
- `attach <role> [ticket]` / `login` – attach to a NineDoor session. Valid roles: `queen`, `worker-heartbeat`, `worker-gpu`, `worker-bus`, `worker-lora` (CLI accepts `worker` as an alias for `worker-heartbeat`); missing roles, unknown roles, too many args, or re-attaching emit errors via the parser and shell.【F:apps/cohsh/src/lib.rs†L711-L729】【F:apps/cohsh/src/lib.rs†L1299-L1317】
- `detach` – close the current session without exiting the shell (required for multi-role scripts).【F:apps/cohsh/src/lib.rs†L1244-L1255】
- `tail <path> [lines]` – stream a bounded file tail; the default is 64 lines and explicit counts are capped at 256. `log` tails `/log/queen.log` with the 64-line default. Requires attachment.【F:apps/cohsh/src/lib.rs†L1170-L1179】
- `log dump <file.txt> [--force]` – host-only dump of the retained `/log/queen.log` payload to a local `.txt` file. Refuses to overwrite unless `--force` is supplied. Run after benchmarks rather than during measured loops.
- `ping` – reports attachment status; errors when detached or when given arguments.【F:apps/cohsh/src/lib.rs†L1181-L1194】
- `test [--mode <quick|full|smp>] [--json] [--timeout <s>] [--no-mutate]` – run the in-session self-tests sourced from `/proc/tests/` (default mode `quick`, default timeout 30s, hard cap 120s). `--no-mutate` skips spawn/kill steps. When `--json` is supplied, emit the stable schema described below.【F:apps/cohsh/src/lib.rs†L1512-L1763】
  - Note: the bundled self-test scripts end with `quit`; interactive `cohsh` reattaches to the last session when possible, while `--script` runs remain detached and require a fresh `attach`.
- `pool bench <k=v...>` – run the pooled throughput benchmark and retry/exhaustion checks; options include `path`, `ops`, `batch`, `payload`, `payload_bytes`, `delay_ms`, `inject_failures`, `inject_bytes`, `exhaust`, `kind`.
  - On TCP console transports, throughput is informational only; readback uses write acknowledgements (CAT is skipped) and line-length limits apply before `payload_bytes`.
- `echo <text> > <path>` – append a newline-terminated payload to an absolute path via NineDoor.【F:apps/cohsh/src/lib.rs†L1211-L1222】【F:apps/cohsh/src/lib.rs†L1319-L1332】
- Control-plane JSONL is appended with `echo` (strict JSON; unknown fields rejected). Examples:
  - `echo {"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120} > /queen/schedule/ctl`
  - `echo {"op":"grant","id":"lease-1","subject":"queen","resource":"gpu0","ttl_s":300,"priority":5} > /queen/lease/ctl`
  - `echo {"op":"open","id":"export-1","ttl_s":900} > /queen/export/ctl`
  - `echo {"op":"apply","id":"rev-2026-02-03","sha256":"<64-hex>"} > /policy/ctl`
- `ls <path>` – list directory entries; entries are newline-delimited and returned in lexicographic order.
- `cat <path>` – bounded read of file contents. `cat /log/queen.log` exports the retained log window, unlike `tail /log/queen.log`, which remains bounded to the default or requested tail count.
  - Common observability reads: `/proc/root/*` (reachability/cut), `/proc/9p/session/active` (session summary), `/proc/pressure/*` (refusal counters), `/proc/ingest/*` (ingest stats), `/proc/schedule/*` and `/proc/lease/*` (queue/lease snapshots).
- `spawn <role> [opts]` – queue a worker spawn via `/queen/ctl` (e.g. `spawn heartbeat ticks=100`, `spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120`).
- `kill <worker_id>` – queue a worker termination via `/queen/ctl`.
- `bind <src> <dst>` – bind a canonical namespace path to a session-scoped mount point via `/queen/ctl`.
- `mount <service> <path>` – mount a named service namespace via `/queen/ctl`.
- `telemetry push <src_file> --device <id>` – request an OS-named segment under `/queen/telemetry/<device_id>/seg/` and append bounded telemetry records using `cohsh-telemetry-push/v1` envelopes (UTF-8, allowlisted extensions only; chunked to `max_record_bytes=4096` and `telemetry_ingest.max_bytes_per_segment`).
- `quit` – prints `closing session` and exits the shell loop.【F:apps/cohsh/src/lib.rs†L1250-L1252】
- Attachments are designed so a single queen session (interactive or scripted) can drive orchestration for many workers without switching tools.

Attachment semantics:
- No role argument → `attach requires a role`.
- Unknown role string → `unknown role '<x>'`.
- More than two args → `attach takes at most two arguments: role and optional ticket`.
- Attempting a second attach without quitting → `already attached; run 'quit' to close the current session`.【F:apps/cohsh/src/lib.rs†L711-L717】

Connection handling (TCP transport):
- Successful connect logs `[cohsh][tcp] connected to <host>:<port> (connects=N)` before presenting the prompt.【F:apps/cohsh/src/transport/tcp.rs†L54-L60】
- Disconnects log `[cohsh][tcp] connection lost: …` and trigger reconnect attempts with incremental back-off, emitting `[cohsh][tcp] reconnect attempt #<n> …`. The shell remains usable in interactive mode; in `--script` mode errors propagate and stop the run.【F:apps/cohsh/src/transport/tcp.rs†L63-L73】

### Acknowledgements and heartbeats
- The root-task event pump emits `OK <VERB> [detail]` or `ERR <VERB> reason=<busy|quota|cut|policy> [detail=<...>]` for every console command, sharing one dispatcher across serial and TCP so both transports see the same lines before any payload (for example, `OK TAIL path=…` precedes streamed data).【F:apps/root-task/src/event/mod.rs†L1000-L1018】
- `PING` always yields `PONG` without affecting state, keeping automation healthy when idle, while TCP adds a 15-second heartbeat cadence on top of the shared grammar so the client can detect stalls without blocking serial progress.【F:apps/root-task/src/event/mod.rs†L1170-L1183】【F:apps/cohsh/src/transport/tcp.rs†L21-L24】
- Interactive `cohsh` sessions send periodic silent `PING` keepalives while idle to avoid TCP console inactivity timeouts; acknowledgements are drained and not echoed at the prompt.【F:apps/cohsh/src/lib.rs†L1046-L1955】
- `cohsh` parses acknowledgement lines using a shared helper, surfaces details inline with shell output, and preserves the order produced by the root-task dispatcher so scripted `attach`/`tail`/`log` flows match serial transcripts byte-for-byte.【F:apps/cohsh/src/proto.rs†L5-L44】【F:apps/cohsh/src/lib.rs†L1031-L1044】

### Script mode
`--script <file>` feeds newline-delimited commands; blank lines and lines starting with `#` are ignored. Errors abort the script and bubble up as a non-zero exit.【F:apps/cohsh/src/lib.rs†L732-L763】

## coh scripts (.coh)
### Purpose
- `.coh` is a deterministic, line-oriented scripting format for running `cohsh` command sequences non-interactively (including `coh> test` regression suites) using the exact same command handlers as the interactive `coh>` prompt.
- Lifecycle commands (`lifecycle cordon|drain|resume|quiesce|reset`) are valid script lines and apply the same local validation as interactive use.

### Non-goals
- No general-purpose shell.
- No variables, loops, branching, includes, macros, or dynamic loading.
- No network fetch of scripts at runtime.
- Not intended as a programming language—only a deterministic batch format for `cohsh` commands plus assertions.

### Execution model
- Scripts run against the current `cohsh` session (already connected); the session is expected to be `AUTH`’d and `ATTACH`’d. Scripts (and `coh> test`) may validate session state and fail fast if invalid.
- Each command line executes exactly as if typed at the `coh>` prompt (identical parsing and handlers, no special RPC path).
- Execution is strict: on the first command failure or failed `EXPECT`, stop immediately and return `FAIL`.
- On failure, report the failing line number, the command text, and the last command response line.

### Syntax
- One statement per line; blank lines are ignored.
- `#` starts a comment to end of line.

Two statement families:

1. **Command line**
   - Any line that does not start with `EXPECT` is interpreted as a `cohsh` command exactly as typed at `coh>`.

2. **Assertion line**
   - Assertions apply only to the **last executed command** and evaluate against the **last command response line** (single line as emitted by `cohsh` for that command).
   - `EXPECT OK` — last command response line must begin with `OK`.
   - `EXPECT ERR` — last command response line must begin with `ERR`.
   - `EXPECT SUBSTR <text>` — last command response line must contain `<text>` as a substring (case-sensitive).
   - `EXPECT NOT <text>` — last command response line must not contain `<text>`.

An optional control statement is provided for bounded waits: `WAIT <ms>` pauses locally (does not issue a server command) for the requested duration.

For streaming commands, the “response line” is the initial acknowledgement line (`OK …` or `ERR …` that starts the stream), not any subsequent streamed payload lines.

### Determinism & bounds
- Max script lines: 256; longer scripts are rejected.
- Max execution time: bounded by `test --timeout`; scripts must not block indefinitely.
- Explicit waiting is allowed via `WAIT <ms>` (line statement), capped at 2000 ms; longer waits are rejected.

### Preinstalled self-test scripts
`coh> test` reads `.coh` scripts from `/proc/tests/`:
- `/proc/tests/selftest_quick.coh`
- `/proc/tests/selftest_full.coh`
- `/proc/tests/selftest_negative.coh`
- `/proc/tests/selftest_smp.coh`
- Operators must rerun this suite whenever console handling, Secure9P transport, namespace structure, or access policies change.

### `coh> test` JSON schema
When invoked with `--json`, `coh> test` emits:
```
{
  "ok": true,
  "mode": "quick",
  "elapsed_ms": 123,
  "checks": [
    {"name": "preflight/ping", "ok": true, "detail": "OK ping"},
    {"name": "line 4: cat /proc/boot", "ok": true, "detail": "OK"}
  ],
  "version": "1"
}
```

### Security posture
- Scripts do not grant privileges: all actions remain subject to the session’s attached role/ticket and server-side access policy; scripts only automate what an operator could type interactively.

### Examples
Quick check (ping, proc read, and an expected error):
```
# connectivity and auth sanity
ping
EXPECT OK
cat /proc/queen/state
EXPECT OK
echo forbidden > /queen/ctl
EXPECT ERR
```

Disposable worker lifecycle with ID assertion:
```
spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=1 ttl_s=60
EXPECT OK
ls /shard
EXPECT OK
EXPECT SUBSTR path=/shard
tail /shard/<label>/worker/worker-123/telemetry
EXPECT OK
WAIT 500
kill worker-123
EXPECT OK
EXPECT NOT ERR
```

## End-to-End Workflow: QEMU + `cohsh` over TCP
This section covers the development harness for running Cohesix on QEMU. The current physical bring-up target is Raspberry Pi 4 through `Pi firmware -> U-Boot -> seL4 image -> root-task`; UEFI/AWS targets are profile-scoped work only when admitted by `docs/BUILD_PLAN.md`.
### Terminal 1 – build and boot under QEMU
Run the build wrapper to compile components, stage host tools, and launch QEMU with PL011 serial plus a user-mode TCP forward to `127.0.0.1:<port>`:
```
SEL4_BUILD_DIR="$PWD/seL4/SMP_build" ./scripts/cohesix-build-run.sh \
  --sel4-build "$PWD/seL4/SMP_build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```
Use `--sel4-build "$PWD/seL4/build"` to target the single-core baseline (keeps SMP artifacts separate).
The script builds `root-task` with the serial and TCP console features, compiles NineDoor and workers, copies host tools (`cohsh`, `gpu-bridge-host`, `host-sidecar-bridge`) into `out/cohesix/host-tools/`, and assembles the CPIO payload.【F:scripts/cohesix-build-run.sh†L369-L454】【F:scripts/cohesix-build-run.sh†L402-L442】
QEMU runs with `-serial mon:stdio` and a user-net device that forwards TCP/UDP ports 31337–31339 into the guest so the TCP console and self-tests are reachable from the host.【F:scripts/cohesix-build-run.sh†L518-L553】 The wrapper selects the NIC backend from the root-task features: `dev-virt` (via `cohesix-dev`) uses virtio-net by default, which adds `-global virtio-mmio.force-legacy=false` for the modern header; removing `net-backend-virtio` switches the wrapper to RTL8139 instead.【F:scripts/cohesix-build-run.sh†L518-L553】 The script prints the ready command for `cohsh` once QEMU is live.【F:scripts/cohesix-build-run.sh†L546-L553】 On Pi 4 U-Boot profiles, backend/address selection is policy-driven (`hw.network.mode`, `hw.network.interface`, `hw.network.static_ipv4`, `hw.network.dhcp`) and the active wired address replaces QEMU hostfwd defaults.

### Network self-test (`nettest` / `netstats`)
- Console grammar is unchanged across profiles; `nettest` and `netstats` names/ACK-ERR-END behavior remain stable.
- `nettest` refusal details are deterministic when the stack cannot start a run: `detail=dhcp-pending`, `detail=wifi-associating`, `detail=wifi-host-eapol-pending`, `detail=wifi-host-eapol-required`, `detail=wifi-association-failed`, `detail=wifi-link-down`, `detail=not-ready:<root-ep|ipc-buffer|cspace-window|bootstrap-commit>`, `detail=policy-disabled`, or `detail=selftest-disabled`.
- `netstats` reports deterministic target fields: `backend=<label> enabled=<bool> running=<bool> udp=<ip:port> tcp=<ip:port> last=<result>`.
- QEMU behavior is unchanged (`127.0.0.1:{31338,31339}` hostfwd workflows remain valid).
- Pi 4 `pi4-uboot-aarch64` uses the active control-plane address (wired GENETv5 or Wi-Fi CYW43455) and reports `backend=bcmgenet-v5` because the compiler-visible Pi 4 backend owns both interface choices.
- `netstats` emits a policy line: `mode=<off|static|dhcp> policy=<wired|wifi|auto> active=<iface> standby=<iface|none> addr_src=<source> ip=<ipv4> gateway=<ipv4> dhcp=<phase>`.
- `netstats` separates TCP establishment from remote shell proof:
  `tcp_accepts=<count>` counts TCP Established and `tcp_auth=<count>` counts
  authenticated `cohsh` sessions. The same line includes shared TCP-console
  receive counters: `tcp_recv_ready=<count>` records receive-ready turns and
  `tcp_recv_budget_hits=<count>` records turns that exhausted the bounded
  receive budget.
- `netstats` emits post-dispatch TCP response-flush counters:
  `tcp_post_flush_polls=<count>` records bounded flush polls after
  network-origin commands, and `tcp_post_flush_exhaustions=<count>` records
  batches that still had TCP work at the flush cap.
- `netstats` emits local-seat HDMI pressure counters for remote console output:
  `local_seat_net_mirror=<count>` records network-origin lines accepted for
  best-effort HDMI mirroring, `local_seat_net_mirror_suppressed=<count>`
  records lines sent to TCP but skipped for HDMI because display work was
  already pressured, and the same line reports bounded HDMI pending/no-reply
  counters when a local seat is attached.
- `netstats` emits driver TX and ARP edge counters:
  `tx_submit=<count> tx_complete=<count> tx_free=<count> tx_in_flight=<count> tx_double_submit=<count> tx_zero_len_attempt=<count> arp_rx=<count> arp_tx=<count>`.
- `netstats` emits a Wi-Fi/EAPOL line: `wifi_assoc=<0|1> wifi_link=<0|1> eapol_rx=<count> eapol_start=<count> eapol_secure=<0|1> wifi_rxq_cur=<count> wifi_rxq_hwm=<count> wifi_rxq_drops=<count>`.
- `netstats` also emits a compact status line for wrapped serial consoles: `netstatus: ip=<ipv4> gateway=<ipv4> src=<source> dhcp=<phase>`.
- Only one control-plane interface is active at a time. The current as-built runtime supports `wired` over GENETv5, `wifi` over CYW43455, and `auto` with deterministic Wi-Fi-first fallback to wired when CYW43455 attach/join setup fails before DHCP ownership transfers to the active Wi-Fi stack; historical Milestone 26b compatibility evidence is recorded in `docs/audit/M26B_COMPLETION_EVIDENCE.md`.
- On QEMU hostfwd/tunnel flows, capture self-test traffic on `lo0`.
- On Pi 4 direct-link flows, capture on the host's physical interface (for example `en8`), not `lo0`; `nettest` logs the peer-side `nc` commands that must be run from the host to exercise the UDP echo and TCP smoke sockets.

### Virtio-MMIO modes (when `net-backend-virtio` is enabled)
- **Modern v2 (default for virtio)**: no extra flags are required; the build wrapper forces `virtio-mmio.force-legacy=false` so QEMU exposes the modern header and the driver accepts it by default.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L118-L157】 Use the host forwards above to reach the TCP console (31337), UDP echo self-test (31338), and TCP smoke test (31339).
- **Legacy v1 (only for debugging)**: export `VIRTIO_MMIO_FORCE_LEGACY=1` before invoking the script **and** rebuild with `--features virtio-mmio-legacy`. The wrapper will switch QEMU to `-global virtio-mmio.force-legacy=true`; the driver will reject v1 unless the feature gate is enabled.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L1379-L1411】 When debugging legacy, prefer bumping QEMU back to modern instead of carrying the feature in normal builds.

### Verify the modern TCP path quickly
- Start QEMU with the default `--transport tcp` flow above (virtio-net backend).
- From the host, attach to the TCP console via `./cohsh --transport tcp --tcp-port 31337`.
- Observe forwarded packets (helpful on macOS `lo0`): `sudo tcpdump -i lo0 -n tcp port 31337 or udp port 31338 or tcp port 31339`.
- For smoke testing, send UDP to 31338 or TCP to 31339 and confirm traffic crosses the hostfwd path.

### Terminal 2 – host `cohsh` session over TCP
From `out/cohesix/host-tools/`:
```
./cohsh --transport tcp --tcp-port 31337
Welcome to Cohesix. Type 'help' for commands.
detached shell: run 'attach <role>' to connect
coh> attach queen
[console] OK ATTACH role=Queen session=1
attached session SessionId(1) as Queen
coh>
```
Use `log` to stream the default 64-line `/log/queen.log` tail, `ping` for health, and `tail <path> [lines]` for ad-hoc bounded inspection. If the TCP session resets, `cohsh` reports the error and continues in a detached state; reconnects are attempted automatically with back-off in interactive mode.【F:apps/cohsh/src/transport/tcp.rs†L54-L73】

## Scripted Sessions with `--script`
Example script (`queen.coh`):
```
# Attach and tail the queen log
attach queen
log
quit
```
Run via `./cohsh --transport tcp --tcp-port 31337 --script queen.coh`. The runner stops on the first error (including connection failures) and propagates the error code to the host shell.【F:apps/cohsh/src/lib.rs†L732-L763】
Use `./cohsh --check <script.coh>` to validate `.coh` syntax without executing commands.【F:apps/cohsh/src/main.rs†L28-L138】

## GUI clients
- A host-side WASM GUI is planned as a hive dashboard. It will speak the same console/NineDoor protocol as `cohsh` (no new verbs, no new in-VM endpoints) and focuses on presentation and workflow rather than new privileges.

## Debugging TCP Console Issues
- **Connection refused / wrong port**: confirm QEMU launched with `--transport tcp` and the `hostfwd` rule; the build script prints the expected port.【F:scripts/cohesix-build-run.sh†L521-L553】
- **Connection reset by peer**: `cohsh` logs the reset and reconnect attempts. Re-run `attach <role>` once the console listener is reachable.【F:apps/cohsh/src/transport/tcp.rs†L63-L73】
- **Authentication failures**: ensure `--auth-token`, `COHSH_AUTH_TOKEN`, or `COH_AUTH_TOKEN` matches the listener requirement. Live listeners reject missing or placeholder tokens such as `changeme`.
- **Serial vs TCP differences**: the root console is independent of the TCP listener—verify liveness with `ping` on the serial console (`cohesix>`) to isolate network issues.【F:apps/root-task/src/event/mod.rs†L10330-L10529】

## Future Root Console Extensions (ideas)
Not implemented yet, but likely additions for debugging:
- `net` – report virtio-net status and console listener port.
- `tcp` – list active TCP console sessions and counters.
- `9p` – basic NineDoor state (session counts, outstanding requests).
- `trace` – toggle trace categories for boot/net/9p.
Any future commands must remain deterministic, no_std-friendly, and will be documented here when they land.

## References & Cross-links
- Architecture and role model: `docs/ARCHITECTURE.md`.
- Protocol/schema details: `docs/INTERFACES.md` (once stabilised).
- This document stays focused on operator-facing workflows and real behaviours for the root console and `cohsh` CLI.
