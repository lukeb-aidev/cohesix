<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record Milestone 26d seL4 15 baseline provenance, generated artifact evidence, and remaining proof boundaries. -->
<!-- Author: Lukas Bower -->
# Milestone 26d seL4 15 Provenance

## Scope

Authorization: `docs/BUILD_PLAN.md` Milestone 26d, tasks
`m26d-kernel-provenance-refresh`, `m26d-sel4-api-compat-audit`,
`m26d-pi4-counter-contract-refresh`, `m26d-domain-schedule-debt-removal`,
`m26d-benchmark-revalidation-and-tuning`, and
`m26d-full-regression-refresh`.

## Accepted Kernel Baseline

- Upstream source path: `/Users/lukasbower/seL4_15`.
- Upstream version: `15.0.0`.
- Upstream commit: `881de507fe528490dc5e570c7810a149bad5880f`.
- Python environment: `/Users/lukasbower/seL4_15/.venv_aarch64`.
- Manual artifact: `seL4/seL4-manual-latest.pdf` and
  `seL4/seL4-manual-latest.md` refreshed from the official seL4 15.0.0
  manual.

The local kernel source intentionally carries one Cohesix Pi 4 overlay patch in
`src/plat/bcm2711/overlay-rpi4.dts`: `device-untypes@600000000` exposes the
VL805 xHCI BAR0 window (`reg = <0x00000006 0x00000000 0x00040000>;`) so the
Pi 4 USB runtime can rediscover the high BAR through HAL-owned mappings after
U-Boot quiesces USB.

## Refreshed Artifact Trees

| Tree | Profile | Required evidence |
| --- | --- | --- |
| `seL4/build` | QEMU `aarch64/virt`, single-core | `KERNEL_PATH=/Users/lukasbower/seL4_15`, `KernelVerificationBuild=OFF`, `KernelDebugBuild=ON`, `KernelPrinting=ON`, `SMP=OFF`, `ElfloaderRootserversLast=ON`, QEMU `virt,secure=off,virtualization=on,gic-version=2` DTB with PSCI `method = "smc"`, no `KernelDomainSchedule` cache entry |
| `seL4/SMP_build` | QEMU `aarch64/virt`, four-node SMP | `KERNEL_PATH=/Users/lukasbower/seL4_15`, `KernelMaxNumNodes=4`, `SMP=ON`, `ElfloaderRootserversLast=ON`, QEMU `virt,secure=off,virtualization=on,gic-version=2` DTB with PSCI `method = "smc"`, no `KernelDomainSchedule` cache entry |
| `seL4/build_UBOOT` | Pi 4 `bcm2711`, U-Boot image | `KERNEL_PATH=/Users/lukasbower/seL4_15`, `ElfloaderRootserversLast=ON`, `IMAGE_START_ADDR=0x10000000`, `KernelArmExportVCNTUser=ON`, `KernelArmExportPCNTUser=OFF`, `KernelArmExportPTMRUser=OFF`, `KernelArmExportVTMRUser=OFF`, `TIMER_CLOCK_HZ=54000000`, no `KernelDomainSchedule` cache entry |

QEMU generated evidence:

- `seL4/SMP_build/elfloader/gen_config/elfloader/gen_config.h` and
  `seL4/build/elfloader/gen_config/elfloader/gen_config.h` define
  `CONFIG_ELFLOADER_ROOTSERVERS_LAST=1`. This fixes the seL4 15 QEMU boot
  failure where the default rootserver placement overlapped the elfloader.
- `seL4/SMP_build/kernel/kernel.dts`, `seL4/SMP_build/qemu-arm-virt.dts`,
  `seL4/build/kernel/kernel.dts`, and `seL4/build/qemu-arm-virt.dts` were
  generated from QEMU with `virtualization=on` and record PSCI
  `method = "smc"`. This matches the Cohesix QEMU launcher and avoids the
  secondary-CPU `HVC is not supported for PSCI` failure on SMP boot.

Pi 4 generated evidence:

- `seL4/build_UBOOT/elfloader/gen_config/elfloader/gen_config.h` defines
  `CONFIG_ELFLOADER_ROOTSERVERS_LAST=1`, carrying the seL4 15 rootserver
  placement fix from the QEMU boot investigation into the U-Boot profile.
- `seL4/build_UBOOT/elfloader/gen_headers/image_start_addr.h` defines
  `IMAGE_START_ADDR 0x10000000`. This preserves the Pi 4 U-Boot `bootm`
  handoff shape proven by earlier live serial logs; the shoehorn-computed
  `0x7ab000` address causes U-Boot to reject the seL4 elfloader with
  `Bad Linux ARM64 Image magic!` before seL4 starts.
