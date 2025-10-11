<!-- Author: Lukas Bower -->
# GPU Bridge Host Tools

Host-side bridge utilities live here as defined in `docs/ARCHITECTURE.md` §3 and
`docs/GPU_NODES.md`. The `gpu-bridge-host` binary provides a mockable discovery
path (`--mock --list`) and an optional NVML backend (enable the `nvml` cargo
feature). Output from the CLI feeds directly into NineDoor via
`NineDoor::install_gpu_nodes` to mirror `/gpu/<id>` namespaces.
