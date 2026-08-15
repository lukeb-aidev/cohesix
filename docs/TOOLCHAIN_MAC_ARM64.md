<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the reproducible macOS Apple Silicon host toolchain and external seL4 build contract. -->
<!-- Author: Lukas Bower -->

# Toolchain Setup — macOS 26 on Apple Silicon

macOS 26 on Apple Silicon is the primary Cohesix development host. This guide
installs host dependencies and explains how the current tree consumes, but does
not vendor, upstream seL4 build outputs.

## Supported toolchain

| Component | Current contract | Source of truth |
| --- | --- | --- |
| Rust | 1.97.1 minimal profile, `rustfmt`, `clippy`, `aarch64-unknown-none` | `rust-toolchain.toml` |
| Host packages | Git, CMake, Ninja, LLVM 17, CPython 3.13, QEMU, coreutils, GNU `cpio`, `jq`, protobuf, `repo`, GNU make, OpenSSL 3, and `pkgconf` | `toolchain/setup_macos_arm64.sh` |
| AArch64 compiler | Official Arm GNU Toolchain 15.2.Rel1 macOS Arm64 archive; GCC 15.2.1; target `aarch64-none-elf` | `configs/sel4/profiles.toml` |
| seL4 archive tool | GNU `cpio` 2.15 at the exact Apple Silicon Homebrew Cellar path, with pinned binary SHA-256 and archive options | `configs/sel4/profiles.toml` |
| seL4 Python environment | Dedicated `out/toolchain/sel4-profile-venv`; two hash locks; exact 38-distribution closure | `configs/sel4/python-bootstrap.lock`, `configs/sel4/python-build-requirements.lock` |
| Pi packaging tool | `mkimage` built from the official DENX U-Boot 2026.01 release tarball | `configs/sel4/profiles.toml` |
| Kernel baseline | Complete upstream seL4Test manifest 16.0.0 source set, including seL4 commit `6e7c3b733d296cfd88d5fbf635c96e447a882374` | `configs/sel4/profiles.toml`, `docs/audit/M26D_SEL4_16_PROVENANCE.md` |
| CAmkES companion | Upstream CAmkES 3.13.0 is the release paired with seL4 16.0.0 | Reference/smoke-test surface only; CAmkES is not a Cohesix build or target dependency |
| Target artifacts | Profile-selected generated headers/configuration, kernel, elfloader, rootserver, system image, and DTB outputs | `SEL4_BUILD_DIR` / `--sel4-build` |

