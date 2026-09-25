<!-- Author: Lukas Bower -->
<!-- Purpose: Select and verify Apple Silicon MLX, Modular MAX, vMLX, Hugging Face and PEFT host tools. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Apple Silicon macOS

First check `uname -m`, `sw_vers -productVersion`, `xcode-select -p`,
`python3 --version`, `command -v hf`, `command -v max`, and installed app
versions. Select a native ARM64 Python interpreter. When Cohesix build tools
are required, use [`toolchain/setup_macos_arm64.sh`](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/toolchain/setup_macos_arm64.sh)
and its [toolchain guide](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TOOLCHAIN_MAC_ARM64.md); do not add AI
packages to the seL4 or repository test dependency locks for convenience.

| Tool | Setup decision | Proof |
| --- | --- | --- |
| Hugging Face Hub | Reuse a working `hf` CLI, or install the Homebrew `hf` formula for a user-wide command. Keep Hub Python libraries inside the selected model runtime. Configure `HF_HOME` on a volume with room for the selected model. | `command -v hf`, `hf version`, a public metadata/download check if needed; report authentication separately. |
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
