---
name: cohesix-ai-host-setup
description: Configure and verify Cohesix host AI tools and optional QEMU on Apple Silicon macOS or Ubuntu ARM64. Use for MLX, MAX, vMLX, Hugging Face, PEFT, CUDA, NeMo, or VM runtime setup and repair; do not claim target acceptance from installation.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Prepare reproducible external AI host environments without conflating package presence with live Cohesix workflows. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Prepare an AI host

Use this skill when the user asks to set up, repair, or inventory host AI tools
or optional QEMU alongside Cohesix. Read [Mac setup](references/mac.md) for
Apple Silicon and [Linux setup](references/linux.md) for Ubuntu ARM64. `MAX` means
Modular MAX; `MLX` means Apple's array/model stack; `vMLX` is a separate desktop
app. Select only the requested tools and an actual supported host profile.
The repository's pinned NeMo Agent Toolkit kit is for Linux AArch64; a Mac
agent client needs its own qualified MCP/A2A path.
For a host-only or Pi journey, do not require QEMU. For a QEMU journey, use the
selected release archive's `scripts/setup_environment.sh --with-qemu` and
`--check --with-qemu`; it checks real HVF/KVM startup and keeps the Mac pinned
fallback separate from the Cohesix host installer. A version print or
accelerator list alone cannot establish guest compatibility.
On headless Ubuntu, add `--headless` to omit SwarmUI graphical packages.

The external model runtime, agent, caches and credentials live on the host,
not in seL4. The Mac may be a controller without local MLX work; a Linux
controller need not have CUDA. Read the selected installation's
[host-tool contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md),
[GPU contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/GPU_NODES.md), and
[Python support](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/PYTHON_SUPPORT.md)
when attaching Cohesix tools. Pin these `main` URLs to the selected source
commit/tag, or read the same documents in that checkout.
The repository's `toolchain/setup_*` scripts establish its own build dependencies;
they are not an AI-runtime installer. Preserve the seL4 profile environment and
the repository's hash-locked `.venv` unless its owner explicitly changes them.

## Decide from the live host

1. Record OS release, architecture, Python interpreters, accelerator and driver
   or JetPack identity, free disk, model/cache locations, package managers,
   installed app/CLI/package/container versions, and actual command paths.
   Compare this with the operator's inventory; treat older entries as leads.
2. Select the workload and its compatible runtime *before* installation:
   local Mac MLX, optional MAX, vMLX model/client, Linux CUDA + HF PEFT, or
   NeMo Agent Toolkit MCP/A2A client. Consult the selected upstream release's
   compatibility matrix. Pin the exact package versions, image digests, model
   revisions and adapter/base pairing in the environment record. Do not turn
   an unqualified `latest` tag or nightly into a reproducible profile.
   If the requested OS, architecture, accelerator or tool combination is
   unsupported, name the exact incompatibility and offer a compatible host,
   runtime or version before making changes. Do not transplant Mac setup
   commands onto Linux or Linux service and driver instructions onto macOS.
3. Reuse a working, compatible environment. Put a user-wide `hf` CLI on PATH;
   install Python model packages in the selected runtime, not in system Python.
   Use a separate NeMo client environment only when dependency compatibility
   requires it. Record the absolute `hf` and `nat` executables and verify them
   in the actual launch context: a login shell's PATH may differ from SSH,
   systemd or another agent's non-login process. Use those verified paths in
   automation. A GPU container is a distinct native execution runtime, not
   an incidental Python venv.
4. Before changing drivers, containers, Python packages, apps, services or
   caches, inspect dependents, running jobs and retained artifacts. Upgrade or
   remove only the selected surface; preserve a working runtime until its
   replacement passes the same smoke. Never put tokens, sudo passwords or
   signing identities in a tracked file or terminal log.

## Prove the requested capability

Use the platform reference's checks. A version print or successful import is
installation evidence. A small real Metal or CUDA forward/backward step,
adapter load, live model response, or native NeMo client call proves that
specific capability. MCP discovery, A2A task state and model text do not prove
the native job outcome. For a Cohesix workflow, follow the selected
[Test Plan](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TEST_PLAN.md)
and the
[GPU operations skill](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/skills/cohesix-gpu-operations/SKILL.md);
correlate the original operation with signed evidence. Report setup, native smoke,
protocol-client compatibility and live Cohesix acceptance separately.

Return a concise environment record: host/profile and source revision;
component, version and absolute path or immutable image digest; model and
cache location; command/check and observed result; blocker; and the exact
scope of any claim. Include install/repair commands that another operator can
repeat without local credentials or private paths.
