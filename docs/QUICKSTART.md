<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide the shortest safe path from a current Cohesix source checkout to a verified mock or QEMU session. -->
<!-- Author: Lukas Bower -->

# Quickstart — Current Source Tree

This guide exercises the current checkout. A versioned release bundle is an
immutable snapshot with its own `QUICKSTART.md`; use the instructions inside
that bundle instead of mixing release binaries with current-tree configuration.

Cohesix is a pre-production research OS. QEMU is the reference development
target, while Raspberry Pi 4 acceptance requires a separate, image-qualified
hardware workflow. A successful build or QEMU boot is not Pi 4 proof.

See the [Glossary](GLOSSARY.md) whenever a Cohesix-specific term is unfamiliar.

## Choose a first run

| Goal | Path | Target required |
| --- | --- | --- |
| Inspect the shell and namespace safely | [Mock session](#1-run-a-mock-session) | None |
| Build and boot the current VM | [QEMU session](#2-build-and-boot-qemu) | External seL4 build outputs |
| Share one target with concurrent host clients | [REST gateway](#3-add-the-rest-gateway-optional) | Running mock or QEMU backend |
| Bring up a Pi 4 | [Hardware bring-up](HARDWARE_BRINGUP.md) | Pi 4 and verified removable media |

## Prerequisites

From the repository root, run the installer for your host. Each installer pins
Rust 1.97.1 and creates the repository `.venv`.

macOS 26 or later on Apple Silicon is the primary, fully pinned seL4 build
host:

```bash
./toolchain/setup_macos_arm64.sh
```

Ubuntu 22.04, 24.04, or 26.04 on ARM64 supports Cohesix host-tool builds and
diagnostic QEMU/TCG runs:

```bash
./toolchain/setup_linux_arm64.sh
```

After either installer:

```bash
source "$HOME/.cargo/env"
source .venv/bin/activate
```

The Linux path does not construct the pinned macOS seL4 compiler/profile
inputs. It can consume an explicitly supplied, compatible seL4 output tree for
diagnostic QEMU, but that run is not target or release acceptance.

For a QEMU target run, the selected seL4 16.0.0 output directory must already
contain the kernel, elfloader, generated headers, and configuration for that
profile. The canonical operational/release input is
`out/sel4/profile-v2/qemu-smp-production`; preserved `seL4/*` trees are
explicit archived diagnostic inputs only. The complete construction and
validation contract is in [Toolchain setup](TOOLCHAIN_MAC_ARM64.md) and
[Hardware bring-up](HARDWARE_BRINGUP.md#qemu-runbook).

## 1. Run a mock session

Mock mode exercises host parsing and namespace behavior without booting seL4.
It is useful for orientation, not target acceptance.

```bash
cargo run -p cohsh -- --transport mock --role queen
```

At the `coh>` prompt:

```text
help
ls /
cat /proc/root/reachable
quit
```

For deterministic host preflight and an isolated evidence sample:

```bash
cargo run -p coh -- doctor --mock
cargo run -p coh -- evidence pack --mock --out out/evidence/quickstart-mock
```

Mock success proves only the host-side mock path.

## 2. Build and boot QEMU

From the repository root, select the seL4 output explicitly and launch the
authenticated TCP-console profile:

```bash
export SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production"
test -f "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"

./scripts/cohesix-build-run.sh \
  --sel4-build "$SEL4_BUILD_DIR" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features release-qemu,bootstrap-trace \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

The selected runtime profile is four-core SMP+MCS with GICv3. The build script
validates that generated kernel truth and refuses forwarded machine or GIC
overrides before it assembles or launches an image.

Leave QEMU running. In a second terminal, enter the Queen console secret from
the selected deployment manifest without placing it in shell history:

```bash
read -r -s COHSH_AUTH_TOKEN
export COHSH_AUTH_TOKEN

out/cohesix/host-tools/cohsh \
  --transport tcp \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337 \
  --role queen

unset COHSH_AUTH_TOKEN
```

The selected manifest's Queen ticket secret is the TCP console authentication
secret. Replace development secrets before any shared deployment. The literal
placeholder `changeme` is rejected by current target and client paths.

Run a bounded smoke check at the prompt:

```text
ls /
cat /proc/boot
cat /proc/root/reachable
test --mode quick --no-mutate
quit
```

The direct TCP console is authenticated but not encrypted. Keep it on loopback
or carry it through an authenticated encrypted tunnel. Only one direct console
owner may attach at a time.

### Alternative: tool-owned QEMU

To avoid exposing the TCP listener to the host, stage without running and let
`cohsh` own QEMU:

```bash
./scripts/cohesix-build-run.sh \
  --sel4-build "$PWD/out/sel4/profile-v2/qemu-smp-production" \
  --out-dir out/cohesix \
  --profile release \
  --cargo-target aarch64-unknown-none \
  --transport qemu \
  --no-run

out/cohesix/host-tools/cohsh \
  --transport qemu \
  --qemu-out-dir out/cohesix \
  --role queen
```

The launcher derives target details from the selected seL4 build. Do not force
a GIC or machine profile that disagrees with its generated configuration.

## 3. Add the REST gateway (optional)

Use `hive-gateway` when multiple REST clients need one bounded projection of a
single target console session. Stop any direct `cohsh`, SwarmUI, or bridge
owner first; the gateway must become the sole TCP-console owner.

Create distinct console and REST request secrets without echoing them:

```bash
read -r -s COH_AUTH_TOKEN
export COH_AUTH_TOKEN
read -r -s HIVE_GATEWAY_REQUEST_AUTH_TOKEN
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN

out/cohesix/host-tools/hive-gateway \
  --bind 127.0.0.1:8080 \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337
```

In another terminal, read a bounded public metadata endpoint:

```bash
curl --fail --silent --show-error \
  http://127.0.0.1:8080/v1/meta/bounds
```

Mutating routes require the separate request token in an `Authorization:
Bearer` or `x-cohesix-auth` header. Follow a task-specific recipe rather than
issuing an arbitrary control write; [API guidelines](API_GUIDELINES.md) owns the
HTTP contract and [Operator recipes](OPERATOR_RECIPES.md) provides bounded
workflows.

Unset both variables after stopping the gateway:

```bash
unset COH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

The gateway defaults to loopback and does not terminate TLS. Do not expose it
directly to an untrusted network. See [API guidelines](API_GUIDELINES.md) for
authentication, read classes, error mapping, and safe reverse-proxy boundaries.

## 4. Understand the proof you have

Keep these states separate:

- source build completed;
- QEMU booted the staged image;
- authenticated console commands completed;
- a specific Pi 4 image was flashed and read back;
- that same image booted on hardware;
- its selected wired or Wi-Fi path reached DHCP/TCP;
- an evidence pack or benchmark report was captured for that run.

Record the selected manifest fingerprint, seL4 output directory, commit, exact
command, and evidence path for any result you intend to compare or publish.

## Next steps

- Follow the ordered [Operator walkthrough](OPERATOR_WALKTHROUGH.md).
- Use [Operator recipes](OPERATOR_RECIPES.md) for mounts, evidence, lifecycle,
  host tickets, federation, and PEFT workflows.
- Review [Userland and CLI](USERLAND_AND_CLI.md) before writing `.coh` scripts.
- Use [Failure modes](FAILURE_MODES.md) when a gate fails.
- Use [Hardware bring-up](HARDWARE_BRINGUP.md) for Pi 4 work.
