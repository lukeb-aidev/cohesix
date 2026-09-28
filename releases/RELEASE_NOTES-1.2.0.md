<!-- Author: Lukas Bower -->
<!-- Purpose: Explain Cohesix 1.2.0, its operator benefits and supported release profiles. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.2.0

Cohesix 1.2.0 brings edge AI work into one accountable flow. Submit a job,
watch the Queen admit it, run it on the right host, and inspect the result that
host actually produced. If a connection drops after submission, Cohesix can
reconcile the original job instead of guessing whether it should run again.

The Queen runs in seL4 on QEMU or Raspberry Pi 4. GPU work stays on the host
that owns the accelerator: CUDA on the supported NVIDIA Jetson reference and
MLX on a supported Apple Silicon Mac. The Queen applies authority, scheduling
and recovery rules; it does not load GPU libraries into the VM or Pi.

## What you can do

- **Use the client you already have.** SwarmUI, Cohesix command line and
  Python clients, MCP tools, and A2A agents can follow the same governed job.
  The QEMU and Pi release profiles enable MCP and A2A in the host gateway by
  default. Operators can disable either protocol in their deployment profile.
- **Know which result is real.** An admission acknowledgement means the Queen
  accepted a request; it does not mean CUDA or MLX finished. Cohesix retains
  the original job identity and checks the native provider result separately.
  A lost reply, uncertain outcome or failed candidate stays visible rather
  than being silently replayed as success.
- **Run useful GPU workflows.** The supported CUDA path covers inference and
  adapter work on the Jetson reference. The local MLX path uses Apple's Metal
  runtime on Apple Silicon. Provider selection is explicit, so a job does not
  quietly move from MLX to CUDA or CPU.
- **Manage model changes deliberately.** The PEFT workflow connects training
  or import to evaluation, canary, promotion and rollback. A rejected
  candidate remains rejected even if the earlier serving version is restored.
  The pinned NeMo Agent Toolkit kit provides MCP and A2A client examples while
  keeping client dependencies separate from the CUDA provider environment.

## Pick a download

| Where you run it | Release files |
| --- | --- |
| Apple Silicon Mac | `Cohesix-1.2.0-MacOS.tar.gz` for a portable setup, or the signed and notarized `Cohesix-1.2.0-MacOS.pkg` for a normal Mac installation with SwarmUI in Applications. |
| JetPack Linux ARM64 | `Cohesix-1.2.0-linux.tar.gz` for a portable setup, or `cohesix-controller_1.2.0_arm64.deb` plus the optional `cohesix-swarmui_1.2.0_arm64.deb` for a system installation. The Debian publisher signs the package manifest, which binds both package hashes. |
| Raspberry Pi 4 | `Cohesix-1.2.0-Pi4.tar.gz` contains `image/cohesix-pi4-sd.img` and its checksum for flashing an SD card. |
| Python clients | The Cohesix SDK and NeMo client kit are included in the host archives at version `1.2.0`. The SDK is also published on PyPI as `cohesix==1.2.0`. |

The Mac and Linux host installers include the Cohesix command line tools and
SwarmUI. QEMU is optional: setup checks the installed binary and offers a
compatible pinned build when the system package is unsuitable. No installer
downloads a model or copies your credentials.

Start with the [quickstart](../docs/QUICKSTART.md), then use the
[use cases](../docs/USE_CASES.md) to choose a client and provider. Verify a
download against the release checksums and the package signature before
installing it. The Debian publisher key fingerprint is
`6840 71A2 AF49 8008 930C F6AE EBB8 ECC4 9397 F8C2`.

## Supported release profiles

The Linux installer reference is JetPack 7.2.1 / L4T 39.2.1 on ARM64. The Mac
reference is Apple Silicon. Raspberry Pi 4 release qualification uses wired
GENET networking. Other JetPack versions, Linux distributions and Pi Wi-Fi
setups require their own qualification; the matching CPU architecture alone
does not establish compatibility. See [host tools](../docs/HOST_TOOLS.md) and
[GPU nodes](../docs/GPU_NODES.md) for the selected host and provider contracts.

For the exact release artifact hashes, installation checks and target evidence,
see the [release evidence checklist](../docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md).
