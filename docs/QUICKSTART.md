<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Guide Mac, Linux and Pi 4 users from a verified release to an authenticated console. -->
<!-- Author: Lukas Bower -->

# Cohesix quickstart

Cohesix is a control-plane OS that runs in QEMU or on a Raspberry Pi 4. Its
shell, gateway, Python client and desktop UI run on your Mac or Linux host.
This guide gets you from a release archive to an authenticated console, then
shows how several clients can share one target through the gateway.

## Choose your download

Use all files from the same release. For **1.0.0-beta**:

| You want to… | Download | What it contains |
| --- | --- | --- |
| Run QEMU or operate a Pi from an Apple Silicon Mac | `Cohesix-1.0.0-beta-MacOS.tar.gz` | Mac binaries, a Mac QEMU guest, Python wheel and runtime setup |
| Run QEMU or operate a Pi from Linux ARM64, including Jetson | `Cohesix-1.0.0-beta-linux.tar.gz` | Linux binaries, a Linux QEMU guest, Python wheel and runtime setup |
| Boot a physical Raspberry Pi 4 | `Cohesix-1.0.0-beta-Pi4.tar.gz` **plus your host's archive above** | A complete SD-card image, image metadata and documentation |

Each archive contains `QUICKSTART.md`, `README.md`, `RELEASE_NOTES.md`,
`VERSION.txt` and `MANIFEST.sha256`. The Pi archive has no `bin/`, Python
runtime or `qemu/run.sh`; run the host tools from the Mac or Linux archive.
You do not need Rust, seL4 build tools, or a GPU to use the prebuilt host tools.
For a source checkout, use [Build from source](#build-from-source) below.

## 1. Extract and verify

Use a new directory for each archive. The examples use **Bash**; on a Mac,
enter `bash` in Terminal first. In each additional terminal, use Bash and
change into the same extracted host bundle. Replace archive names with the
one you downloaded; do not paste angle-bracket placeholders literally.

```bash
mkdir -p "$HOME/cohesix-releases"
cd "$HOME/cohesix-releases"
tar -xzf "$HOME/Downloads/Cohesix-1.0.0-beta-MacOS.tar.gz"
cd Cohesix-1.0.0-beta-MacOS
```

Before running anything, verify **all** manifest entries:

```bash
# macOS
shasum -a 256 --check MANIFEST.sha256
```

```bash
# Linux
sha256sum --check MANIFEST.sha256
```

Every entry must report `OK`. Check `VERSION.txt` and `RELEASE_NOTES.md` for
the intended release and its limitations. Obtain archives from the trusted
release publisher; the internal hashes detect changed files, not publisher
identity. Keep the archive so you can make another clean extraction.

## 2. Set up the Mac or Linux host

Run these commands from the **host bundle root**, including when your target
is a Pi. Installation may download host packages and ask for administrator
access; run the script as your normal user:

```bash
./scripts/setup_environment.sh
./scripts/setup_environment.sh --check
source .venv/bin/activate
```

The script installs runtime dependencies and the bundled Python wheel into
this bundle's `.venv`. It does not build Cohesix or install CUDA drivers.
`--check` verifies an already prepared installation without installing packages.

| Host | Requirements and behavior |
| --- | --- |
| Mac | macOS 26 or later, Apple Silicon, and Homebrew available if packages need installing. QEMU must advertise HVF. Use a native ARM64 terminal, not Rosetta. |
| Linux | Ubuntu 22.04, 24.04 or 26.04 on ARM64. Setup uses apt and enables Universe for required runtime packages, including WebKitGTK 4.1. Other distributions and x86-64 are not supported by this installer. |
| Linux QEMU | For the native release profile, `/dev/kvm` must be readable/writable by your user and the host counter must be 31.25 MHz. The Linux guest is built for that counter; the Mac guest is built for 24 MHz. A successful package install alone does not check KVM eligibility. |
| NVIDIA host | Jetson is one reference Linux ARM64 host. Optional GPU discovery needs the host's compatible CUDA/NVML stack. Keep Jetson's board-managed driver packages; the runtime installer does not replace them. |

Verify the tools without a target:

```bash
./bin/coh doctor --mock
./bin/cohsh --transport mock --role queen
```

At the `coh>` prompt, try `help`, `ls /`, `cat /proc/boot`, then `quit`.
Mock output is simulated. You can also create a sample evidence pack and run a
packaged Python example:

```bash
./bin/coh evidence pack --mock --out out/evidence/quickstart-mock
./bin/coh evidence timeline --input out/evidence/quickstart-mock
python python/cohesix-py/examples/lease_run.py --mock
```

For a physical Pi, continue with [Install the Pi image](#4-install-the-pi-4-sd-image).

## 3. Boot the QEMU guest

In terminal 1, from the host bundle root:

```bash
./qemu/run.sh
```

Leave it running. The launcher uses four cores and GICv3, with HVF on Mac and
KVM on eligible Linux hosts. Wait for `[mark] root-console.start.ok` and the
`cohesix>` prompt. The serial console supports `ping`, `bi`, `caps mcs`,
`smp mcs`, `mem` and `netstats` for boot and network inspection.

The default forwarded endpoints are `127.0.0.1:31337` (TCP console), UDP
31338 and TCP 31339. If occupied, stop the prior instance or select free host
ports, for example:

```bash
TCP_PORT=32337 UDP_PORT=32338 SMOKE_PORT=32339 ./qemu/run.sh
```

Use the chosen TCP port when connecting below. Do not change CPU, timer, core
count or machine options to get around a failed native boot. A Linux fallback
to TCG is a slower diagnostic run and does not establish native release
acceptance. A different Linux counter frequency needs a compatible guest build.

Continue with [Connect to your target](#5-connect-to-your-target), using
`127.0.0.1` and port `31337` unless you changed the host port.

## 4. Install the Pi 4 SD image

Use a Pi 4, a suitable power supply, an SD card and reader, an HDMI display and
a USB keyboard. Connect Ethernet to your local network, or use the boot menu
to configure Wi-Fi. The host computer must be able to reach the selected Pi
address. Cohesix boots directly on the Pi; you do not first install Raspberry
Pi OS or run the Linux host bundle on the bare Cohesix target.

Extract the `-Pi4` archive and verify its `MANIFEST.sha256` as in step 1.
The following commands run from that **Pi bundle root**. Verify the image
sidecar too:

```bash
# macOS
(cd image && shasum -a 256 --check cohesix-pi4-sd.img.sha256)
# Linux: use sha256sum --check in the same image directory instead.
```

`image/cohesix-pi4-sd.img` is the complete raw MBR/FAT32 image. Read
`image/cohesix-pi4-sd.json`: the card's byte capacity must be at least
`minimum_target_bytes`. Extra capacity on larger cards remains unallocated;
no expansion is needed. Flashing replaces **the whole card**, including any
saved network settings. Back up anything you need before proceeding.

### Write and read back on Mac

Discover the removable card again after insertion. Replace `diskN` below with
that exact whole disk; check its model, removable status and byte size using
`diskutil info`. The commands erase the selected disk:

```bash
diskutil list external physical
diskutil info /dev/diskN
diskutil unmountDisk /dev/diskN
sudo dd if=image/cohesix-pi4-sd.img of=/dev/rdiskN bs=4m
sync
IMAGE_BYTES=$(stat -f %z image/cohesix-pi4-sd.img)
sudo cmp -n "$IMAGE_BYTES" image/cohesix-pi4-sd.img /dev/rdiskN
```

Readback succeeds only if `cmp` exits zero without output. Do not boot on a
write or comparison error. Before ejecting, mount the new boot partition with
`diskutil mountDisk /dev/diskN` if you need the console credential described
below. Then unmount and eject:

```bash
diskutil eject /dev/diskN
```

### Write and read back on Linux

Identify the removable whole disk by model, transport and byte size. It may
be `/dev/sdX` or `/dev/mmcblkN`. Unmount **each mounted partition shown by
lsblk**; for example, use `sudo umount /dev/sdX1` for that listed partition.
Substitute the verified whole disk in the write and readback commands:

```bash
lsblk --bytes --output NAME,SIZE,TYPE,TRAN,MODEL,MOUNTPOINTS
sudo dd if=image/cohesix-pi4-sd.img of=/dev/sdX bs=4M conv=fsync status=progress
IMAGE_BYTES=$(stat -c %s image/cohesix-pi4-sd.img)
sudo cmp -n "$IMAGE_BYTES" image/cohesix-pi4-sd.img /dev/sdX
```

Require a successful write and a zero-exit, silent comparison. If you need the
console credential below, reinsert the reader so the new partition table is
recognised, then mount the FAT partition using your desktop disk utility.
Unmount it after reading and eject or safely remove the card. Do not use a
partition such as `/dev/sdX1` as the destination for the raw image.

### First boot and network configuration

Before removing the card from the workstation, obtain the target's console
credential from the mounted FAT volume: in `cohesix-root-task-resolved.json`,
find the `tickets` entry with `role` equal to `queen` and retain its `secret`
securely for step 5. This is distinct from the Wi-Fi password. The Pi image's
credential can differ from the QEMU bundle's; use the Pi's own manifest.

Insert the ejected card into the Pi, attach HDMI and the USB keyboard, then
power it on. The image stops at **Cohesix boot menu**; it does not automatically
skip network setup. For a first installation:

1. Select **2 — Change network settings**.
2. Select **Automatic (DHCP)**, or **Manual (static IPv4)** with a valid address,
   subnet prefix and optional gateway for your network.
3. Select **Ethernet (wired)** or **Wi-Fi (wireless)**. For Wi-Fi, enter the SSID
   and password on the attached USB keyboard. They are visible on the local
   display and hidden from serial output; do not enter passwords over serial.
4. On **Review network settings**, select **2 — Save settings and restart**.
5. After restart, verify **Saved network settings loaded**, then select
   **1 — Boot with saved settings**.

The **Boot once without saving** option is temporary. Saving creates
`cohesix.env` on the card; treat that file and the card as credentials. Changing
networks later uses the same menu, without reflashing. If keyboard setup is
unavailable, the bounded offline `cohesix.env` procedure is in
[First-boot network policy](HARDWARE_BRINGUP.md#4-set-first-boot-network-policy).

Wait for the `cohesix>` console and inspect `netstats`. For DHCP, find the
assigned address in the boot/network output or your router's lease table;
for static mode, use the address you configured. Confirm the active interface
and address before connecting. A blank display or missing USB input is a
boot/input problem, not a reason to guess network settings. A correctly wired
serial console at 115200 baud can retain boot output; use one serial owner
and follow [Hardware Bring-up](HARDWARE_BRINGUP.md) for capture and diagnostics.

## 5. Connect to your target

In terminal 2, change into the **Mac or Linux host bundle**. For QEMU, the
compiled Queen console credential is the `secret` in the `tickets` entry with
`role` equal to `queen` in `configs/generated/root_task_resolved.json`. For Pi, use
the credential from its card as described above. Open the manifest in a local
editor; do not print secrets into shared logs. Credentials distributed in a
public evaluation image are shared, not private deployment credentials.
Changing a host environment variable or editing the copied manifest does not
change the secret already compiled into the target.

Enter the target credential without putting it in shell history (Bash):

```bash
read -r -s -p 'Target console token: ' COHSH_AUTH_TOKEN
printf '\n'
export COHSH_AUTH_TOKEN
export COH_AUTH_TOKEN="$COHSH_AUTH_TOKEN"
TARGET_HOST=127.0.0.1
TARGET_PORT=31337
```

For Pi, replace `127.0.0.1` with its actual IP address. For an alternate QEMU
forward, use the selected host TCP port. Connect:

```bash
./bin/cohsh --transport tcp --tcp-host "$TARGET_HOST" \
  --tcp-port "$TARGET_PORT" --role queen
```

At `coh>`, try these read-only observations:

```text
ping
ls /
cat /proc/boot
cat /proc/root/reachable
cat /proc/schedule/summary
cat /proc/lease/summary
ls /shard
quit
```

Success means the authenticated session attached, the reads returned bounded
responses, and `/proc/boot` identifies the intended target. `quit` closes the
session; it does not power off the Pi. The canonical Worker namespace is
`/shard`. Runtime observations and mock output are different proof sources;
this quickstart is not a complete hardware or performance qualification.

TCP authentication provides no encryption. Keep QEMU forwards on loopback.
Use a private controlled network for Pi, or terminate an encrypted tunnel/VPN
on a host gateway. Only one direct TCP client may own the target at a time.

## 6. Share the target through the gateway

Close the direct `cohsh` session first. In terminal 2, retaining the target
address and credential above, choose a separate strong request token for gateway
writes and start the gateway:

```bash
read -r -s -p 'New gateway request token: ' HIVE_GATEWAY_REQUEST_AUTH_TOKEN
printf '\n'
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN
export COH_REST_URL=http://127.0.0.1:8080
./bin/hive-gateway --bind 127.0.0.1:8080 \
  --tcp-host "$TARGET_HOST" --tcp-port "$TARGET_PORT"
```

In terminal 3, from the host bundle root:

```bash
export COH_REST_URL=http://127.0.0.1:8080
curl --fail --silent --show-error "$COH_REST_URL/v1/meta/status"
./bin/cohsh --transport rest --rest-url "$COH_REST_URL" --role queen
```

Require `connected: true` before using the REST-backed shell. These clients
share the gateway's upstream role and ticket. For writes, securely set the
same `HIVE_GATEWAY_REQUEST_AUTH_TOKEN` in the client terminal; a new terminal
does not inherit variables exported in terminal 2.

After quitting the shell, launch the desktop UI through that same gateway:

```bash
SWARMUI_TRANSPORT=rest SWARMUI_REST_URL="$COH_REST_URL" ./bin/swarmui
```

SwarmUI needs a graphical desktop. On a headless Jetson, use the CLI/REST
clients or an existing remote desktop; `xvfb-run` provides an off-screen test
display, not a visible desktop. Python users can activate `.venv` in this
terminal and follow [Python support](PYTHON_SUPPORT.md) for `RestBackend`.
For optional real NVIDIA inventory, run `./bin/gpu-bridge-host --list` on the
Linux GPU host; [Host tools](HOST_TOOLS.md) covers publishing it through the
gateway. GPU discovery does not establish model execution.

When finished, quit clients, stop the gateway with `Ctrl-C`, then exit QEMU
with `Ctrl-A`, followed by `X`. Avoid cutting Pi power while its boot menu is
saving settings. Clear credentials from each terminal that used them:

```bash
unset COHSH_AUTH_TOKEN COH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

## If something fails

| Symptom | Next check |
| --- | --- |
| Manifest or image readback mismatch | Stop. Re-extract a trusted archive, or rediscover and rewrite the intended card; do not boot mismatched media. |
| Wrong executable format or GLIBC error | Use the matching native ARM64 bundle and a compatible Ubuntu runtime. Do not copy individual binaries between releases. |
| Missing Python, QEMU or WebKit library | Run the host setup script, then `--check`. Activate this extraction's `.venv` for Python. |
| HVF/KVM unavailable, or TCG fallback | Check native architecture, QEMU accelerator support and Linux `/dev/kvm` permissions. Keep the guest's declared timer profile. |
| Pi remains in the menu | Select the displayed boot action after saving/restarting. Check whether the menu reports saved or default settings. |
| TCP refused or timeout | Keep QEMU running, check its forwarded port, or verify the Pi's selected interface/IP and host route. Check for a previous direct client. |
| Missing credential or `ERR AUTH` | Use the Queen secret from the exact target manifest. Placeholder credentials are rejected; gateway request tokens and Wi-Fi passwords are different credentials. |
| Busy console or gateway disconnected | Quit direct clients, leave one gateway as TCP owner, and connect additional clients through REST. |

See [Userland and CLI](USERLAND_AND_CLI.md) for commands,
[Hardware Bring-up](HARDWARE_BRINGUP.md) for Pi diagnostics, and
[Host tools](HOST_TOOLS.md) for mounts, GPU bridges, tickets and evidence packs.

## Build from source

This section requires a source checkout; the runtime bundles intentionally omit
the build toolchain. Follow the [source README](../README.md#build-the-current-source-tree)
for the Mac or Linux installer. The Mac installer creates the canonical seL4
profile. For a Mac source build, activate the tools from the repository root:

```bash
source "$HOME/.cargo/env"
source .venv/bin/activate
export SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production"
./scripts/cohesix-build-run.sh \
  --sel4-build "$SEL4_BUILD_DIR" --out-dir out/cohesix \
  --profile release --root-task-features release-qemu,bootstrap-trace \
  --cargo-target aarch64-unknown-none --transport tcp
```

Use `out/cohesix/host-tools/` in place of the release's `bin/` for host commands.
The Linux installer supplies host tools and diagnostic QEMU; the native Linux
release lane additionally requires its own built 31.25 MHz KVM seL4 profile.
See the [release factory](HOST_TOOLS.md#release-factory) for native builds and
release qualification. Do not mix checkout artifacts with an extracted release.