The official kernel interface reference is the
[seL4 Reference Manual 16.0.0](https://sel4.systems/Info/Docs/seL4-manual-16.0.0.pdf).

## 1. Install host and Rust dependencies

The repository script is the canonical setup path:

```bash
./toolchain/setup_macos_arm64.sh
source "$HOME/.cargo/env"
```

It uses Homebrew only for the declared host packages and CPython base, installs
the pinned Rust toolchain and components, verifies and extracts the official Arm
GNU archive, recreates the dedicated hash-locked seL4 Python environment, and
builds `mkimage` from the verified official DENX U-Boot archive. It verifies
`qemu-system-aarch64`; it does not build seL4 or download target artifacts.

Verify the result:

```bash
rustc --version
rustup target list --installed | rg '^aarch64-unknown-none$'
qemu-system-aarch64 --version | head -n 1
cmake --version | head -n 1
ninja --version
/opt/homebrew/Cellar/cpio/2.15/bin/cpio --version | head -n 1
shasum -a 256 /opt/homebrew/Cellar/cpio/2.15/bin/cpio
"$(brew --prefix python@3.13)/bin/python3.13" --version
PROFILE_CC=out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf/bin/aarch64-none-elf-gcc
"$PROFILE_CC" -dumpfullversion
"$PROFILE_CC" -dumpmachine
protoc --version
repo version
out/toolchain/sel4-profile-venv/bin/python -c \
  'import importlib.metadata; print(*(importlib.metadata.version(name) for name in ("sel4-deps", "protobuf", "setuptools")))'
out/toolchain/u-boot-tools-build/tools/mkimage -V
shasum -a 256 out/toolchain/u-boot-tools-build/tools/mkimage
```

`configs/sel4/profiles.toml` pins the compiler archive URL, size, SHA-256
`37084c99bc05fda43a6c48900c638ae4fd6d93e2287ceb3e9bcda55437f1aadd`,
and the SHA-256 of GCC, G++, CPP, assembler, linker, objcopy, ar, and ranlib.
Setup writes `cohesix-compiler-provenance.json` beside the extracted toolchain;
the validator requires the archive, provenance, executable hashes, GCC 15.2.1,
and `aarch64-none-elf` target to agree.

Upstream seL4 archive rules invoke the bare command name `cpio` with GNU-only
`--append` and `--owner` behavior. The profile contract therefore binds GNU
`cpio` 2.15 by its resolved, versioned Homebrew Cellar path and executable
digest, validates every required option, prepends that exact directory to both
configure and build `PATH`, and records the identity in the causal host-input
stamp. The option closure includes `--reproducible`, so a fresh CMake configure
retains deterministic archive metadata. `/usr/bin/cpio` is BSD `cpio` and is
never an eligible substitute.

The Pi host-tool contract downloads the official DENX
`u-boot-2026.01.tar.bz2` archive, verifies size and SHA-256
`b60d5865cefdbc75da8da4156c56c458e00de75a49b80c1a2e58a96e30ad0d54`,
extracts it into `out/toolchain/u-boot-tools-source`, and builds `tools-only`
with deterministic build metadata. The result must report exactly `mkimage
version 2026.01`. Its provenance record binds the source archive, live binary
digest, setup script, and profile contract; every Pi profile validation checks
that record again.

Homebrew's LLVM 17 is available at `/opt/homebrew/opt/llvm/bin` on the standard
Apple Silicon prefix. Add it only for commands that need that toolchain:

```bash
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
```

Do not set model-specific flags such as `target-cpu=apple-m4` in shared scripts
or CI. Host binaries and caches must remain portable across supported Apple
Silicon machines.

## 2. Prepare the external seL4 project source

seL4 source remains outside this repository. A kernel-only checkout is not a
complete Cohesix build input: the seL4Test project, elfloader and support
libraries must come from the same pinned upstream manifest. Parameterize the
complete project location rather than embedding a developer home directory:

```bash
export COHESIX_SEL4_PROJECT="${COHESIX_SEL4_PROJECT:-$PWD/out/sel4/v16-worktree-project}"
mkdir -p "$COHESIX_SEL4_PROJECT"
(
  cd "$COHESIX_SEL4_PROJECT"
  repo init \
    -u https://github.com/seL4/sel4test-manifest.git \
    -b refs/tags/16.0.0
  repo sync -c
)
```

An optional `$HOME/seL4_16` checkout may hold the complete seL4Test 16.0.0
manifest plus its recreated `.venv_aarch64`. When the authenticated Pi overlay
is applied, it is eligible only for the Pi operational source policy; it cannot
replace the pristine QEMU/proof checkout or establish a build claim without a
fresh profile build and validation.

The complete repository revision set is pinned in
`configs/sel4/profiles.toml`. Validate it before configuration; checking only
the kernel `HEAD` is insufficient:

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py configure \
  --profile qemu_smp_diagnostic \
  --source "$COHESIX_SEL4_PROJECT" \
  --build-dir out/sel4/profile-v2/qemu-smp-diagnostic \
  --dry-run
```

Production and proof-eligibility profiles require a clean manifest checkout.
Pi 4 operational profiles permit exactly the recorded VL805 high-BAR overlay
diff and reject any other tracked or untracked source change. Prepare a
separate pristine manifest checkout explicitly; never add the operational
overlay to the clean QEMU/proof source:

```bash
export COHESIX_SEL4_PI4_PROJECT="${COHESIX_SEL4_PI4_PROJECT:-$PWD/out/sel4/v16-pi4-project}"
mkdir -p "$COHESIX_SEL4_PI4_PROJECT"
(
  cd "$COHESIX_SEL4_PI4_PROJECT"
  repo init \
    -u https://github.com/seL4/sel4test-manifest.git \
    -b refs/tags/16.0.0
  repo sync -c
)
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py prepare-source \
  --profile pi4_diagnostic \
  --source "$COHESIX_SEL4_PI4_PROJECT"
```

`prepare-source` first requires every manifest repository to be at its exact
pinned commit and pristine. It then authenticates and applies only
`configs/sel4/patches/bcm2711-vl805-device-untyped.patch`. Repeating the
command is an idempotent no-op only when the checkout contains that exact diff;
wrong revisions, untracked files, staged changes, a changed patch, or any other
source dirt fail before application. `validate` never mutates source. The
proof-eligibility profile rejects that operational overlay because it must stay
source-clean relative to the upstream verified configuration. Do not silently
apply another revision or patch set. The overlay digest is canonicalized with
`git diff --binary --full-index --no-ext-diff --no-renames --no-color
--src-prefix=a/ --dst-prefix=b/` and compared byte-for-byte with the tracked
patch, so local
`core.abbrev`, color, rename, or external-diff settings cannot change the same
patch's identity.
The authenticated diff adds the Cohesix author, purpose, and 2026 copyright
metadata directly to the patched DTS file's native comment header. It therefore
satisfies the repository header contract without adding unauthenticated text
outside the raw diff.

The Homebrew `protobuf` package supplies `protoc`; it does not replace the
Python `google.protobuf` module required by nanopb. The canonical setup script
recreates `out/toolchain/sel4-profile-venv` from
`configs/sel4/python-bootstrap.lock` and
`configs/sel4/python-build-requirements.lock` with `--require-hashes` and
`--no-deps`. The two lock digests and the exact 38-distribution name/version set
are part of the profile contract; an extra, missing, changed, or content-drifted
distribution is rejected.

The closure pins `setuptools==80.9.0` because the upstream nanopb generator
used by this seL4 16 project imports `pkg_resources`. Setup smoke-tests that
import; selecting a newer environment that removes it is a hard provisioning
failure rather than a profile-build workaround.

```bash
out/toolchain/sel4-profile-venv/bin/python -c \
  'import google.protobuf, jsonschema, libarchive, lxml, yaml'
```

The wrapper passes that interpreter to CMake and prepends its `bin` directory to
the artifact-build `PATH`, because nanopb launches its generator through
`/usr/bin/env python3`. The completed build stamp records the interpreter,
executable and installed-file content identities, complete distribution
closure, both lock digests, and exact `PATH` prefixes. The isolated environment
does not write package metadata into either seL4 source checkout.

The upstream
[CAmkES 3.13.0 release](https://docs.sel4.systems/releases/camkes/camkes-3.13.0.html)
is the companion component-framework release for seL4 16.0.0. Cohesix does not
use CAmkES, capDL-generated components, or their Haskell build closure in its
target architecture. A separate CAmkES smoke checkout may test upstream host
compatibility, but it is not part of the pinned Cohesix toolchain, generated
profile evidence, or TCB. The 2026-07-23 macOS smoke configured and compiled
the upstream `adder` example through capDL source generation, then stopped
when the then-unbound BSD `cpio` rejected the upstream GNU-only
`--append --owner=+0:+0` invocation; simulation was not reached. Operational Cohesix
profiles now fail closed unless the pinned GNU tool above wins `PATH`. See
`docs/audit/M26D_SEL4_16_PROVENANCE.md` for the bounded result.

## 3. Select a generated profile intentionally

The source-controlled profile classes have different evidentiary meanings:

| Contract | Intended use | Release/runtime boundary |
| --- | --- | --- |
| `qemu-smp-production` | Four-core QEMU `aarch64/virt`, GICv3, kernel debug/printing disabled | Profile-configuration eligible for release integration; the upstream seL4Test wrapper artifacts remain explicitly non-shipping |
| `qemu-smp-diagnostic` | GICv3 QEMU bring-up and diagnostics | Runtime eligible; never release evidence |
| `pi4-production` | SMP Pi 4 with the recorded VL805 overlay and VCNT-only timing | Release/runtime eligible only after fresh exact-image Pi acceptance |
| `pi4-diagnostic` | Reopened Pi/CYW43 bring-up with kernel diagnostics | Runtime diagnostic only; never release evidence |
| `bcm2711-proof-eligibility` | Pristine upstream AArch64 BCM2711 verified-configuration compatibility | Neither release nor runtime eligible; not a Cohesix proof or boot claim |

Production root-task bootstrap must not depend on debug-only kernel syscalls.
In particular, a successful capability-creating invocation plus generated
bootinfo bounds is the production authority; `DebugCapIdentify` may strengthen
diagnostics when `KernelDebugBuild=ON`, but its absence is not a failed
capability invariant. Bytes emitted before the production PL011 mapping is
admitted stay in the bounded panic/debug buffer until the real sink is
installed.

The five fresh source-build profiles use isolated `profile-v2` trees:

| Directory | Target profile | Important boundary |
| --- | --- | --- |
| `out/sel4/profile-v2/qemu-smp-production` | QEMU production | Canonical static release-profile contract; not boot proof |
| `out/sel4/profile-v2/qemu-smp-diagnostic` | QEMU diagnostic | Diagnostic/runtime profile only |
| `out/sel4/profile-v2/pi4-production` | Pi 4 production | Static release-profile contract; exact-image Pi proof remains separate |
| `out/sel4/profile-v2/pi4-diagnostic` | Pi 4 diagnostic source-build audit | Fresh-source audit lane only; not an input to the CYW43 exact-image lane |
| `out/sel4/profile-v2/bcm2711-proof-eligibility` | BCM2711 proof eligibility | Upstream configuration compatibility only; not a Cohesix proof |

The repo-managed `seL4/` trees remain immutable seL4 16 Milestone 26d
references. Milestone 26e changes the operational scheduler contract to
SMP+MCS, so those classic-scheduler mirrors are no longer current profile
outputs. In particular, `seL4/build_UBOOT` is intentionally rejected by the
current `pi4_diagnostic` contract until the later fresh-Pi phase rebuilds and
deliberately refreshes that tracked artifact set. QEMU-first work uses only the
fresh validated `out/sel4/profile-v2/qemu-smp-*` trees. It must not relabel a
tracked classic artifact, update only its stamp, or use it as MCS evidence.
Pi composition never configures or builds `seL4/build_UBOOT` in place.

These build directories contain generated kernel truth, not vendored seL4
source. The selected directory must match the intended target and root-task
ABI:

```bash
export SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production"
test -f "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"
test -d "$SEL4_BUILD_DIR/libsel4/include"
```

Root-task compilation reads generated headers and configuration from this
directory. Do not combine a kernel or elfloader from one profile with headers,
slot layouts, or root-task artifacts from another.

QEMU launchers inspect the selected seL4 configuration and choose matching
machine details, including the GIC revision. An explicit override is valid only
when it agrees with that generated configuration. In seL4 16,
`QEMU_GIC_VERSION=3` is the platform source selector that chooses `gic_v3.c`;
`KernelArmGicV3=ON` or `QEMU_MACHINE=...,gic-version=3` alone is insufficient.
The profile contract validates the source selector, its derived kernel option,
the generated header consumed by the Cohesix launchers, each DTS, and parsed FDT
semantics from both actual QEMU DTBs as one invariant. Each QEMU DTB must
independently contain GICv3 and PSCI `method=hvc` for the non-hypervisor HVF
machine; existence or a detached DTS is not sufficient. The wrapper replaces
only the upstream QEMU AArch64 PSCI SMP driver so an HVC-selected DTB invokes
HVC `CPU_ON`, while SMC-selected platforms retain the upstream SMC conduit. The profile also binds
`cortex-a57`, `virtualization=off`, scalar `-mgeneral-regs-only` elfloader/libcpio
code, and `TIMER_CLOCK_HZ=24000000`, matching `hw.tbfrequency` on the supported
Apple-Silicon host. A 62.5 MHz or SMC QEMU tree is a TCG comparator, not a
runtime-eligible production input. Pi 4 builds require the generated virtual-counter
export and `TIMER_CLOCK_HZ=54000000`; target timeout logic must not substitute
CPU-speed loops or physical-counter access.

All operational QEMU and Pi profiles select a 14-bit initial root CNode. This
is required by the compiler-owned M26e retention-anchor slots at `0x3f00` and
above; a kernel that falls back to the upstream 13-bit default cannot construct
the critical MCS topology and is not a valid Cohesix runtime profile.

Validate cache, generated JSON, DTS, source provenance and evidence class as
one contract. This command is expected to fail for stale GICv2, old-project or
legacy-domain-schedule trees:

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile qemu_smp_production \
  --build-dir "$SEL4_BUILD_DIR" \
  --source "$COHESIX_SEL4_PROJECT" \
  --require-source \
  --require-artifacts \
  --for-release \
  --evidence out/audit/m26d-profile-v2-qemu-smp-production.json
```

`--for-release` means that the seL4 **profile configuration** is eligible to
enter later Cohesix release integration, and it automatically requires complete
source and artifact proof. The wrapper builds an upstream seL4Test validation
image, not the Cohesix root-task image. Machine evidence therefore reports
`artifact_set_shipping=false` and `cohesix_system_image=false`; a PASS is not a
release publication, boot, or shipping-image W^X result.

## 4. Build the current QEMU profile

The integrated build stages the selected elfloader and kernel, regenerates
manifest outputs, builds the Rust target and host tools, assembles the rootfs,
and launches QEMU. Run it only after the selected generated tree passes its
profile contract:

```bash
./scripts/cohesix-build-run.sh \
  --sel4-build "$SEL4_BUILD_DIR" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features release-qemu,bootstrap-trace \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

`release-qemu,bootstrap-trace` is also the script default. The launcher derives
the interrupt-controller version from the selected seL4 generated headers,
requires GICv3, emits `virt,gic-version=3`, and rejects machine/GIC overrides
from environment or forwarded QEMU arguments. The build regenerates the full
compiler-owned Rust, policy, host-integration, implementation-surface, and
QEMU/Pi Python-contract set before compiling selected artifacts.

Use `--no-run` to stage artifacts without claiming a boot. Use `--transport
qemu` when `cohsh` should own QEMU without exposing the guest TCP listener.
See [Quickstart](QUICKSTART.md) for the verified connection flow.

The production wrapper reserves exactly 7 MiB of file-backed space in its
non-shipping seL4Test placeholder rootserver. The resulting elfloader CPIO must
be at least 8 MiB, and that capacity is part of profile validation and the
causal artifact stamp. `scripts/lib/strip_elfloader_modules.py` replaces the
whole placeholder with the boot-minimized Cohesix root task, including the
linker-retained MCS driver archive; it still fails closed if the replacement
does not fit. The reserve is not copied into the staged Cohesix image and does
not relax the separate rootfs CPIO size guard.

The workspace disables incremental compilation and routes Rust invocations
through `scripts/rustc-wrapper.sh` to avoid APFS temporary-directory races.
Those settings live in `.cargo/config.toml`; do not duplicate them in local or
CI commands.

## 5. Validate toolchain and generated alignment

Before attributing a failure to the target, record:

```bash
git rev-parse HEAD
rustc --version --verbose
qemu-system-aarch64 --version | head -n 1
(cd "$COHESIX_SEL4_PROJECT" && repo manifest -r)
shasum -a 256 "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"
```

Run repository guards from the workspace root:

```bash
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

Target-specific acceptance remains in the staged Test Plan. A host build proves
neither a QEMU boot nor Pi 4 behavior; retain the command, selected profile,
manifest fingerprint, and evidence directory for each claim.

## 6. Rebuilding seL4 profiles

Kernel-profile regeneration is scoped work, not a routine cleanup step. Follow
the active task in [Build plan](BUILD_PLAN.md). Configure only into a new, empty
tree; the profile tool refuses tracked `seL4/` reference trees, non-empty build
directories, pre-existing completion stamps, and pre-existing declared build
outputs. All profiles disable seL4 binary memoization. Build stamps record the
configure/build boundary and causal freshness of each required output, so a
no-op rebuild, copied artifact, or re-stamp cannot qualify as a fresh build.
Changing the pinned GNU `cpio`, setup script, or profile contract requires
rerunning `toolchain/setup_macos_arm64.sh` and configuring a new empty profile
tree; an older provenance record or configured tree is intentionally rejected.

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py configure \
  --profile qemu_smp_production \
  --source "$COHESIX_SEL4_PROJECT" \
  --build-dir out/sel4/profile-v2/qemu-smp-production
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py build \
  --profile qemu_smp_production \
  --source "$COHESIX_SEL4_PROJECT" \
  --build-dir out/sel4/profile-v2/qemu-smp-production
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile qemu_smp_production \
  --build-dir out/sel4/profile-v2/qemu-smp-production \
  --source "$COHESIX_SEL4_PROJECT" \
  --require-source \
  --require-artifacts \
  --for-release \
  --evidence out/audit/m26d-profile-v2-qemu-smp-production.json
```

The historical Pi exact-image lane used the tracked diagnostic artifact tree
directly. Under the active Milestone 26e SMP+MCS contract the following
validation is expected to reject the pre-26e tree; do not proceed to root-task
or image composition until a later fresh-Pi rebuild and controlled tracked-tree
refresh makes it pass:

```bash
.venv/bin/python scripts/sel4_profile.py validate \
  --repo-managed \
  --profile pi4_diagnostic \
  --build-dir seL4/build_UBOOT \
  --require-artifacts \
  --for-runtime \
  --evidence out/audit/pi4-repo-managed-profile.json
```

Before Pi composition is re-enabled, the tracked
`cohesix-profile-build-inputs.json` must have schema
`cohesix-sel4-profile-build-inputs/v2`, status `complete`, profile
`pi4_diagnostic`, and artifact/configuration identities that relocate under
`seL4/build_UBOOT` without any byte change. Validation rejects a dirty tracked
tree, an untracked entry, a changed contract, a missing artifact, or an identity
mismatch. It deliberately does not require the historical absolute source,
CMake, or build paths to exist.

`scripts/pi4-image-build.sh` hashes the complete immutable input tree before
and after composition. It rebuilds the tracked baseline elfloader from the
archived object set as a byte-for-byte toolchain oracle, then creates a new
rootserver archive and relinks only in a disposable composition directory.
Final provenance binds the canonical stamp and tree, exact tool identities and
oracle, and the derived rootserver/newc/wrapper bytes. No CMake or Ninja command
runs against `seL4/build_UBOOT`; no source or seL4 build input below `out/` is
consulted. These diagnostic artifacts and their composition are not release,
media, boot, Wi-Fi, TCP, or benchmark proof.

Wrapper AArch64 contracts set `AARCH64=ON`, and every profile binds
`CROSS_COMPILER_PREFIX=aarch64-none-elf-` before upstream `kernel/gcc.cmake`
selects the compiler. The upstream verified-config script derives its
architecture directly from `KernelSel4Arch=aarch64`, so it does not persist the
wrapper-only `AARCH64` cache key. The contract pins GNU bare-metal version
`15.2.1` and target triple `aarch64-none-elf`, preventing an ambient GNU/Linux,
Homebrew, or other compiler from silently changing outputs. Validation checks
the official archive and provenance plus every required compiler-program hash,
generated CMake metadata, and live resolved C/C++/ASM commands. A compiler
change is intentional profile-contract work, not an automatic acceptance.

The aggregate audit resolves every canonical `default_build_dir` from the
contract and requires all five profile source/artifact sets to pass in one
invocation:

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate --all \
  --require-source --require-artifacts \
  --evidence out/audit/m26d-profile-v2-all.json
```

This command is intentionally fail-closed and requires both complete source
proof and every declared artifact for QEMU production/diagnostic, Pi
production/diagnostic, and BCM2711 proof-eligibility under
`out/sel4/profile-v2/`. Preserved legacy and external exact-image trees cannot
make the aggregate pass. A missing or stale default is a failed closure gate.
Release validation implies `--require-source` and `--require-artifacts`, even
when a caller does not spell out those flags.

For configuration-only diagnosis, relaxation must be explicit and its evidence
is marked accordingly:

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --all --diagnostic-relaxed
```

That command cannot establish milestone closure. Full evidence binds the
Cohesix commit and dirty-state digest; hashes the contract, validator, wrapper,
CMake cache, generated JSON/headers, every DTS/DTB and target artifact; embeds
the expected/observed configuration values; and records compiler, Python,
packaging-tool, causal-freshness, and parsed-DTB evidence. The wrapper also
stores its configure-time SHA-256 in the CMake cache, so replacing it in place
invalidates the build.

Artifact validation is structural before W^X classification: each declared ELF
must be AArch64 `ET_EXEC`, have a nonzero entry point, contain an executable
`PT_LOAD`, and place the entry point inside executable load memory. A malformed
or non-executable file fails even when its artifact class is non-shipping.

The upstream wrapper's kernel, seL4Test rootserver, elfloader, and QEMU combined
image currently contain RWX ELF `LOAD` segments. The profile records an explicit
artifact-by-artifact non-shipping exception. Any future artifact policy marked
shipping-eligible is fail-closed and rejects RWX rather than inheriting that
exception.

Pi wrapper builds additionally bind the tracked
`scripts/aarch64-objcopy-stdout.sh` compatibility wrapper and require the
source-derived executable
`out/toolchain/u-boot-tools-build/tools/mkimage`. The setup script exports the
contract-pinned official DENX U-Boot 2026.01 archive rather than consuming a
vendored checkout, local dirt, or an ignored ambient binary. The profile fails
when either tool or provenance record is absent, when the Python closure or
locks differ, or when `mkimage -V` is not exactly `mkimage version 2026.01`.
Evidence records the resolved paths, digests, provenance, versions, and
executed `PATH` prefixes. That host-tool record and the upstream uImage remain
non-shipping profile evidence; neither establishes an exact Cohesix Pi image,
board boot, Wi-Fi repeatability, TCP/`cohsh`, or benchmark proof.

The proof-eligibility lane executes the upstream verified-configuration script
from a pristine pinned checkout. It deliberately produces static evidence and
does not publish a Cohesix runtime image. The profile tool forwards the same
`aarch64-none-elf-` compiler prefix into the verified script's inner CMake
configure:

```bash
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py build \
  --profile bcm2711_proof_eligibility \
  --source "$COHESIX_SEL4_PROJECT" \
  --build-dir out/sel4/profile-v2/bcm2711-proof-eligibility
out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile bcm2711_proof_eligibility \
  --build-dir out/sel4/profile-v2/bcm2711-proof-eligibility \
  --source "$COHESIX_SEL4_PROJECT" \
  --require-source \
  --require-artifacts
```

Its required static artifact set is the upstream kernel and its parsed kernel
DTB; neither is a Cohesix runtime or shipping-image claim.

Only publish generated reference outputs after validation and target-specific
review. Do not hand-edit `CMakeCache.txt`, generated headers, generated JSON,
DTS/DTB, kernel or elfloader artifacts. In particular:

- keep `ElfloaderRootserversLast=ON` for the accepted seL4 16 profiles;
- keep Pi 4 `IMAGE_START_ADDR=0x10000000` for the U-Boot `bootm` XIP handoff;
- when both settings are active, let the profile wrapper regenerate
  `elfloader/gen_headers/platform_info.h` from the kernel-generated
  `platform_gen.yaml` before elfloader compilation. Upstream emits an empty
  platform header for a fixed image address, but the rootservers-last path
  requires its `memory_region` declaration;
- treat a missing or empty rootservers-last memory map as profile-validation
  failure after a build;
- reject every `KernelDomainSchedule` cache entry, including an empty one;
- require `QEMU_GIC_VERSION=3`, its derived kernel option, generated
  configuration, DTS/DTBs and launcher selection to agree on GICv3 before the
  tree is canonical;
- generate QEMU DTB/PSCI settings from the selected profile rather than copying
  values between QEMU and Pi builds;
- store transient outputs under `out/` and replace tracked reference outputs
  only after profile guards pass;
- use `scripts/lib/strip_elfloader_modules.py` through the integrated harness so
  staged elfloaders do not retain an upstream test rootserver; keep the
  production profile's declared 8 MiB minimum archive capacity and fail rather
  than enlarging a linked elfloader in place.

The detailed image, flash, boot, and current-image evidence procedure is in
[Hardware bring-up](HARDWARE_BRINGUP.md).
