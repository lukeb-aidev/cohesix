<!-- Author: Lukas Bower -->
<!-- Purpose: Explain the Release B candidate in plain English with measured evidence and publication limits. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.2.0 candidate

Release B brings more of an edge AI job into one governed journey. An operator
can ask the Queen to admit work, run it on the appropriate host, inspect the
result, and recover the original job after a lost reply. The Pi 4 remains the
control plane: CUDA and PEFT run on a compatible NVIDIA host, while MLX runs
locally on a supported Mac. Cohesix does not run GPU libraries inside seL4.

## What changes since 1.1.0-beta

- **More useful GPU work.** The selected CUDA path covers reference inference
  and adaptation with pinned inputs and independently checked outputs. The PEFT
  path connects training or import to evaluation, canary, promotion and
  rollback. A refused candidate stays refused even when the previous serving
  generation is restored. See [GPU nodes](../docs/GPU_NODES.md) and
  [private LoRA release](../docs/PRIVATE_LORA_RELEASE.md).
- **A native Mac compute path.** MLX work uses Apple's local Metal runtime on
  supported Macs. Host and provider selection is explicit so an MLX request
  cannot silently become a remote CUDA or CPU job. The Mac and NVIDIA paths
  retain their separate device and result evidence.
- **MCP and A2A reach the same governed jobs.** A standard MCP client can
  discover scoped tools and recover their original results. An A2A peer can
  follow the same underlying job through its task identity. Protocol responses
  do not certify native execution by themselves; the Queen's admission and the
  independent provider verifier remain authoritative. The selected QEMU and Pi
  production profiles enable both host gateway protocols, with explicit
  deployment switches to disable either one.
- **A pinned NeMo Agent Toolkit kit.** The Linux ARM64 kit packages client
  configuration and a bounded install path for Toolkit 1.9.0. It keeps NeMo's
  MCP and A2A clients separate from the CUDA provider environment. A model's
  text is never treated as a signed Cohesix job outcome.
- **One version across the Python clients.** The Cohesix and NeMo wheels use
  `1.2.0`. The host tools, generated
  contracts and selected package inventory are checked together.

## Evidence so far

At source commit `522463fa1ade`, the canonical QEMU and Pi 4 Test Plans each
passed Stages 01–05. The physical Pi used an exact-source GENET RAM image;
its runtime and DMA proof combined the same boot's UART output and authenticated
Queen log, with both original captures retained and hashed. A separate
60-minute Pi GENET diagnostic completed four MCP and six A2A original jobs with
target acknowledgements and independently checked Jetson CUDA outputs. That
diagnostic changed its collector during the timed run and used development
installations, so it is not an installed-release acceptance result.

The selected Jetson CUDA and NeMo component checks and Mac MLX host checks have
their own retained identities. They show those host paths working within their
tested scope. They do not establish every mixed-provider, installed-client or
model-quality journey in the Release B matrix.

## Installation and release status

The planned distribution includes portable Mac, Linux ARM64 and Pi 4 archives,
a signed and notarized macOS `.pkg`, and signed ARM64 `.deb` packages qualified
against JetPack 7.2.1 / L4T 39.2.1 (Ubuntu 24.04 base). Other JetPack and
Ubuntu releases have no Release B installer qualification claim.
The Debian publisher's public key has fingerprint
`7E27 A4AB 355D 2EA5 6571 8CA4 1059 A516 E53B 0B70`; verify it through an
independent trusted release announcement before trusting package signatures.
The native installers and exact distributed archives are still being assembled
and qualified. Do not install a candidate by treating a source build or an
unsigned package as a published release. Release 1.1.0-beta remains the
published version; its [release notes](RELEASE_NOTES-1.1.0-beta.md) and
[quickstart](../docs/QUICKSTART.md) remain available.

Release B is not yet approved for publication. The remaining gates include
signed installer and clean-install checks on advertised hosts, launch from
the native desktops, installed CLI/Python/SwarmUI parity, the complete live
client and provider matrix, Jetson KVM pressure, physical media and
repeatability evidence, final archive verification and the named release
owner's approval. The [M28g implementation record](../docs/audit/M28G_IMPLEMENTATION_RECORD.md)
keeps passing results and unresolved work at their original identities.
