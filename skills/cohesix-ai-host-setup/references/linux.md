<!-- Author: Lukas Bower -->
<!-- Purpose: Select and verify Linux Cohesix installation, optional QEMU and NVIDIA AI host tools. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Ubuntu ARM64 host

Identify the distribution, architecture and existing Cohesix installation.
For an NVIDIA workload, inspect the GPU model/compute capability, driver,
CUDA and container runtimes. On Jetson, record the actual JetPack/L4T release;
`nvidia-smi` may be unavailable there.
For a compatible Ubuntu ARM64 host, use the verified `cohesix-controller`
`.deb` for host tools under `/usr/lib/cohesix/bin`. Add the optional
`cohesix-swarmui` `.deb` for GNOME; headless operation needs no desktop package.
Verify downloaded package hashes and `installers.json.asc` using an
independently authenticated publisher key as the selected
[Quickstart](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/QUICKSTART.md)
specifies. Check package-manager ownership and the installed version; GNOME
launch should work without an inherited shell environment. Use package-manager
remove or purge according to the selected release's state-retention guidance.
An archive is a valid portable alternative and still supplies matching QEMU
guest files; the `.deb` packages do not install the guest or QEMU.

For an NVIDIA workload, read the selected host's live inventory and the
[NVIDIA Jetson PyTorch compatibility table](https://docs.nvidia.com/deeplearning/frameworks/install-pytorch-jetson-platform-release-notes/pytorch-jetson-rel.html)
before choosing a PyTorch image or wheel. For a different NVIDIA AArch64 host,
use its own driver/architecture compatibility evidence. Do not replace
board-managed drivers or a working iGPU PyTorch build with a generic CUDA
wheel merely to align version strings.

| Tool | Setup decision | Proof |
| --- | --- | --- |
| Optional QEMU guest | On supported Ubuntu ARM64, use the matching release archive's `./scripts/setup_environment.sh --with-qemu`, which obtains `qemu-system-aarch64` from apt's `qemu-system-arm` package if needed. Add `--headless` on a server without SwarmUI to omit graphical packages. Host-only, Pi and remote executor operation need no local QEMU. A selected external `QEMU_BIN` is checked rather than silently replaced. | Check four-core KVM startup and `/dev/kvm` access, then boot the Linux guest and make an authenticated client request. Ubuntu 24.04's apt QEMU passed that journey on the selected Jetson; that observation does not qualify every Ubuntu release or pressure lane. |
| Cohesix host build tools | If required, run the maintained [`setup_linux_arm64.sh`](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/toolchain/setup_linux_arm64.sh) for supported Ubuntu ARM64. It does not install a CUDA workload runtime. | Confirm its selected build tools separately from GPU checks. |
| CUDA + PyTorch | Select a compatible native runtime or digest-pinned NVIDIA/JetPack container for the GPU workload. Inspect useful existing images and mounts before pulling or pruning. Keep model/cache storage on a volume with sufficient space. | In that exact runtime, inspect CUDA availability, actual device name and supported compiled GPU architectures; perform a real CUDA forward/backward/optimizer step. |
| Hugging Face + PEFT | Make `hf` available to the selected user or service through a working install or `pipx`; put Transformers, Accelerate, PEFT and their PyTorch dependency in the selected GPU runtime. Pin base model, adapter and package revisions. | Resolve `hf` in its launch context, run `hf version` and runtime `pip check`, then load an adapter and perform a small CUDA LoRA forward/backward/optimizer step with nonzero gradients. |
| NeMo Agent Toolkit | Install the selected pinned `nvidia-nat[mcp,a2a]` release following [NVIDIA's install guide](https://docs.nvidia.com/nemo/agent-toolkit/latest/quick-start/installing.html). Reuse a compatible client environment; isolate NAT from the GPU image only when their dependency sets conflict. Record the exact `nat` executable; do not assume a login-shell PATH is present under SSH or a service. Do not install full NeMo Framework, NIM, Guardrails, Triton or Kubernetes unless a selected workflow needs one. | Run the pinned executable's `--version` and dependency check in the actual launch context, then native MCP tool ping/list/call and A2A Agent Card/task helpers on the selected transport. Test protected credentials and user scope against the real endpoint before claiming Cohesix integration. |

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
