<!-- Author: Lukas Bower -->
<!-- Purpose: Select and verify Linux NVIDIA CUDA, Hugging Face PEFT and NeMo Agent Toolkit host tools. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Linux NVIDIA host

Identify distribution, architecture, GPU model/compute capability, driver,
CUDA runtime and container runtime. On Jetson, record the actual JetPack/L4T
release and NVIDIA container runtime; `nvidia-smi` may be unavailable there.
Read the selected host's live inventory and the
[NVIDIA Jetson PyTorch compatibility table](https://docs.nvidia.com/deeplearning/frameworks/install-pytorch-jetson-platform-release-notes/pytorch-jetson-rel.html)
before choosing a PyTorch image or wheel. For a different NVIDIA AArch64 host,
use its own driver/architecture compatibility evidence. Do not replace
board-managed drivers or a working iGPU PyTorch build with a generic CUDA
wheel merely to align version strings.

| Tool | Setup decision | Proof |
| --- | --- | --- |
| Cohesix host build tools | If required, run the maintained [`setup_linux_arm64.sh`](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/toolchain/setup_linux_arm64.sh) for supported Ubuntu ARM64. It does not install a CUDA workload runtime. | Confirm its selected build tools separately from GPU checks. |
| CUDA + PyTorch | Prefer a compatible, digest-pinned NVIDIA/JetPack container for the GPU workload. Inspect useful existing images and mounts before pulling or pruning. Keep model/cache storage on a volume with sufficient space. | In that exact image, inspect CUDA availability, actual device name and supported compiled GPU architectures; perform a real CUDA forward/backward/optimizer step. |
| Hugging Face + PEFT | Expose `hf` as a user-wide CLI, through a working install or `pipx`; put Transformers, Accelerate, PEFT and their PyTorch dependency in the selected GPU runtime. Pin base model, adapter and package revisions. | `hf version`, runtime `pip check`, adapter load, and a small CUDA LoRA forward/backward/optimizer step with nonzero gradients. |
| NeMo Agent Toolkit | Install the selected pinned `nvidia-nat[mcp,a2a]` release following [NVIDIA's install guide](https://docs.nvidia.com/nemo/agent-toolkit/latest/quick-start/installing.html). Reuse a compatible client environment; isolate NAT from the GPU image only when their dependency sets conflict, and expose `nat` on PATH. Do not install full NeMo Framework, NIM, Guardrails, Triton or Kubernetes unless a selected workflow needs one. | `nat --version`, dependency check, native MCP tool ping/list/call on the selected transport, and native A2A Agent Card/task helpers. Test protected credentials and user scope against the real endpoint before claiming Cohesix integration. |

The Jetson test bed has used a dedicated HF/PEFT GPU container, a user-wide
`hf` command, and a separate NeMo client environment because that combination
passed its local compatibility checks. Treat those as inventory examples, not
portable pins. A container can expose an ordinary launcher without requiring
operators to activate another Python venv. Keep Docker privileges scoped to
the host policy; do not make passwordless root or docker-group membership a
setup shortcut.

For NeMo, verify the selected release's native
[MCP client](https://docs.nvidia.com/nemo/agent-toolkit/latest/build-workflows/mcp-client.html)
and [A2A client](https://docs.nvidia.com/nemo/agent-toolkit/latest/build-workflows/a2a-client.html)
against the selected Cohesix transport and auth. A temporary local fixture is
client compatibility evidence only. A live accepted CUDA/PEFT operation needs
the original job identity, native receipt and Cohesix verifier result under
the [Test Plan](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TEST_PLAN.md).