- `seL4/build_UBOOT/kernel/gen_headers/plat/platform_gen.h`:
  `TIMER_CLOCK_HZ=54000000`.
- `seL4/build_UBOOT/kernel/gen_config/kernel/gen_config.h`:
  `CONFIG_EXPORT_VCNT_USER=1`; physical counter and EL0 timer-control exports
  disabled.
- `seL4/build_UBOOT/kernel/kernel.dts` includes `device-untypes@600000000`
  with the expected `0x600000000` / `0x40000` VL805 window.
- `out/pi4-sd/pi4-runtime-dma-proof.env` is stage-only proof:
  `PI4_RUNTIME_DMA_PROOF=target-build` and
  `PI4_RUNTIME_DMA_COUNTER_PROOF=not-live`.

## Commands Run

```bash
git -C /Users/lukasbower/seL4_15 describe --tags --always --dirty
git -C /Users/lukasbower/seL4_15 rev-parse HEAD
cat /Users/lukasbower/seL4_15/VERSION

CARGO_TARGET_DIR=target/m26d-pi4 scripts/pi4-image-build.sh \
  --manifest configs/root_task_pi4_uboot_aarch64.toml \
  --sel4-build-dir seL4/build_UBOOT \
  --sel4-kernel-source-dir /Users/lukasbower/seL4_15 \
  --venv /Users/lukasbower/seL4_15/.venv_aarch64

SEL4_BUILD_DIR="$PWD/seL4/SMP_build" \
  COHSH_BATCH_TARGET=qemu \
  COHSH_BATCH_GROUPS=base \
  COHSH_LOG_ROOT=out/regression-logs/m26d-qemu-base \
  scripts/cohsh/run_regression_batch.sh

SEL4_BUILD_DIR="$PWD/seL4/SMP_build" \
  COHSH_BATCH_TARGET=qemu \
  COHSH_BATCH_GROUPS=base-telemetry,base-shard,gated \
  COHSH_LOG_ROOT=out/regression-logs/m26d-qemu-remaining \
  scripts/cohsh/run_regression_batch.sh

.venv/bin/python -m pytest -q \
  tests/test_rest_perf_harness.py \
  tests/test_pi4_compare_driver_models.py
```

The Pi image command completed stage-only and restored canonical generated
manifest artifacts afterward.

## Focused QEMU Evidence

- QEMU SMP TCP regression passed on `seL4/SMP_build`: `11` base `.coh`
  scripts in `out/regression-logs/m26d-qemu-base` and `7` remaining `.coh`
  scripts in `out/regression-logs/m26d-qemu-remaining`.
- QEMU REST performance harness ran through a local `hive-gateway` backed by
  the refreshed `seL4/SMP_build` VM:
  - `out/bench/m26d-qemu-sel4-15-initial_20260629T082620Z.log`:
    status suite complete, speedup `27.31x`; telemetry skipped because the
    QEMU manifest exposed no `/worker` entries to the perf discovery path.
  - `out/bench/m26d-qemu-sel4-15-final_20260629T082622Z.log`:
    status suite complete, speedup `30.45x`; telemetry skipped for the same
    QEMU worker-discovery reason.
  - VM and gateway logs: `out/bench/m26d-qemu-sel4-15.qemu.log` and
    `out/bench/m26d-qemu-sel4-15.gateway.log`.
- Single-core QEMU smoke on `seL4/build` with `COHESIX_QEMU_SMP=1` confirmed
  the seL4 15 loader/kernel/rootserver handoff reaches userspace with the
  rootserver placed at `0x7fbe4000..0x7fffefff`. It then hits the existing
  root-task `allocation attempted before allocator ready` bootstrap guard before
  TCP auth readiness, so this ledger does not claim single-core Cohesix runtime
  acceptance. Evidence: `out/bench/m26d-qemu-single-smoke-1cpu.qemu.log`.

## Focused Pi 4 Wi-Fi Evidence

- Selected newest non-empty serial log:
  `/Users/lukasbower/pi4-serial-20260629-220122.log`.
- Selected boot-time Wi-Fi packet capture:
  `/Users/lukasbower/tcpdump-wifi-20260629-202452.pcap`.
- Live boot reached U-Boot, seL4, root-task userspace, arch-counter timer
  setup, Wi-Fi secure association, DHCP bind, USB input readiness, HDMI
  readiness, and all six driver-task DMA proof rows.
