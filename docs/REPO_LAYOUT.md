<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Help contributors find the owners of target code, host tools, generated contracts and release evidence. -->
<!-- Author: Lukas Bower -->
# Repository layout

Start with [Quickstart](QUICKSTART.md) if you want to run Cohesix. This map is
for readers who need to find an implementation, change a selected profile or
understand where a build result came from.

| Directory | What belongs here |
| --- | --- |
| `apps/root-task/` | The target Queen, admission, supervision and target service integration. |
| `apps/nine-door-runtime/`, `apps/console-network-runtime/`, `apps/pi4-driver-runtime/` | Selected target child runtimes. The driver runtime has physical Pi duties; the console child owns the target's sole authenticated TCP listener. |
| `apps/worker-heart/`, `apps/worker-gpu/`, `apps/worker-lora/` | Packaged passive target Worker roles. CUDA and PEFT run on external hosts, not in these images. |
| Other `apps/` | Host clients and services such as `coh`, `cohsh`, `hive-gateway`, SwarmUI, `gpu-bridge-host` and `host-ticket-agent`. Check each app's profile before running it. |
| `crates/` | Shared Rust libraries and protocol types used by the selected apps. |
| `tools/coh-rtc/` | The compiler for manifest validation and generated target/host contracts. |
| `tools/cohesix-py/` | Python package, examples and tests. |
| `configs/` | Source profiles and host integration inputs. `configs/generated/` contains compiler outputs; edit their inputs and regenerate, never patch generated files by hand. |
| `docs/` | Community guides, architecture and reference contracts. `docs/snippets/` contains generated excerpts; `docs/audit/` retains historical source-bound evidence and decisions. |
| `scripts/`, `toolchain/`, `packaging/` | Maintained build, test, provider, setup and packaging entry points. |
| `resources/`, `demo/`, `tests/` | API/resources, labelled examples and integration tests. A demo result has its own proof scope. |
| `seL4/` | Selected prebuilt kernel artifacts; upstream seL4 source is external. Match each artifact to its target profile. |
| `releases/` | Retained distribution trees and archive history. Use the [1.2.0 quickstart](QUICKSTART.md) for current archive names and verify each downloaded bundle's `VERSION.txt` and hashes. Older beta trees keep their original names. |
| `out/` | Ignored local builds, staging, run logs and scratch reports; it is not a published evidence store by itself. |

The root [AGENTS.md](../AGENTS.md) defines repo-wide invariants.
[CONTRIBUTING.md](../CONTRIBUTING.md) covers scoped changes and validation.
The [Build Plan](BUILD_PLAN.md) owns task scope, while the
[Test Plan](TEST_PLAN.md) selects checks and evidence. The
[architecture guide](ARCHITECTURE.md) explains which parts run on the target
and which run on a Mac or Linux host.

To trace an as-built value, start with the selected source manifest, resolve it
through `coh-rtc`, then compare the generated profile and exact target or host
artifact. Directory names alone do not establish what a release contains.
