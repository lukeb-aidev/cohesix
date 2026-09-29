<!-- Author: Lukas Bower -->
<!-- Purpose: Select and verify Mac Cohesix installation, optional QEMU and AI host tools. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Apple Silicon macOS

First check `uname -m`, `sw_vers -productVersion` and any existing Cohesix
installation. Inspect Python, `hf`, `max`, app versions and developer tools only
when the selected workload uses them. Select a native ARM64 Python interpreter
for local AI workloads.
For the Cohesix host tools, use a verified release `.pkg` when a normal Mac
installation and `/Applications/SwarmUI.app` are wanted. Verify its Developer
ID Installer signature, notarization and release manifest as the selected
[Quickstart](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/QUICKSTART.md)
specifies. Check the installed version and host tools under
`/Library/Application Support/Cohesix/bin`; the app should launch from Finder
without a Terminal environment. Use the package's receipt-bound
`cohesix-uninstall` helper for removal. An archive is a valid alternative for
portable use and is still needed for a matching QEMU guest; do not assume the
`.pkg` contains guest files or installs QEMU.

When Cohesix build tools are required, use
[`toolchain/setup_macos_arm64.sh`](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/toolchain/setup_macos_arm64.sh)
and its [toolchain guide](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TOOLCHAIN_MAC_ARM64.md); do not add AI
packages to the seL4 or repository test dependency locks for convenience.

| Tool | Setup decision | Proof |
| --- | --- | --- |
| Optional QEMU guest | Use the matching release archive's `./scripts/setup_environment.sh --with-qemu` and `--check --with-qemu`. Reuse a system QEMU only if the four-core Cortex-A57/GICv3 HVF startup probe passes. If it fails, the maintained setup builds pinned QEMU 10.1.0 with the verified HVF fix from upstream source into a user-owned prefix; do not copy a binary from another Mac or replace Homebrew's QEMU. Host-only and Pi operation need no QEMU. | Record the selected executable and SHA-256, successful startup probe, then boot the matching Mac guest and make an authenticated client request. Startup alone is diagnostic, not target acceptance. |
| Hugging Face Hub | Reuse a working `hf` CLI, or install it where the selected user or service can reach it; Homebrew's formula is one Mac option. Keep Hub Python libraries inside the selected model runtime. Configure `HF_HOME` on a volume with room for the selected model. | Resolve the executable in its launch context, run `hf version`, and make a public metadata/download check if needed; report authentication separately. |
| MLX / MLX-LM | Install compatible pinned `mlx` and, if serving/generation is selected, `mlx-lm` with the native Python used by the local workload. Verify model format, memory and Metal support before download. | Force an MLX GPU array evaluation, then use a pinned small model for real inference/evaluation. Check actual selected device and output; a Python import alone is insufficient. |
| Modular MAX | If requested, install a pinned stable `max` package with only the extras the workload needs, following the [official requirements and install guide](https://docs.modular.com/max/packages). Its `max` CLI is distinct from MLX. Do not assume its installed version uses the Apple GPU for a selected model. | `max --version`, then a supported pinned model operation and actual device/backend observation. Record unsupported model or device as unavailable. |
| vMLX | Install or retain the app from its [upstream release](https://github.com/jjang-ai/vmlx); record app version and macOS signature assessment. Explicitly select a tool-capable model and the configured local endpoint. | Inspect the live model list, make a model response, then test its configured MCP tool discovery/call if claiming client compatibility. An empty model list is not a usable session. |
| Hugging Face PEFT | Select a PyTorch/MPS, Transformers, Accelerate and PEFT combination compatible with the Mac workload; pin them together. A PEFT import on the Mac does not substitute for an MLX adapter or a Jetson CUDA run. | `pip check`, real local adapter load and a bounded forward/backward or inference step on the selected backend; record base/model/adapter revisions. |

Before claiming vMLX MCP support, record its app/model/MCP SDK versions,
transport, tool policy and original Cohesix operation identity. For A2A, name
the actual A2A-capable peer using vMLX as its model endpoint; the app itself is
not automatically an A2A peer. Follow the [Test Plan](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TEST_PLAN.md)
for admitted work, denial and reconnect. Keep Apple signing certificates and
Hub credentials outside this environment record.

The [MLX project](https://github.com/ml-explore/mlx) and
[PEFT install guide](https://huggingface.co/docs/peft/install) describe their
current install surfaces. Check those and the selected versions before making
changes; do not silently replace a working local runtime with a moving release.
