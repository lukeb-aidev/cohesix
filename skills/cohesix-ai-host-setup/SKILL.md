---
name: cohesix-ai-host-setup
description: Set up and verify Cohesix host tools from native .pkg or .deb installers or release archives, with optional AI runtimes and QEMU, on Apple Silicon macOS or Ubuntu ARM64. Use for host installation, MLX, MAX, vMLX, Hugging Face, PEFT, CUDA, NeMo, or VM setup and repair.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Route verified Cohesix host installation and AI runtime setup without conflating setup with live target acceptance. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Prepare an AI host

Use this skill when the user asks to install Cohesix host tools or set up,
repair, or inventory host AI tools or optional QEMU alongside them. Read
[Mac setup](references/mac.md) for Apple Silicon and
[Linux setup](references/linux.md) for Ubuntu ARM64. `MAX` means
Modular MAX; `MLX` means Apple's array/model stack; `vMLX` is a separate desktop
app. Choose the components and installation method for the actual host,
workload and operator preference. The platform references give decisions and
proof examples, not a required stack or fixed sequence.
The repository's pinned NeMo Agent Toolkit kit is for Linux AArch64; a Mac
agent client needs its own qualified MCP/A2A path.

## Choose the Cohesix installation

Discover and reuse the user's selected installation before downloading or
installing anything. On Mac, handle both the extracted `.tar.gz` host bundle
and the native `.pkg`; neither requires a source checkout. Resolve `COH_BIN`
to an absolute directory and retain how its release identity was verified:

| Host installation | Tool directory | Identity and resources |
| --- | --- | --- |
| Mac `.pkg` | `/Library/Application Support/Cohesix/bin` | Check the actual installer receipt, selected installed-file manifest and publisher validation. SwarmUI is `/Applications/SwarmUI.app`. |
| Mac `.tar.gz` | `<absolute-extracted-bundle>/bin` | Check that bundle's `VERSION.txt`, manifest, packaged guide and hashes. Use its own policies, Python and resources. |
| Ubuntu ARM64 `.deb` | `/usr/lib/cohesix/bin` | Check the controller package receipt and installed-file manifest; desktop SwarmUI is optional. |
| Linux `.tar.gz` | `<absolute-extracted-bundle>/bin` | Check its version, manifest and matching Linux resources. |

For an existing tarball installation, obtain its selected extraction directory
or resolve an existing executable path to its bundle. Do not guess a path from
this repository's `releases/` directory or assume the shell's first `coh` is
the selected one. For a `.pkg`, check the Application Support location even
when `coh` is absent from PATH. If both exist, use the user's selection or
determine which owns the current services/configuration before changing
anything. Record the absolute `coh` and `cohsh` paths and check their launch/help
in the actual user/service context. A package launch failure remains a failure
of that installation; choosing a tarball is an explicit installation choice.

Read the selected release's
[Quickstart](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/QUICKSTART.md)
at its tag or source commit. For supported native host installation, choose the
verified macOS `.pkg` or Ubuntu ARM64 `.deb` set. A headless Linux host needs
only the controller package; add the SwarmUI package for a GNOME desktop.
The portable host archive remains useful for a self-contained installation
and supplies the matching guest files for a QEMU journey. Keep binaries,
policies and Python from one host installation. If the host tools came from a
native package, use guest assets from the same release archive without adding
its `bin` directory to the active tool path.

Before installing a downloaded package, check its bytes against the release
installer manifest and establish publisher trust independently: accepted
Developer ID Installer signature and stapled notarization on Mac, or the
signed Debian installer manifest with a key authenticated outside the download
on Linux. Check the selected release's advertised OS and architecture envelope;
record when a different environment has only local smoke evidence. Inspect
running services, existing state and the documented upgrade/removal behavior
before replacing an installation. The native packages preserve external
operator state and do not provision a seL4 guest, CUDA driver or model.

For a host-only or Pi journey, QEMU is optional. For a QEMU journey, retain the
matching release archive and use its `scripts/setup_environment.sh --with-qemu`
and `--check --with-qemu` for the guest's HVF/KVM startup profile. On a headless
Ubuntu archive setup, add `--headless` to omit graphical dependencies. Guest
boot and an authenticated request remain separate checks; a version print or
accelerator list does not establish guest compatibility.

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

1. Record the OS, architecture, accelerator and driver or JetPack identity,
   relevant Python and package managers, free disk, model/cache locations,
   installed versions and actual command paths.
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
3. Reuse a working, compatible environment. Choose a native app, package,
   isolated Python runtime or container according to the workload and host;
   keep model packages out of system Python. Isolate NeMo when its dependencies
   conflict with the selected runtime. Record the absolute `hf` and `nat`
   executables when used and verify them in the actual launch context: a login
   shell's PATH may differ from SSH, systemd or another agent's process. Use
   verified paths in automation.
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

For the user's first useful task, load `cohesix-get-started`. Local Mac model
experiments use `cohesix-mlx-workbench`; preparing a new CUDA batch uses
`cohesix-workload-authoring`; the pinned Linux client/evaluation journey uses
`cohesix-nemo-workflows`. Load those complete companion folders from the same
collection. Host installation alone does not enroll or submit a job.

Return a concise environment record: host/profile, Cohesix installation method
and release identity; component, version and absolute path or immutable image
digest; model and cache location; command/check and observed result; blocker;
and the exact scope of any claim. Include install/repair commands that another
operator can repeat without local credentials or private paths.