- Live ARP and ping proof: host `.102` ARPed for Pi `.154`, Pi replied from
  `88:a2:9e:66:59:10`, and host ping to `192.168.86.154` returned `2/2`
  packets with `15.048 ms` average round-trip time.
- Raw authenticated TCP console proof:
  `out/bench/m26d-pi4-wifi-20260629T120550Z-raw-cohsh.txt`.
  The run completed `AUTH`, `ATTACH`, `PING`, and `NETSTATS` over
  `192.168.86.154:31337`.
- Strict normalized gate proof:
  `out/test-plan/m26d-pi4/pi4-runtime-dma-proof-20260629-220122.env`.
  The proof records `USB_GATE=10`, `WIFI_GATE=10`, `NET_ACTIVE=wifi`,
  `NET_DHCP=bound`, `PI4_RUNTIME_DMA_PROOF=fresh-pi`,
  `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`,
  `DRIVER_TASK_ACTIVE_NET=cyw43`, `DRIVER_TASK_DMA_PROOFS=6`,
  `DRIVER_TASK_DMA_BLOCKER=none`, and
  `DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT=0`.
- Live Pi REST performance harness ran through a local `hive-gateway` backed by
  the Wi-Fi TCP console:
  - `out/bench/m26d-pi4-wifi-20260629T120550Z-m26d-pi4-sel4-15-final_20260629T120755Z.log`:
    status suite complete, sequential average `0.093s`, parallel average
    `0.004s`, speedup `25.73x`.
  - Telemetry suite skipped because no `/worker` entries were exposed to the
    perf discovery path on this live Pi manifest.
  - Gateway counters stayed clean for the benchmark start state:
    `pool_exhausted=0`, `checkout_retries=0`, and `timeout_rejections=0`.

## Intermittent Wi-Fi Reboot Evidence and Fix Batch

Follow-up saved-policy Wi-Fi reboot diagnostics on the pre-fix Pi image used
the redacted serial log `/Users/lukasbower/pi4-serial-20260630-000021.log`,
cycle ledger
`out/pi4-live/m26d-wifi-10cycle-20260630-000021.jsonl`, and TCP transcripts
under `out/pi4-live/m26d-wifi-10cycle-tcp-20260630-000021/`.

The ten-cycle run did not stop on the first Wi-Fi failure. It recorded eight
DHCP-bound boots at `192.168.86.154`, one `dhcp timeout-exhausted` boot, and
one prompt-ready `dhcp=host-eapol-required` boot. TCP auth was ready on seven
of the ten boots. Cycles 1 and 4 completed `tcp_basic.coh`, `boot_v0.coh`, and
`smp_parity.coh`; the remaining bound cycles exposed TCP script or reboot
timeouts after DHCP bind. The serial trace repeatedly shows the `.154` Pi lease
paired with `.102` ARP traffic and CYW43 TCP transmit retry/no-completion
events toward the `.102` peer, preserving the `.154`/`.102` ARP clue as a live
diagnostic rather than treating DHCP success as full TCP success.

The fix batch in this change keeps CYW43 post-secure data admission
broadcast-capable (`allmulti=1`, `promisc=1`), allows CYW43 pre-poll data drain
during DHCP selecting/requesting/bound phases, sends both unicast and broadcast
ARP-assist replies for the assigned Pi address, emits gratuitous ARP request
and reply announcements when DHCP assigns the IPv4 address, and records ARP TX
counter evidence on submitted CYW43 Ethernet payloads.

This reboot evidence is a pre-fix diagnostic baseline. The patched image was
not reflashed in this run because `/Volumes/COHESIX` was not mounted on the
macOS host. The next hardware acceptance step is to rebuild/reflash the patched
Pi image and rerun the same saved-policy ten-cycle Wi-Fi loop before claiming
post-fix Wi-Fi stability or proceeding to the GENET parity loop.

## Remaining Closure Boundary

This ledger proves the seL4 15 build-artifact replacement, Pi stage-only image
build, focused QEMU SMP TCP regression, focused QEMU REST status benchmark,
live Pi 4 Wi-Fi boot/network/raw-console proof, strict live Pi runtime/DMA
proof, and live Pi 4 Wi-Fi REST status benchmark lane.

The full target-qualified staged runner is not claimed here because the live
Pi run did not execute the full lifecycle-reset/reboot path required by that
runner. Production wired/GENET parity is also not claimed here; that remains a
separate 26b/26d proof boundary. Wi-Fi evidence is accepted only for the
diagnostic/status lane documented in `docs/BENCHMARKS.md`.
