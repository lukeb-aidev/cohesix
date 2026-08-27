#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build, optionally flash, capture, and normalize Raspberry Pi 4 USB/WiFi gate proofs.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

IMAGE_BUILD_SCRIPT="${SCRIPT_DIR}/pi4-image-build.sh"
TRACE_NORMALIZER="${SCRIPT_DIR}/pi4_trace_normalize.py"
MANIFEST_PATH="${ROOT_DIR}/configs/root_task_pi4_uboot_aarch64.toml"
VENV_DIR="${COHESIX_PI4_VENV:-${ROOT_DIR}/.venv}"
PYTHON="${VENV_DIR}/bin/python"
FLASH_DISK=""
DISK_LABEL="COHESIX"
SERIAL_DEVICE="${COHESIX_PI4_SERIAL_DEVICE:-/dev/cu.usbserial-0001}"
DEFAULT_LOG_PATH="/Users/lukasbower/pi4-serial-$(date +%Y%m%d-%H%M%S).log"
LOG_PATH="${COHESIX_PI4_SERIAL_LOG:-${DEFAULT_LOG_PATH}}"
RUNTIME_DMA_PROOF_PATH="${COHESIX_PI4_RUNTIME_DMA_PROOF:-}"
CYW43_COEXISTENCE_RECORD_PATH="${COHESIX_PI4_CYW43_COEXISTENCE_RECORD:-}"
BOOT_WAIT_SECONDS=12
CONSOLE_READY_TIMEOUT_SECONDS=60
CAPTURE_SECONDS=10
COMMAND_DELAY_SECONDS=2
COMMAND_CHAR_DELAY_SECONDS="${COHESIX_PI4_COMMAND_CHAR_DELAY_SECONDS:-0.06}"
COMMAND_PROMPT_TIMEOUT_SECONDS=30
WIFI_SUPERVISOR_TIMEOUT_SECONDS=180
WIFI_DHCP_TIMEOUT_SECONDS=60
GATEWAY_READY_TIMEOUT_SECONDS="${COHESIX_PI4_GATEWAY_READY_TIMEOUT_SECONDS:-60}"
SKIP_BUILD=0
NO_CAPTURE=0
NORMALIZE_ONLY=0
ALLOW_SUMMARY_ONLY=0
REQUIRE_USB_READY=0
REQUIRE_WIFI_READY=0
REQUIRE_WIRED_READY=0
REQUIRE_DRIVER_TASK_PROOF=0
REQUIRE_INPUT_RESPONSIVE=0
GATEWAY_STATUS_URL=""
GATEWAY_STATUS_ENDPOINT=""
GATEWAY_TARGET_HOST=""
GATEWAY_TARGET_PORT=31337
GATEWAY_REQUEST_AUTH_TOKEN="${COHESIX_PI4_GATEWAY_REQUEST_AUTH_TOKEN:-${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}}"
GATEWAY_START_CAPTURED_UNIX_NS=""
GATEWAY_START_CONNECTED=""
GATEWAY_START_CONNECTS=""
GATEWAY_START_RECONNECTS=""
GATEWAY_START_LAST_CHANGE_UNIX_MS=""
GATEWAY_START_TARGET_HOST=""
GATEWAY_START_TARGET_PORT=""
GATEWAY_END_CAPTURED_UNIX_NS=""
GATEWAY_END_CONNECTED=""
GATEWAY_END_CONNECTS=""
GATEWAY_END_RECONNECTS=""
GATEWAY_END_LAST_CHANGE_UNIX_MS=""
GATEWAY_END_TARGET_HOST=""
GATEWAY_END_TARGET_PORT=""

DEFAULT_COMMANDS=(
    "netstats"
    "smp activity"
    "nettest"
    "netstats"
    "wifi diag"
    "usb diag"
    "usb status"
)
EXTRA_COMMANDS=()
EXPECTATIONS=()
MIN_EXPECTATIONS=()
NOT_EXPECTATIONS=()
CAPTURE_PID=""
SERIAL_CAPTURE_ERROR_LOG=""
NETWORK_INTERFACE=""
NETWORK_CAPTURE_PATH=""
NETWORK_CAPTURE_TEMP_PATH=""
NETWORK_CAPTURE_TEMP_SEAL=""
NETWORK_CAPTURE_PID=""
NETWORK_CAPTURE_ERROR_LOG=""
NETWORK_CAPTURE_STARTED_AT_UTC=""
NETWORK_CAPTURE_FINISHED_AT_UTC=""
NETWORK_CAPTURE_STARTED_UNIX_NS=""
NETWORK_CAPTURE_FINISHED_UNIX_NS=""
NETWORK_CAPTURE_ID=""
NETWORK_CAPTURE_CONTROLLED=0
LOG_OUTPUT_SEAL=""
NETWORK_OUTPUT_SEAL=""
RUNTIME_OUTPUT_SEAL=""
CYW43_OUTPUT_SEAL=""
SERIAL_SNAPSHOT_PATH=""
NETWORK_CAPTURE_SNAPSHOT_PATH=""
RUNTIME_PROOF_SNAPSHOT_PATH=""

usage() {
    cat <<'USAGE'
Usage: scripts/pi4_gate_proof.sh [options]

Builds and stages the Pi 4 payload, optionally flashes an SD card, captures the
Cohesix serial proof commands, and summarizes the current USB/WiFi gates.

Options:
  --manifest <path>          Root-task Pi 4 manifest
                             (default: configs/root_task_pi4_uboot_aarch64.toml)
  --venv <dir>               Python virtualenv for local scripts
                             (default: <repo>/.venv)
  --flash-disk <device|auto> Flash SD card via scripts/pi4-image-build.sh.
                             "auto" requires exactly one external disk carrying
                             the configured --disk-label.
  --disk-label <name>        FAT32 label used when flashing or auto-detecting
                             (default: COHESIX)
  --serial-device <path>     Serial device for Cohesix console
                             (default: /dev/cu.usbserial-0001)
  --log <path>               Serial log output/input path
                             (default: /Users/lukasbower/pi4-serial-<timestamp>.log)
                             Active capture refuses existing paths; use
                             --normalize-only or --no-capture for existing logs.
  --runtime-dma-proof-out <path>
                             Write an env-style Pi runtime/DMA proof artifact
                             after successful normalization. Defaults to a
                             sibling file next to --log when driver-task proof
                             is required.
  --network-interface <name>
                             Capture the selected boot's packets concurrently
                             with serial diagnostics on this host interface.
  --network-capture-out <path>
                             Fresh pcap output paired with --network-interface.
                             Existing paths and offline/normalize-only use fail.
  --gateway-status-url <url>
                             Require one unchanged gateway connection across
                             the active controlled capture. Accepts an HTTP(S)
                             base URL or its exact /v1/meta/status endpoint.
                             Request auth is read only from the supported env
                             token variables and is never retained in proof.
  --gateway-target-host <ip>
                             Canonical Pi IP reached by that gateway session.
                             Required with --gateway-status-url and retained
                             for serial and benchmark cross-validation.
  --cyw43-coexistence-record-out <path>
                             Atomically publish a positive exact-image CYW43
                             record after WiFi, driver, and controlled paired
                             capture gates all pass. Existing paths fail.
  --boot-wait <seconds>      Delay before issuing console commands
                             (default: 12)
  --console-ready-timeout <seconds>
                             Maximum extra time to wait for the Cohesix prompt,
                             advancing the top-level Cohesix boot menu
                             with its default choice when needed
                             (default: 60)
  --capture-seconds <n>      Delay after the final command before normalization
                             (default: 10)
  --command-delay <seconds>  Delay between console commands
                             (default: 2)
  --command-char-delay <seconds>
                             Delay between characters while sending commands
                             (default: 0.06)
  --command-prompt-timeout <seconds>
                             Maximum time to wait for the prompt after each
                             command before sending the next command
                             (default: 30)
  --skip-build               Reuse existing seL4 image while staging/flashing
  --no-capture               Do not open serial; normalize the existing log
  --normalize-only           Skip build, flash, and capture; normalize only
  --no-default-commands      Do not send the default proof commands
  --probe-usb-keyboard       Explicitly append one active USB keyboard probe.
                             Passive default diagnostics never probe hardware.
  --command <line>           Append a console command to send during capture
  --expect <KEY=VALUE>       Require a gate summary value from the normalizer.
                             Examples: USB_GATE=3, WIFI_BLOCKER=ht-clock-timeout
  --expect-min <KEY=VALUE>   Require a numeric gate to be at least VALUE.
                             Example: USB_GATE=3 accepts USB_GATE=4.
  --expect-not <KEY=VALUE>   Fail if a gate summary value still equals VALUE.
                             Example: USB_BLOCKER=cmd-poll-only-timeout.
  --allow-summary-only       Do not require USB/WiFi evidence gates. This is
                             for exploratory summaries only, not proof output.
  --require-usb-ready        Require current USB owner/descriptor proof,
                             startup gate 10, an exact one-deep queue, and a
                             fresh post-diag key-path proof.
  --require-wifi-ready       Require WiFi gate 10, DHCP, nettest, authenticated
                             TCP bytes, healthy DPC, ordered Gate 7a-7e proof,
                             and the linked old-good CYW43 replay contract.
  --require-wired-ready      Require netstats to report active=wired.
  --require-driver-task-proof
                             Require driver-task substrate, capset, fault,
                             revoke, scheduling, per-driver affinity,
                             VSpace, role, latency, zero bootstrap failure,
                             and zero budget-overrun proof.
  --require-input-responsive Require serial echo, USB burst, and HDMI proof
                             breadcrumbs with zero USB burst drops.
  --require-ready            Require current functional USB readiness plus the
                             WiFi ready gate and linked old-good replay contract.
  -h, --help                 Show this help

Default proof commands:
  netstats
  smp activity
  nettest
  netstats
  wifi diag
  usb diag
  usb status

`--require-wifi-ready` inserts `wifi dump-state` immediately before the compact
WiFi diagnostic so DPC and verbose acceptance evidence are command-bound.
USAGE
}

log() {
    echo "[pi4-gate] $*"
}

fail() {
    echo "[pi4-gate] error: $*" >&2
    exit 1
}

require_arg() {
    local option="$1"
    local argc="$2"
    [[ "${argc}" -ge 2 ]] || fail "${option} requires a value"
}

require_file() {
    local path="$1"
    [[ -f "${path}" ]] || fail "required file missing: ${path}"
}

require_nonnegative_integer() {
    local name="$1"
    local value="$2"
    [[ "${value}" =~ ^[0-9]+$ ]] || fail "${name} must be a non-negative integer: ${value}"
}

prepare_fresh_output_path() {
    local label="$1"
    local path="$2"
    local reserve="${3:-no}"

    "${PYTHON}" - "${label}" "${path}" "${reserve}" <<'PY'
import base64
import json
import os
import stat
import sys

label, path_value, reserve_value = sys.argv[1:]
path = os.path.abspath(path_value)
if (
    path == os.sep
    or path.endswith(os.sep)
    or any(ord(character) < 0x20 for character in path)
):
    raise SystemExit(f"{label} output path is invalid: {path}")
components = [component for component in path.split(os.sep) if component]
parent_components = components[:-1]
leaf = components[-1]
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
file_flags = (
    os.O_WRONLY
    | os.O_CREAT
    | os.O_EXCL
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
chain = []
try:
    root_metadata = os.fstat(directory_fd)
    chain.append([root_metadata.st_dev, root_metadata.st_ino])
    for component in parent_components:
        try:
            next_fd = os.open(component, directory_flags, dir_fd=directory_fd)
        except FileNotFoundError:
            os.mkdir(component, 0o755, dir_fd=directory_fd)
            next_fd = os.open(component, directory_flags, dir_fd=directory_fd)
        metadata = os.fstat(next_fd)
        if not stat.S_ISDIR(metadata.st_mode):
            os.close(next_fd)
            raise SystemExit(f"{label} parent is not a directory: {path}")
        chain.append([metadata.st_dev, metadata.st_ino])
        os.close(directory_fd)
        directory_fd = next_fd
    try:
        os.stat(leaf, dir_fd=directory_fd, follow_symlinks=False)
    except FileNotFoundError:
        pass
    else:
        raise SystemExit(f"{label} already exists: {path}")
    leaf_identity = None
    if reserve_value == "yes":
        leaf_fd = os.open(leaf, file_flags, 0o644, dir_fd=directory_fd)
        try:
            metadata = os.fstat(leaf_fd)
            leaf_identity = [metadata.st_dev, metadata.st_ino]
            os.fsync(leaf_fd)
        finally:
            os.close(leaf_fd)
        os.fsync(directory_fd)
    elif reserve_value != "no":
        raise SystemExit(f"{label} reserve mode is invalid")
except OSError as error:
    raise SystemExit(f"{label} output path is unsafe: {error}") from error
finally:
    os.close(directory_fd)
seal = {
    "schema": "cohesix-output-path-seal/v1",
    "path": path,
    "parent_components": parent_components,
    "chain": chain,
    "leaf": leaf,
    "leaf_identity": leaf_identity,
}
print(base64.urlsafe_b64encode(json.dumps(seal, separators=(",", ":")).encode()).decode())
PY
}

publish_file_exclusively() {
    local label="$1"
    local source="$2"
    local destination="$3"
    local seal="$4"
    local source_seal="$5"

    "${PYTHON}" - "${label}" "${source}" "${destination}" \
      "${seal}" "${source_seal}" <<'PY'
import base64
import json
import os
import secrets
import stat
import sys

label, source_value, destination_value, seal_value, source_seal_value = sys.argv[1:]
destination = os.path.abspath(destination_value)
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
    source_seal = json.loads(
        base64.urlsafe_b64decode(source_seal_value.encode()).decode()
    )
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"{label} output path seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-path-seal/v1"
    or seal.get("path") != destination
    or seal.get("leaf_identity") is not None
):
    raise SystemExit(f"{label} output path seal does not match destination")
if (
    source_seal.get("schema") != "cohesix-output-source-seal/v1"
    or source_seal.get("requested_path") != os.path.abspath(source_value)
    or not isinstance(source_seal.get("leaf_identity"), list)
):
    raise SystemExit(f"{label} temporary source seal is invalid")
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
source_directory_fd = -1
temporary_name = f".cohesix-publish-{secrets.token_hex(16)}"
temporary_fd = -1
published_identity = None
publication_committed = False


def verify_destination_binding(expected_leaf_identity):
    verification_fd = os.open(os.sep, directory_flags)
    try:
        for index, identity in enumerate(seal["chain"]):
            metadata = os.fstat(verification_fd)
            if [metadata.st_dev, metadata.st_ino] != identity:
                raise SystemExit(f"{label} output ancestor identity changed")
            if index < len(seal["parent_components"]):
                next_fd = os.open(
                    seal["parent_components"][index],
                    directory_flags,
                    dir_fd=verification_fd,
                )
                os.close(verification_fd)
                verification_fd = next_fd
        pinned = os.fstat(directory_fd)
        observed = os.fstat(verification_fd)
        if (pinned.st_dev, pinned.st_ino) != (observed.st_dev, observed.st_ino):
            raise SystemExit(f"{label} output directory is no longer path-bound")
        try:
            leaf_metadata = os.stat(
                seal["leaf"],
                dir_fd=verification_fd,
                follow_symlinks=False,
            )
        except FileNotFoundError:
            if expected_leaf_identity is not None:
                raise SystemExit(f"{label} published leaf is no longer path-bound")
        else:
            if expected_leaf_identity is None:
                raise SystemExit(f"{label} destination already exists: {destination}")
            if [leaf_metadata.st_dev, leaf_metadata.st_ino] != expected_leaf_identity:
                raise SystemExit(f"{label} published leaf identity changed")
    finally:
        os.close(verification_fd)


try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit(f"{label} output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    try:
        os.stat(seal["leaf"], dir_fd=directory_fd, follow_symlinks=False)
    except FileNotFoundError:
        pass
    else:
        raise SystemExit(f"{label} destination already exists: {destination}")
    source_directory_fd = os.open(os.sep, directory_flags)
    for index, identity in enumerate(source_seal["chain"]):
        metadata = os.fstat(source_directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit(f"{label} temporary source ancestor identity changed")
        if index < len(source_seal["parent_components"]):
            next_fd = os.open(
                source_seal["parent_components"][index],
                directory_flags,
                dir_fd=source_directory_fd,
            )
            os.close(source_directory_fd)
            source_directory_fd = next_fd
    source_fd = os.open(
        source_seal["leaf"],
        os.O_RDONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=source_directory_fd,
    )
    try:
        source_metadata = os.fstat(source_fd)
        if (
            not stat.S_ISREG(source_metadata.st_mode)
            or source_metadata.st_size <= 0
            or [source_metadata.st_dev, source_metadata.st_ino]
            != source_seal["leaf_identity"]
        ):
            raise SystemExit(f"{label} temporary source is not a nonempty regular file")
        temporary_fd = os.open(
            temporary_name,
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
            0o644,
            dir_fd=directory_fd,
        )
        while True:
            chunk = os.read(source_fd, 1024 * 1024)
            if not chunk:
                break
            view = memoryview(chunk)
            while view:
                written = os.write(temporary_fd, view)
                view = view[written:]
        final_source_metadata = os.fstat(source_fd)
        if (
            final_source_metadata.st_dev != source_metadata.st_dev
            or final_source_metadata.st_ino != source_metadata.st_ino
            or final_source_metadata.st_size != source_metadata.st_size
            or final_source_metadata.st_mtime_ns != source_metadata.st_mtime_ns
        ):
            raise SystemExit(f"{label} temporary source changed during publication")
        os.fsync(temporary_fd)
    finally:
        os.close(source_fd)
    os.close(temporary_fd)
    temporary_fd = -1
    verify_destination_binding(None)
    try:
        os.link(
            temporary_name,
            seal["leaf"],
            src_dir_fd=directory_fd,
            dst_dir_fd=directory_fd,
            follow_symlinks=False,
        )
    except OSError as error:
        raise SystemExit(f"cannot publish {label} exclusively: {error}") from error
    metadata = os.stat(seal["leaf"], dir_fd=directory_fd, follow_symlinks=False)
    published_identity = [metadata.st_dev, metadata.st_ino]
    seal["leaf_identity"] = published_identity
    verify_destination_binding(published_identity)
    os.fsync(directory_fd)
    publication_committed = True
    print(
        base64.urlsafe_b64encode(
            json.dumps(seal, separators=(",", ":")).encode()
        ).decode()
    )
except OSError as error:
    raise SystemExit(f"{label} output path is unsafe: {error}") from error
finally:
    if temporary_fd >= 0:
        os.close(temporary_fd)
    if published_identity is not None and not publication_committed:
        try:
            metadata = os.stat(
                seal["leaf"],
                dir_fd=directory_fd,
                follow_symlinks=False,
            )
            if [metadata.st_dev, metadata.st_ino] == published_identity:
                os.unlink(seal["leaf"], dir_fd=directory_fd)
                os.fsync(directory_fd)
        except FileNotFoundError:
            pass
    try:
        os.unlink(temporary_name, dir_fd=directory_fd)
    except FileNotFoundError:
        pass
    if source_directory_fd >= 0:
        os.close(source_directory_fd)
    os.close(directory_fd)
PY
}

snapshot_sealed_output() {
    local label="$1"
    local seal="$2"

    "${PYTHON}" - "${label}" "${seal}" <<'PY'
import base64
import json
import os
import stat
import sys
import tempfile

label, seal_value = sys.argv[1:]
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"{label} output path seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-path-seal/v1"
    or not isinstance(seal.get("leaf_identity"), list)
):
    raise SystemExit(f"{label} output path seal lacks a leaf identity")
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
source_fd = -1
temporary_fd = -1
temporary_path = ""
try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit(f"{label} output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    source_fd = os.open(
        seal["leaf"],
        os.O_RDONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=directory_fd,
    )
    metadata = os.fstat(source_fd)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or [metadata.st_dev, metadata.st_ino] != seal["leaf_identity"]
    ):
        raise SystemExit(f"{label} output leaf identity changed")
    temporary_fd, temporary_path = tempfile.mkstemp(prefix="cohesix-output-snapshot-")
    remaining = metadata.st_size
    chunks = []
    while remaining:
        chunk = os.read(source_fd, min(remaining, 1024 * 1024))
        if not chunk:
            raise SystemExit(f"{label} output shrank during snapshot")
        chunks.append(chunk)
        remaining -= len(chunk)
    raw = b"".join(chunks)
    os.lseek(source_fd, 0, os.SEEK_SET)
    repeated = bytearray()
    while len(repeated) < len(raw):
        chunk = os.read(source_fd, min(len(raw) - len(repeated), 1024 * 1024))
        if not chunk:
            raise SystemExit(f"{label} output prefix changed during snapshot")
        repeated.extend(chunk)
    final_metadata = os.fstat(source_fd)
    if (
        final_metadata.st_dev != metadata.st_dev
        or final_metadata.st_ino != metadata.st_ino
        or final_metadata.st_size < metadata.st_size
        or bytes(repeated) != raw
    ):
        raise SystemExit(f"{label} output changed during snapshot")
    verification_fd = os.open(os.sep, directory_flags)
    try:
        for index, identity in enumerate(seal["chain"]):
            observed = os.fstat(verification_fd)
            if [observed.st_dev, observed.st_ino] != identity:
                raise SystemExit(f"{label} output ancestor identity changed")
            if index < len(seal["parent_components"]):
                next_fd = os.open(
                    seal["parent_components"][index],
                    directory_flags,
                    dir_fd=verification_fd,
                )
                os.close(verification_fd)
                verification_fd = next_fd
        pinned = os.fstat(directory_fd)
        observed = os.fstat(verification_fd)
        if (pinned.st_dev, pinned.st_ino) != (observed.st_dev, observed.st_ino):
            raise SystemExit(f"{label} output directory is no longer path-bound")
        leaf_metadata = os.stat(
            seal["leaf"],
            dir_fd=verification_fd,
            follow_symlinks=False,
        )
        if [leaf_metadata.st_dev, leaf_metadata.st_ino] != seal["leaf_identity"]:
            raise SystemExit(f"{label} output leaf identity changed")
    finally:
        os.close(verification_fd)
    view = memoryview(raw)
    while view:
        written = os.write(temporary_fd, view)
        view = view[written:]
    os.fsync(temporary_fd)
    print(temporary_path)
except BaseException:
    if temporary_path:
        try:
            os.unlink(temporary_path)
        except FileNotFoundError:
            pass
    raise
finally:
    if source_fd >= 0:
        os.close(source_fd)
    if temporary_fd >= 0:
        os.close(temporary_fd)
    os.close(directory_fd)
PY
}

refresh_serial_snapshot() {
    if [[ -n "${SERIAL_SNAPSHOT_PATH}" ]]; then
        rm -f "${SERIAL_SNAPSHOT_PATH}"
    fi
    SERIAL_SNAPSHOT_PATH="$(snapshot_sealed_output \
      "serial capture log" "${LOG_OUTPUT_SEAL}")"
}

create_sealed_temp_file() {
    local prefix="$1"

    "${PYTHON}" - "${prefix}" <<'PY'
import base64
import json
import os
import stat
import sys
import tempfile

prefix = sys.argv[1]
descriptor, path_value = tempfile.mkstemp(prefix=prefix)
resolved = os.path.realpath(path_value)
components = [component for component in resolved.split(os.sep) if component]
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
chain = []
try:
    for component in components[:-1]:
        metadata = os.fstat(directory_fd)
        chain.append([metadata.st_dev, metadata.st_ino])
        next_fd = os.open(component, directory_flags, dir_fd=directory_fd)
        os.close(directory_fd)
        directory_fd = next_fd
    metadata = os.fstat(directory_fd)
    chain.append([metadata.st_dev, metadata.st_ino])
    leaf_fd = os.open(
        components[-1],
        os.O_RDONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=directory_fd,
    )
    try:
        created = os.fstat(descriptor)
        observed = os.fstat(leaf_fd)
        if (
            not stat.S_ISREG(created.st_mode)
            or created.st_dev != observed.st_dev
            or created.st_ino != observed.st_ino
        ):
            raise SystemExit("temporary output identity changed during creation")
        seal = {
            "schema": "cohesix-output-source-seal/v1",
            "requested_path": os.path.abspath(path_value),
            "path": resolved,
            "parent_components": components[:-1],
            "chain": chain,
            "leaf": components[-1],
            "leaf_identity": [created.st_dev, created.st_ino],
        }
    finally:
        os.close(leaf_fd)
finally:
    os.close(directory_fd)
    os.close(descriptor)
encoded = base64.urlsafe_b64encode(
    json.dumps(seal, separators=(",", ":")).encode()
).decode()
print(f"{path_value}\t{encoded}")
PY
}

write_sealed_temp_file() {
    local label="$1"
    local path="$2"
    local seal="$3"

    "${PYTHON}" - "${label}" "${path}" "${seal}" 3<&0 <<'PY'
import base64
import json
import os
import stat
import sys

label, path_value, seal_value = sys.argv[1:]
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"{label} temporary output seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-source-seal/v1"
    or seal.get("requested_path") != os.path.abspath(path_value)
    or not isinstance(seal.get("leaf_identity"), list)
):
    raise SystemExit(f"{label} temporary output seal does not match its path")
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
output_fd = -1
try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit(f"{label} temporary output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    output_fd = os.open(
        seal["leaf"],
        os.O_WRONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=directory_fd,
    )
    metadata = os.fstat(output_fd)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_size != 0
        or [metadata.st_dev, metadata.st_ino] != seal["leaf_identity"]
    ):
        raise SystemExit(f"{label} temporary output identity changed")
    while True:
        chunk = os.read(3, 1024 * 1024)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(output_fd, view)
            view = view[written:]
    os.fsync(output_fd)
except OSError as error:
    raise SystemExit(f"{label} temporary output path is unsafe: {error}") from error
finally:
    if output_fd >= 0:
        os.close(output_fd)
    os.close(directory_fd)
PY
}

remove_sealed_temp_file() {
    local label="$1"
    local path="$2"
    local seal="$3"

    "${PYTHON}" - "${label}" "${path}" "${seal}" <<'PY'
import base64
import json
import os
import sys

label, path_value, seal_value = sys.argv[1:]
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"{label} temporary output seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-source-seal/v1"
    or seal.get("requested_path") != os.path.abspath(path_value)
    or not isinstance(seal.get("leaf_identity"), list)
):
    raise SystemExit(f"{label} temporary output seal does not match its path")
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit(f"{label} temporary output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    try:
        metadata = os.stat(
            seal["leaf"],
            dir_fd=directory_fd,
            follow_symlinks=False,
        )
    except FileNotFoundError:
        pass
    else:
        if [metadata.st_dev, metadata.st_ino] != seal["leaf_identity"]:
            raise SystemExit(f"{label} temporary output identity changed")
        os.unlink(seal["leaf"], dir_fd=directory_fd)
        os.fsync(directory_fd)
finally:
    os.close(directory_fd)
PY
}

normalize_gateway_target_host() {
    local value="$1"

    "${PYTHON}" - "${value}" <<'PY'
import ipaddress
import sys

value = sys.argv[1]
try:
    address = ipaddress.ip_address(value)
except ValueError as error:
    raise SystemExit("gateway target host must be an IP address") from error
canonical = str(address)
if value != canonical:
    raise SystemExit(f"gateway target host is not canonical; use {canonical}")
print(canonical)
PY
}

normalize_gateway_status_url() {
    local value="$1"

    "${PYTHON}" - "${value}" <<'PY'
import sys
import urllib.parse

value = sys.argv[1]
if (
    not value
    or len(value) > 2048
    or any(ord(character) < 0x21 or ord(character) > 0x7e for character in value)
):
    raise SystemExit("gateway status URL is empty or contains unsafe characters")
parsed = urllib.parse.urlsplit(value)
if (
    parsed.scheme not in {"http", "https"}
    or not parsed.hostname
    or parsed.username is not None
    or parsed.password is not None
    or parsed.query
    or parsed.fragment
):
    raise SystemExit("gateway status URL must be an HTTP(S) URL without credentials, query, or fragment")
try:
    parsed.port
except ValueError as error:
    raise SystemExit("gateway status URL contains an invalid port") from error
path = parsed.path.rstrip("/")
if not path:
    path = "/v1/meta/status"
elif path != "/v1/meta/status":
    raise SystemExit("gateway status URL path must be /v1/meta/status")
print(urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, path, "", "")))
PY
}

read_gateway_status_projection() {
    local endpoint="$1"

    "${PYTHON}" - "${endpoint}" "${GATEWAY_REQUEST_AUTH_TOKEN}" \
      "${GATEWAY_TARGET_HOST}" "${GATEWAY_TARGET_PORT}" <<'PY'
import ipaddress
import json
import sys
import time
import urllib.error
import urllib.request

endpoint, token, expected_host, expected_port_raw = sys.argv[1:]
if len(token) > 4096 or any(ord(character) < 0x20 for character in token):
    raise SystemExit("gateway request auth token contains unsafe characters")


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def reject_nonfinite_constant(value):
    raise ValueError(f"non-finite JSON constant: {value}")


request = urllib.request.Request(endpoint, method="GET")
if token:
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("x-cohesix-auth", token)
try:
    with urllib.request.urlopen(request, timeout=5.0) as response:
        raw = response.read(1024 * 1024 + 1)
except (OSError, urllib.error.URLError) as error:
    raise SystemExit(f"gateway status query failed: {error}") from error
captured_unix_ns = time.time_ns()
if len(raw) > 1024 * 1024:
    raise SystemExit("gateway status response exceeds 1 MiB")
try:
    status = json.loads(
        raw,
        object_pairs_hook=reject_duplicate_keys,
        parse_constant=reject_nonfinite_constant,
    )
except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
    raise SystemExit(f"gateway status response is not strict JSON: {error}") from error
if not isinstance(status, dict):
    raise SystemExit("gateway status response must be a JSON object")
connected = status.get("connected")
connects = status.get("connects")
reconnects = status.get("reconnects")
last_change = status.get("last_change_unix_ms")
target_host = status.get("target_host")
target_port = status.get("target_port")
if connected is not True:
    raise SystemExit("gateway status is not connected")
for name, value in (
    ("connects", connects),
    ("reconnects", reconnects),
    ("last_change_unix_ms", last_change),
):
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise SystemExit(f"gateway status {name} is not a non-negative integer")
if connects != 1 or reconnects != 0:
    raise SystemExit("gateway status does not prove one connection and zero reconnects")
if last_change == 0:
    raise SystemExit("gateway status lacks a connection-change timestamp")
try:
    canonical_target_host = str(ipaddress.ip_address(target_host))
except (TypeError, ValueError) as error:
    raise SystemExit("gateway status target_host is not a canonical IP address") from error
if target_host != canonical_target_host or target_host != expected_host:
    raise SystemExit("gateway status target_host differs from the selected Pi")
try:
    expected_port = int(expected_port_raw)
except ValueError as error:
    raise SystemExit("configured gateway target port is invalid") from error
if (
    not isinstance(target_port, int)
    or isinstance(target_port, bool)
    or target_port != expected_port
    or target_port != 31337
):
    raise SystemExit("gateway status target_port is not the Cohesix console port")
print(
    f"true\t{connects}\t{reconnects}\t{last_change}\t{captured_unix_ns}"
    f"\t{target_host}\t{target_port}"
)
PY
}

capture_gateway_continuity_start() {
    local projection
    local last_error=""
    local deadline

    [[ -n "${GATEWAY_STATUS_ENDPOINT}" ]] || return 0
    deadline=$((SECONDS + GATEWAY_READY_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        if projection="$(read_gateway_status_projection \
          "${GATEWAY_STATUS_ENDPOINT}" 2>&1)"; then
            IFS=$'\t' read -r \
              GATEWAY_START_CONNECTED GATEWAY_START_CONNECTS \
              GATEWAY_START_RECONNECTS GATEWAY_START_LAST_CHANGE_UNIX_MS \
              GATEWAY_START_CAPTURED_UNIX_NS GATEWAY_START_TARGET_HOST \
              GATEWAY_START_TARGET_PORT <<<"${projection}"
            [[ "${GATEWAY_START_CONNECTED}" == "true" ]] \
              || fail "gateway continuity start status is malformed"
            return 0
        fi
        last_error="${projection//$'\n'/ }"
        if ((SECONDS >= deadline)); then
            break
        fi
        sleep 1
    done
    fail "gateway continuity did not become ready within ${GATEWAY_READY_TIMEOUT_SECONDS}s${last_error:+: ${last_error:0:512}}"
}

capture_gateway_continuity_end() {
    local projection

    [[ -n "${GATEWAY_STATUS_ENDPOINT}" ]] || return 0
    if ! projection="$(read_gateway_status_projection \
      "${GATEWAY_STATUS_ENDPOINT}")"; then
        fail "gateway continuity end status is unavailable or invalid"
    fi
    IFS=$'\t' read -r \
      GATEWAY_END_CONNECTED GATEWAY_END_CONNECTS \
      GATEWAY_END_RECONNECTS GATEWAY_END_LAST_CHANGE_UNIX_MS \
      GATEWAY_END_CAPTURED_UNIX_NS GATEWAY_END_TARGET_HOST \
      GATEWAY_END_TARGET_PORT <<<"${projection}"
    if [[ "${GATEWAY_END_CONNECTED}" != "true" ]] \
        || [[ "${GATEWAY_END_CONNECTS}" != "${GATEWAY_START_CONNECTS}" ]] \
        || [[ "${GATEWAY_END_RECONNECTS}" != "${GATEWAY_START_RECONNECTS}" ]] \
        || [[ "${GATEWAY_END_LAST_CHANGE_UNIX_MS}" \
          != "${GATEWAY_START_LAST_CHANGE_UNIX_MS}" ]] \
        || [[ "${GATEWAY_START_TARGET_HOST}" != "${GATEWAY_TARGET_HOST}" ]] \
        || [[ "${GATEWAY_END_TARGET_HOST}" != "${GATEWAY_START_TARGET_HOST}" ]] \
        || [[ "${GATEWAY_START_TARGET_PORT}" != "${GATEWAY_TARGET_PORT}" ]] \
        || [[ "${GATEWAY_END_TARGET_PORT}" != "${GATEWAY_START_TARGET_PORT}" ]]; then
        fail "gateway connection changed during the controlled capture"
    fi
}

validate_gateway_capture_timeline() {
    [[ -n "${GATEWAY_STATUS_ENDPOINT}" ]] || return 0
    for value in \
      "${NETWORK_CAPTURE_STARTED_UNIX_NS}" \
      "${GATEWAY_START_CAPTURED_UNIX_NS}" \
      "${GATEWAY_END_CAPTURED_UNIX_NS}" \
      "${NETWORK_CAPTURE_FINISHED_UNIX_NS}"; do
        [[ "${value}" =~ ^[0-9]+$ ]] \
          || fail "gateway continuity capture timestamp is malformed"
    done
    if ((GATEWAY_START_CAPTURED_UNIX_NS < NETWORK_CAPTURE_STARTED_UNIX_NS \
        || GATEWAY_END_CAPTURED_UNIX_NS < GATEWAY_START_CAPTURED_UNIX_NS \
        || GATEWAY_END_CAPTURED_UNIX_NS > NETWORK_CAPTURE_FINISHED_UNIX_NS \
        || GATEWAY_START_LAST_CHANGE_UNIX_MS \
          < NETWORK_CAPTURE_STARTED_UNIX_NS / 1000000 \
        || GATEWAY_START_LAST_CHANGE_UNIX_MS \
          > GATEWAY_START_CAPTURED_UNIX_NS / 1000000)); then
        fail "gateway continuity statuses are outside the controlled capture"
    fi
}

ensure_capture_log_is_fresh() {
    if [[ -n "${LOG_OUTPUT_SEAL}" ]]; then
        return 0
    fi
    if [[ -e "${LOG_PATH}" || -L "${LOG_PATH}" ]]; then
        fail "refusing to capture to existing log without truncating: ${LOG_PATH}; pass a fresh --log path, or use --normalize-only/--no-capture for existing logs"
    fi
    if ! LOG_OUTPUT_SEAL="$(prepare_fresh_output_path \
      "serial capture log" "${LOG_PATH}" yes)"; then
        fail "refusing unsafe or existing serial capture log: ${LOG_PATH}"
    fi
}

detect_flash_disk() {
    local label="$1"
    local python_bin="$2"

    "${python_bin}" - "${label}" <<'PY'
import plistlib
import subprocess
import sys

label = sys.argv[1]
plist = plistlib.loads(subprocess.check_output(["diskutil", "list", "-plist"]))
candidates: list[str] = []
for disk in plist.get("AllDisksAndPartitions", []):
    parent = disk.get("DeviceIdentifier")
    for partition in disk.get("Partitions", []):
        if partition.get("VolumeName") != label:
            continue
        info = plistlib.loads(
            subprocess.check_output(["diskutil", "info", "-plist", f"/dev/{parent}"])
        )
        removable = (
            info.get("RemovableMediaOrExternalDevice", False)
            or info.get("Removable", False)
            or info.get("Ejectable", False)
        )
        system_image = info.get("SystemImage", False) or info.get("OSInternalMedia", False)
        if not removable or system_image:
            continue
        candidates.append(f"/dev/{parent}")

unique = sorted(set(candidates))
if len(unique) != 1:
    print(
        f"expected exactly one external disk with volume label {label!r}, got {unique}",
        file=sys.stderr,
    )
    sys.exit(2)
print(unique[0])
PY
}

run_image_build() {
    local resolved_flash_disk="${FLASH_DISK}"
    local -a args=(
        "${IMAGE_BUILD_SCRIPT}"
        "--manifest"
        "${MANIFEST_PATH}"
        "--venv"
        "${VENV_DIR}"
    )

    require_file "${IMAGE_BUILD_SCRIPT}"
    if [[ "${SKIP_BUILD}" -eq 1 ]]; then
        args+=("--skip-build")
    fi
    if [[ -n "${resolved_flash_disk}" ]]; then
        if [[ "${resolved_flash_disk}" == "auto" ]]; then
            resolved_flash_disk="$(detect_flash_disk "${DISK_LABEL}" "${PYTHON}")"
            log "auto-selected flash disk ${resolved_flash_disk}"
        fi
        args+=("--flash-disk" "${resolved_flash_disk}" "--disk-label" "${DISK_LABEL}")
    fi

    log "running image stage${resolved_flash_disk:+ and flash}"
    "${args[@]}"
}

cleanup_capture() {
    if [[ -n "${CAPTURE_PID}" ]]; then
        kill "${CAPTURE_PID}" 2>/dev/null || true
        wait "${CAPTURE_PID}" 2>/dev/null || true
        CAPTURE_PID=""
    fi
    if [[ -n "${SERIAL_CAPTURE_ERROR_LOG}" ]]; then
        rm -f "${SERIAL_CAPTURE_ERROR_LOG}"
        SERIAL_CAPTURE_ERROR_LOG=""
    fi
    if [[ -n "${NETWORK_CAPTURE_PID}" ]]; then
        kill -INT "${NETWORK_CAPTURE_PID}" 2>/dev/null || true
        wait "${NETWORK_CAPTURE_PID}" 2>/dev/null || true
        NETWORK_CAPTURE_PID=""
    fi
    if [[ -n "${NETWORK_CAPTURE_TEMP_PATH}" ]]; then
        if [[ -n "${NETWORK_CAPTURE_TEMP_SEAL}" ]]; then
            remove_sealed_temp_file \
              "network capture" "${NETWORK_CAPTURE_TEMP_PATH}" \
              "${NETWORK_CAPTURE_TEMP_SEAL}" 2>/dev/null || true
        fi
        NETWORK_CAPTURE_TEMP_PATH=""
        NETWORK_CAPTURE_TEMP_SEAL=""
    fi
    if [[ -n "${NETWORK_CAPTURE_ERROR_LOG}" ]]; then
        rm -f "${NETWORK_CAPTURE_ERROR_LOG}"
        NETWORK_CAPTURE_ERROR_LOG=""
    fi
    if [[ -n "${SERIAL_SNAPSHOT_PATH}" ]]; then
        rm -f "${SERIAL_SNAPSHOT_PATH}"
        SERIAL_SNAPSHOT_PATH=""
    fi
    if [[ -n "${NETWORK_CAPTURE_SNAPSHOT_PATH}" ]]; then
        rm -f "${NETWORK_CAPTURE_SNAPSHOT_PATH}"
        NETWORK_CAPTURE_SNAPSHOT_PATH=""
    fi
    if [[ -n "${RUNTIME_PROOF_SNAPSHOT_PATH}" ]]; then
        rm -f "${RUNTIME_PROOF_SNAPSHOT_PATH}"
        RUNTIME_PROOF_SNAPSHOT_PATH=""
    fi
}

start_network_capture() {
    local tcpdump_bin

    [[ -n "${NETWORK_INTERFACE}" ]] || return 0
    [[ "${NETWORK_INTERFACE}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || \
      fail "--network-interface contains unsafe characters: ${NETWORK_INTERFACE}"
    [[ -n "${NETWORK_OUTPUT_SEAL}" ]] \
      || fail "network capture output path was not prepared"
    tcpdump_bin="$(command -v tcpdump || true)"
    [[ -n "${tcpdump_bin}" ]] || fail "tcpdump is required for controlled network capture"
    IFS=$'\t' read -r NETWORK_CAPTURE_TEMP_PATH NETWORK_CAPTURE_TEMP_SEAL \
      <<<"$(create_sealed_temp_file "cohesix-pi4-network-")"
    NETWORK_CAPTURE_ERROR_LOG="$(mktemp "${TMPDIR:-/tmp}/cohesix-pi4-tcpdump.XXXXXX")"
    NETWORK_CAPTURE_STARTED_AT_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    NETWORK_CAPTURE_STARTED_UNIX_NS="$("${PYTHON}" -c 'import time; print(time.time_ns())')"
    NETWORK_CAPTURE_ID="$("${PYTHON}" -c 'import secrets; print(secrets.token_hex(16))')"
    log "capturing ${NETWORK_INTERFACE} to ${NETWORK_CAPTURE_PATH} during serial proof"
    "${PYTHON}" - "${tcpdump_bin}" "${NETWORK_INTERFACE}" \
      "${NETWORK_CAPTURE_TEMP_PATH}" "${NETWORK_CAPTURE_TEMP_SEAL}" \
      > /dev/null 2>"${NETWORK_CAPTURE_ERROR_LOG}" <<'PY' &
import base64
import json
import os
import stat
import sys

tcpdump_bin, interface, path_value, seal_value = sys.argv[1:]
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit("network capture temporary output seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-source-seal/v1"
    or seal.get("requested_path") != os.path.abspath(path_value)
    or not isinstance(seal.get("leaf_identity"), list)
):
    raise SystemExit("network capture temporary output seal does not match its path")
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
output_fd = -1
try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit("network capture temporary output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    output_fd = os.open(
        seal["leaf"],
        os.O_WRONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=directory_fd,
    )
    metadata = os.fstat(output_fd)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_size != 0
        or [metadata.st_dev, metadata.st_ino] != seal["leaf_identity"]
    ):
        raise SystemExit("network capture temporary output identity changed")
finally:
    os.close(directory_fd)
os.dup2(output_fd, 1)
os.close(output_fd)
os.execv(
    tcpdump_bin,
    [tcpdump_bin, "-U", "-n", "-s", "0", "-i", interface, "-w", "-"],
)
PY
    NETWORK_CAPTURE_PID="$!"
    sleep 1
    if ! kill -0 "${NETWORK_CAPTURE_PID}" 2>/dev/null; then
        local detail
        detail="$(tail -n 1 "${NETWORK_CAPTURE_ERROR_LOG}" 2>/dev/null || true)"
        wait "${NETWORK_CAPTURE_PID}" 2>/dev/null || true
        NETWORK_CAPTURE_PID=""
        fail "controlled network capture did not start${detail:+: ${detail}}"
    fi
    NETWORK_CAPTURE_CONTROLLED=1
}

finish_network_capture() {
    [[ "${NETWORK_CAPTURE_CONTROLLED}" -eq 1 ]] || return 0
    if [[ -n "${NETWORK_CAPTURE_PID}" ]]; then
        kill -INT "${NETWORK_CAPTURE_PID}" 2>/dev/null || true
        wait "${NETWORK_CAPTURE_PID}" 2>/dev/null || true
        NETWORK_CAPTURE_PID=""
    fi
    NETWORK_CAPTURE_FINISHED_AT_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    NETWORK_CAPTURE_FINISHED_UNIX_NS="$("${PYTHON}" -c 'import time; print(time.time_ns())')"
    if [[ ! -s "${NETWORK_CAPTURE_TEMP_PATH}" ]]; then
        local detail
        detail="$(tail -n 1 "${NETWORK_CAPTURE_ERROR_LOG}" 2>/dev/null || true)"
        fail "controlled network capture is empty${detail:+: ${detail}}"
    fi
    if ! NETWORK_OUTPUT_SEAL="$(publish_file_exclusively \
      "network capture" "${NETWORK_CAPTURE_TEMP_PATH}" \
      "${NETWORK_CAPTURE_PATH}" "${NETWORK_OUTPUT_SEAL}" \
      "${NETWORK_CAPTURE_TEMP_SEAL}")"; then
        fail "could not publish the controlled network capture without replacement"
    fi
    NETWORK_CAPTURE_SNAPSHOT_PATH="$(snapshot_sealed_output \
      "network capture" "${NETWORK_OUTPUT_SEAL}")"
    remove_sealed_temp_file \
      "network capture" "${NETWORK_CAPTURE_TEMP_PATH}" \
      "${NETWORK_CAPTURE_TEMP_SEAL}" \
      || fail "could not remove the sealed network capture temporary"
    NETWORK_CAPTURE_TEMP_PATH=""
    NETWORK_CAPTURE_TEMP_SEAL=""
    rm -f "${NETWORK_CAPTURE_ERROR_LOG}"
    NETWORK_CAPTURE_ERROR_LOG=""
}

start_serial_capture() {
    SERIAL_CAPTURE_ERROR_LOG="$(mktemp "${TMPDIR:-/tmp}/cohesix-pi4-serial.XXXXXX")"
    "${PYTHON}" - "${SERIAL_DEVICE}" "${LOG_OUTPUT_SEAL}" \
      2>"${SERIAL_CAPTURE_ERROR_LOG}" <<'PY' &
import base64
import json
import os
import signal
import stat
import sys

device_path, seal_value = sys.argv[1:]
try:
    seal = json.loads(base64.urlsafe_b64decode(seal_value.encode()).decode())
except (ValueError, UnicodeDecodeError, json.JSONDecodeError) as error:
    raise SystemExit("serial capture output path seal is invalid") from error
if (
    seal.get("schema") != "cohesix-output-path-seal/v1"
    or not isinstance(seal.get("leaf_identity"), list)
):
    raise SystemExit("serial capture output path seal lacks a leaf identity")
device_fd = os.open(device_path, os.O_RDONLY | getattr(os, "O_NOCTTY", 0))
directory_flags = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
directory_fd = os.open(os.sep, directory_flags)
try:
    for index, identity in enumerate(seal["chain"]):
        metadata = os.fstat(directory_fd)
        if [metadata.st_dev, metadata.st_ino] != identity:
            raise SystemExit("serial capture output ancestor identity changed")
        if index < len(seal["parent_components"]):
            next_fd = os.open(
                seal["parent_components"][index],
                directory_flags,
                dir_fd=directory_fd,
            )
            os.close(directory_fd)
            directory_fd = next_fd
    output_fd = os.open(
        seal["leaf"],
        os.O_WRONLY
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0),
        dir_fd=directory_fd,
    )
    metadata = os.fstat(output_fd)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or [metadata.st_dev, metadata.st_ino] != seal["leaf_identity"]
        or metadata.st_size != 0
    ):
        raise SystemExit("serial capture output leaf identity changed")
except BaseException:
    os.close(directory_fd)
    os.close(device_fd)
    raise
os.close(directory_fd)


def stop_capture(_signum: int, _frame: object) -> None:
    raise SystemExit(0)


signal.signal(signal.SIGTERM, stop_capture)
signal.signal(signal.SIGINT, stop_capture)
try:
    while True:
        chunk = os.read(device_fd, 64 * 1024)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(output_fd, view)
            view = view[written:]
finally:
    os.fsync(output_fd)
    os.close(output_fd)
    os.close(device_fd)
PY
    CAPTURE_PID="$!"
    sleep 0.1
    if ! kill -0 "${CAPTURE_PID}" 2>/dev/null; then
        local detail
        detail="$(tail -n 1 "${SERIAL_CAPTURE_ERROR_LOG}" 2>/dev/null || true)"
        wait "${CAPTURE_PID}" 2>/dev/null || true
        CAPTURE_PID=""
        fail "serial capture did not start${detail:+: ${detail}}"
    fi
}

console_prompt_seen() {
    refresh_serial_snapshot
    "${PYTHON}" - "${SERIAL_SNAPSHOT_PATH}" <<'PY'
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
for line in data.replace(b"\r", b"\n").split(b"\n"):
    if line.startswith(b"cohesix>"):
        sys.exit(0)
sys.exit(1)
PY
}

console_prompt_count() {
    refresh_serial_snapshot
    "${PYTHON}" - "${SERIAL_SNAPSHOT_PATH}" <<'PY'
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
count = 0
for line in data.replace(b"\r", b"\n").split(b"\n"):
    if line.startswith(b"cohesix>"):
        count += 1
print(count)
PY
}

wait_for_console_ready() {
    local deadline
    local boot_options_advanced=0

    deadline=$((SECONDS + CONSOLE_READY_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        if console_prompt_seen; then
            return
        fi
        if [[ "${boot_options_advanced}" -eq 0 ]] \
            && grep -q '\[cohesix\] Cohesix boot menu' "${SERIAL_SNAPSHOT_PATH}" \
            && grep -q 'Select option \[1\]:' "${SERIAL_SNAPSHOT_PATH}"; then
            log "advancing Cohesix boot menu with its displayed default selection"
            printf '1\r' > "${SERIAL_DEVICE}"
            boot_options_advanced=1
        fi
        sleep 1
    done
    fail "Cohesix console prompt did not appear within ${CONSOLE_READY_TIMEOUT_SECONDS}s"
}

wait_for_prompt_after_command() {
    local previous_count="$1"
    local command="$2"
    local deadline
    local current_count

    deadline=$((SECONDS + COMMAND_PROMPT_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        current_count="$(console_prompt_count)"
        if ((current_count > previous_count)); then
            return
        fi
        sleep 1
    done
    fail "Cohesix prompt did not return within ${COMMAND_PROMPT_TIMEOUT_SECONDS}s after command: ${command}"
}

wifi_supervisor_terminal_status() {
    refresh_serial_snapshot
    rg -o 'CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=(ready|failed|permanent)' "${SERIAL_SNAPSHOT_PATH}" \
        | tail -n 1 \
        | sed -E 's/.*status=//'
}

wait_for_wifi_supervisor_terminal() {
    local deadline
    local status

    deadline=$((SECONDS + WIFI_SUPERVISOR_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        status="$(wifi_supervisor_terminal_status || true)"
        if [[ -n "${status}" ]]; then
            printf '%s' "${status}"
            return
        fi
        sleep 1
    done
    fail "CYW43 bootstrap supervisor did not publish an attempt=1 terminal within ${WIFI_SUPERVISOR_TIMEOUT_SECONDS}s"
}

wifi_dhcp_bound_seen() {
    refresh_serial_snapshot
    rg -q 'active=wifi .*addr_src=dhcp-lease .*dhcp=bound|active=wifi .*dhcp=bound .*addr_src=dhcp-lease' "${SERIAL_SNAPSHOT_PATH}"
}

wait_for_wifi_dhcp_bound() {
    local deadline
    local prompt_count_before

    deadline=$((SECONDS + WIFI_DHCP_TIMEOUT_SECONDS))
    while ((SECONDS <= deadline)); do
        if wifi_dhcp_bound_seen; then
            return 0
        fi
        prompt_count_before="$(console_prompt_count)"
        log "console command: netstats (guarded DHCP poll)"
        send_console_line "netstats"
        wait_for_prompt_after_command "${prompt_count_before}" "netstats (guarded DHCP poll)"
        sleep "${COMMAND_DELAY_SECONDS}"
    done
    return 1
}

send_console_line() {
    local line="$1"
    local index
    local char

    for ((index = 0; index < ${#line}; index++)); do
        char="${line:index:1}"
        printf '%s' "${char}" > "${SERIAL_DEVICE}"
        sleep "${COMMAND_CHAR_DELAY_SECONDS}"
    done
    printf '\r' > "${SERIAL_DEVICE}"
}

run_capture() {
    local -a commands=()
    local command
    local index
    local wifi_supervisor_status="not-required"
    local wifi_dhcp_bound=0

    for ((index = 0; index < ${#DEFAULT_COMMANDS[@]}; index++)); do
        if [[ "${REQUIRE_WIFI_READY}" -eq 1 && "${DEFAULT_COMMANDS[$index]}" == "wifi diag" ]]; then
            commands+=("wifi dump-state")
        fi
        commands+=("${DEFAULT_COMMANDS[$index]}")
    done
    for ((index = 0; index < ${#EXTRA_COMMANDS[@]}; index++)); do
        commands+=("${EXTRA_COMMANDS[$index]}")
    done
    [[ -e "${SERIAL_DEVICE}" ]] || fail "serial device missing: ${SERIAL_DEVICE}"
    ensure_capture_log_is_fresh
    stty -f "${SERIAL_DEVICE}" 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts raw

    trap cleanup_capture EXIT
    start_network_capture
    log "capturing ${SERIAL_DEVICE} to ${LOG_PATH}"
    start_serial_capture

    sleep "${BOOT_WAIT_SECONDS}"
    wait_for_console_ready
    if [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]; then
        wifi_supervisor_status="$(wait_for_wifi_supervisor_terminal)"
        log "wifi supervisor terminal: ${wifi_supervisor_status}"
    fi
    capture_gateway_continuity_start
    for command in "${commands[@]}"; do
        local prompt_count_before
        if [[ "${command}" == "nettest" && "${REQUIRE_WIFI_READY}" -eq 1 ]]; then
            if [[ "${wifi_supervisor_status}" != "ready" ]]; then
                log "skipping nettest: wifi supervisor status=${wifi_supervisor_status}"
                continue
            fi
            if [[ "${wifi_dhcp_bound}" -eq 0 ]]; then
                if wait_for_wifi_dhcp_bound; then
                    wifi_dhcp_bound=1
                else
                    log "skipping nettest: WiFi DHCP did not bind within ${WIFI_DHCP_TIMEOUT_SECONDS}s"
                    continue
                fi
            fi
        fi
        prompt_count_before="$(console_prompt_count)"
        log "console command: ${command}"
        send_console_line "${command}"
        wait_for_prompt_after_command "${prompt_count_before}" "${command}"
        sleep "${COMMAND_DELAY_SECONDS}"
    done
    sleep "${CAPTURE_SECONDS}"
    capture_gateway_continuity_end
    if [[ -n "${CAPTURE_PID}" ]]; then
        kill "${CAPTURE_PID}" 2>/dev/null || true
        wait "${CAPTURE_PID}" 2>/dev/null || true
        CAPTURE_PID=""
    fi
    if [[ -n "${SERIAL_CAPTURE_ERROR_LOG}" ]]; then
        rm -f "${SERIAL_CAPTURE_ERROR_LOG}"
        SERIAL_CAPTURE_ERROR_LOG=""
    fi
    finish_network_capture
    validate_gateway_capture_timeline
}

run_normalizer() {
    local -a args
    local index
    local output
    local status
    local require_usb_frontier=1

    require_file "${TRACE_NORMALIZER}"
    if [[ "${NORMALIZE_ONLY}" -eq 0 && "${NO_CAPTURE}" -eq 0 ]]; then
        refresh_serial_snapshot
        args=("${PYTHON}" "${TRACE_NORMALIZER}" "${SERIAL_SNAPSHOT_PATH}" "--gate-summary")
    else
        args=("${PYTHON}" "${TRACE_NORMALIZER}" "${LOG_PATH}" "--gate-summary")
        require_file "${LOG_PATH}"
    fi
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 && "${REQUIRE_USB_READY}" -eq 0 && "${REQUIRE_INPUT_RESPONSIVE}" -eq 0 ]]; then
        require_usb_frontier=0
    fi
    if [[ "${ALLOW_SUMMARY_ONLY}" -eq 0 ]]; then
        if [[ "${require_usb_frontier}" -eq 1 ]]; then
            args+=("--expect-min" "USB_GATE=3")
        fi
        if [[ "${REQUIRE_WIRED_READY}" -eq 1 && "${REQUIRE_WIFI_READY}" -eq 0 ]]; then
            args+=("--expect" "WIFI_BLOCKER=not-selected")
        else
            args+=("--expect-min" "WIFI_GATE=1")
        fi
        args+=("--expect" "SERIAL_CLEAN=yes")
        args+=("--expect" "BOOT_HALTED=no")
        args+=("--expect" "PANIC_SEEN=no")
        args+=("--expect" "PANIC_REASON=none")
        args+=("--expect" "TIMER_IRQ27_SEEN=no")
        args+=("--expect" "USB_BOOTLOADER_HANDOFF_SEEN=no")
        args+=("--expect" "USB_COLD_BOOT_SEEN=yes")
        args+=("--expect" "USB_STALE_UEFI_HINT_SEEN=no")
        args+=("--expect" "ROOT_CONSOLE_READY=yes")
        args+=("--expect" "ROOT_PROMPT_SEEN=yes")
        if [[ "${require_usb_frontier}" -eq 1 ]]; then
            args+=("--expect-not" "USB_BLOCKER=unknown")
            args+=("--expect-not" "USB_BLOCKER=no-controller-edge-yet")
            args+=("--expect-not" "USB_BLOCKER=policy-skip-before-run")
            args+=("--expect-not" "USB_BLOCKER=pcie-config-replay")
            args+=("--expect-not" "USB_BLOCKER=pcie-irq-quiesce-failed")
            args+=("--expect-not" "USB_BLOCKER=pcie-irq-quiesce-missing")
            args+=("--expect-not" "USB_BLOCKER=cmd-controller-not-running")
            args+=("--expect-not" "USB_BLOCKER=cmd-controller-halted")
            args+=("--expect-not" "USB_BLOCKER=cmd-submit-proof-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=cmd-pre-doorbell-proof-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=cmd-doorbell-proof-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=pcie-window-cmd-doorbell-proof-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=raw-phys-cmd-doorbell-proof-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=cmd-poll-pending")
            args+=("--expect-not" "USB_BLOCKER=cmd-doorbell-write-halt")
            args+=("--expect-not" "USB_BLOCKER=cmd-fetch-timeout")
            args+=("--expect-not" "USB_BLOCKER=cmd-event-ring-timeout")
            args+=("--expect-not" "USB_BLOCKER=command-event-rings")
            args+=("--expect-not" "USB_BLOCKER=command-event-ring-not-proven")
            args+=("--expect-not" "USB_BLOCKER=enable-slot-completion-pending")
            args+=("--expect-not" "USB_BLOCKER=command-ring-ready")
            args+=("--expect-not" "USB_BLOCKER=cmd-poll-only-timeout")
            args+=("--expect-not" "USB_BLOCKER=cmd-live-timeout-snapshot-missing")
            args+=("--expect-not" "USB_BLOCKER=cmd-timeout")
            args+=("--expect-not" "USB_BLOCKER=usbcmd-run-preserved-reset-bit")
            args+=("--expect-not" "USB_BLOCKER=usbcmd-run-posted-flush-halt")
            args+=("--expect-not" "USB_BLOCKER=pcie-window-no-op-timeout")
            args+=("--expect-not" "USB_BLOCKER=raw-phys-cmd-poll-only-timeout")
            args+=("--expect-not" "USB_BLOCKER=brcm-axi-setup-read")
            args+=("--expect-not" "USB_BLOCKER=enumeration-disabled-bootloader-owned")
            args+=("--expect-not" "USB_BLOCKER=reset-pre-usbcmd-source")
            args+=("--expect-not" "USB_BLOCKER=reset-pre-usbcmd-source-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=port-register-access-disabled")
            args+=("--expect-not" "USB_BLOCKER=root-port-read-begin")
            args+=("--expect-not" "USB_BLOCKER=root-port-read-timer-preempted")
            args+=("--expect-not" "USB_BLOCKER=root-port-sample-deferred")
            args+=("--expect-not" "USB_BLOCKER=root-port-connected")
            args+=("--expect-not" "USB_BLOCKER=no-connected-ports")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-connect-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-connect-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-completion-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-enable-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-retry")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-failed")
            args+=("--expect-not" "USB_BLOCKER=root-port-stale-cleanup-no-reply")
            args+=("--expect-not" "USB_BLOCKER=root-port-stale-cleanup-failed")
            args+=("--expect-not" "USB_BLOCKER=port-reset-timeout")
            args+=("--expect-not" "USB_BLOCKER=port-enable-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-reset-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-enable-timeout")
            args+=("--expect-not" "USB_BLOCKER=root-port-device-not-found")
            args+=("--expect-not" "USB_BLOCKER=address-enable-slot-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-context-publish-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-command-submit-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-command-completion-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-publish-no-reply")
            args+=("--expect-not" "USB_BLOCKER=address-device-timeout")
            args+=("--expect-not" "USB_BLOCKER=address-device-pending")
            args+=("--expect-not" "USB_BLOCKER=address-device-failed")
            args+=("--expect-not" "USB_BLOCKER=address-failed")
            args+=("--expect-not" "USB_BLOCKER=device-addressed")
            args+=("--expect-not" "USB_BLOCKER=device-descriptor-no-reply")
            args+=("--expect-not" "USB_BLOCKER=device-descriptor")
            args+=("--expect-not" "USB_BLOCKER=config-descriptor")
            args+=("--expect-not" "USB_BLOCKER=config-parse")
            args+=("--expect-not" "USB_BLOCKER=set-config")
            args+=("--expect-not" "USB_BLOCKER=invalid-config-value")
            args+=("--expect-not" "USB_BLOCKER=hid-init-failed")
            args+=("--expect-not" "USB_BLOCKER=hid-interrupt-in")
            args+=("--expect-not" "USB_BLOCKER=hid-queue-read-failed")
            args+=("--expect-not" "USB_BLOCKER=hid-first-report")
            args+=("--expect-not" "USB_BLOCKER=hid-first-byte")
            args+=("--expect-not" "USB_BLOCKER=keyboard-first-byte")
            args+=("--expect-not" "USB_BLOCKER=no-keyboard-found")
            args+=("--expect-not" "USB_BLOCKER=keyboard-not-ready")
            args+=("--expect-not" "USB_BLOCKER=pcie-xhci-device-coverage-missing")
            args+=("--expect-not" "USB_BLOCKER=pcie-owner-ring-unavailable")
            args+=("--expect-not" "USB_BLOCKER=pcie-vl805-config-contract-missing")
            args+=("--expect-not" "USB_BLOCKER=unavailable")
            args+=("--expect-not" "USB_BLOCKER=safe-port-event-required")
            args+=("--expect-not" "USB_BLOCKER=safe-port-state")
        fi
        args+=("--expect-not" "WIFI_BLOCKER=ht-recover-cmd5-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=unknown")
        args+=("--expect-not" "WIFI_BLOCKER=deferred")
        args+=("--expect-not" "WIFI_BLOCKER=boot-deferred-local-seat-usb")
        args+=("--expect-not" "WIFI_BLOCKER=boot-deferred-root-console")
        args+=("--expect-not" "WIFI_BLOCKER=boot-waiting-for-wifi")
        args+=("--expect-not" "WIFI_BLOCKER=ht-clock-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=devon-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=function2-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd53-data-wait")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd52-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=ht-backplane-cmd52-unreadable")
        args+=("--expect-not" "WIFI_BLOCKER=chipclkcsr-cmd52-pre-f2")
        args+=("--expect-not" "WIFI_BLOCKER=linux-probe-pmu-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=linux-probe-pmu-write-skip")
        args+=("--expect-not" "WIFI_BLOCKER=chipcommon-socram-remap-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-prereset-fgc-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-reset-assert-cmd52-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-reset-assert-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-core-control")
        args+=("--expect-not" "WIFI_BLOCKER=pre-f2-core-control")
        args+=("--expect-not" "WIFI_BLOCKER=armcr4-release-readback-unavailable")
        args+=("--expect-not" "WIFI_BLOCKER=socram-prereset-zero-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-prereset-fgc-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-assert-reset-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-clear-reset-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=socram-postreset-clock-cmd53-r5-rejected")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd52-write")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd52-read")
        args+=("--expect-not" "WIFI_BLOCKER=sdio-cmd53-r5-error")
        args+=("--expect-not" "WIFI_BLOCKER=sdhci-byte-mode-count")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-channel-f2")
        args+=("--expect-not" "WIFI_BLOCKER=firmware-ready-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=mailbox-ready-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=sdpcm-credit-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=ioctl-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-bdc-event")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-interrupt-programming-drift")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-interrupts-deferred")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-no-reply")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-partial-hint-visibility")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-rearm-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-reply-idle-loop")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-sideband-unreadable")
        args+=("--expect-not" "WIFI_BLOCKER=control-plane-startup-link-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=join-pending")
        args+=("--expect-not" "WIFI_BLOCKER=join-timeout")
        args+=("--expect-not" "WIFI_BLOCKER=wifi-association-failed")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-pending")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-failed")
        args+=("--expect-not" "WIFI_BLOCKER=dhcp-invalid-packet")
        args+=("--expect-not" "WIFI_BLOCKER=net-not-ready-ipc-buffer")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-policy-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-selftest-disabled")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-unsupported")
        args+=("--expect-not" "WIFI_BLOCKER=nettest-failed")
        args+=("--expect-not" "WIFI_BLOCKER=wifi-driver-task-runtime-unproved")
    fi
    if [[ "${REQUIRE_USB_READY}" -eq 1 ]]; then
        args+=("--expect-min" "USB_GATE=10" "--expect" "USB_BLOCKER=none")
        args+=("--expect" "USB_LOCAL_SEAT_STATE=ready")
        args+=("--expect" "USB_COMMAND_READY=yes")
        args+=("--expect" "USB_FIRST_REPORT_READY=yes")
        args+=("--expect" "USB_BUSY_AFTER_READY=no")
        args+=("--expect" "USB_GATE_SCOPE=startup")
        args+=("--expect" "USB_CURRENT_LIVENESS=pass")
        args+=("--expect" "USB_PHYSICAL_INPUT_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_OWNER_STATE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes")
        args+=("--expect" "USB_RUNTIME_QUEUE_VALID=yes")
        args+=("--expect" "USB_RUNTIME_QUEUED_REPORTS=1")
        args+=("--expect" "USB_FIRST_BYTE_READY=yes")
        args+=("--expect" "USB_POST_FIRST_BYTE_BLOCKER=none")
        args+=("--expect" "USB_STARTUP_BLOCKER_SEEN=no")
        args+=("--expect" "USB_ACTIVE_BLOCKER_SEEN=no")
        args+=("--expect" "USB_KEYBOARD_NO_REPLIES=0")
    fi
    if [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]; then
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_SEEN=yes")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_READY=yes")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS=ready")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER=none")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT=1")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES=0")
        args+=("--expect" "CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES=0")
        args+=("--expect-min" "WIFI_GATE=10" "--expect" "WIFI_BLOCKER=none")
        args+=("--expect" "NET_ACTIVE=wifi")
        args+=("--expect" "NET_ADDR_SRC=dhcp-lease")
        args+=("--expect" "NET_DHCP=bound")
        args+=("--expect" "NET_TCP_READY=yes")
        args+=("--expect" "NETTEST_PROOF=yes")
        args+=("--expect" "COHSH_TCP_AUTH_PROOF=yes")
        args+=("--expect-min" "TCP_ACCEPTS=1")
        args+=("--expect-min" "TCP_AUTH_SESSIONS=1")
        args+=("--expect-min" "TCP_RX_BYTES=1")
        args+=("--expect" "WIFI_DPC_PROOF=yes")
        args+=("--expect" "WIFI_DPC_REASON=none")
        args+=("--expect" "WIFI_DPC_OWNER_ACTIVE=yes")
        args+=("--expect" "WIFI_GATE7_COMPLETE=yes")
        args+=("--expect" "WIFI_GATE7_SEEN=7a>7b>7c>7d>7e")
        args+=("--expect" "WIFI_GATE7_LAST=7e")
        args+=("--expect" "WIFI_GATE7_MISSING=none")
        args+=("--expect" "WIFI_OLDGOOD_REPLAY=yes")
        args+=("--expect" "WIFI_OLDGOOD_MISSING=none")
        args+=("--expect" "WIFI_FIRMWARE_IDENTITY_PROOF=yes")
        args+=("--expect" "WIFI_FIRMWARE_IDENTITY_BLOCKER=none")
        args+=("--expect" "WIFI_CLM_READY_PROOF=yes")
        args+=("--expect" "WIFI_FIRMWARE_VERSION_PROOF=yes")
        args+=("--expect" "WIFI_CLM_VERSION_PROOF=yes")
        args+=("--expect" "SDIO_IRQ158_INBAND_PROOF=yes")
    fi
    if [[ "${REQUIRE_WIRED_READY}" -eq 1 ]]; then
        args+=("--expect" "NET_ACTIVE=wired")
    fi
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]]; then
        local require_sdio_proof=1
        if [[ "${REQUIRE_WIRED_READY}" -eq 1 && "${REQUIRE_WIFI_READY}" -eq 0 ]]; then
            require_sdio_proof=0
        fi
        args+=("--expect" "DRIVER_TASK_DEFAULT_REQUESTED=yes")
        args+=("--expect" "DRIVER_TASK_LIVE_HOT_PATHS=yes")
        args+=("--expect-min" "DRIVER_TASK_CONTRACTS=5")
        args+=("--expect-min" "DRIVER_TASK_DEDICATED=5")
        args+=("--expect" "DRIVER_TASK_COMPATIBILITY=0")
        args+=("--expect" "DRIVER_TASK_DEDICATED_READY=yes")
        args+=("--expect" "DRIVER_TASK_SERIAL_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_USB_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_DISPLAY_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_NET_DEDICATED=yes")
        if [[ "${require_sdio_proof}" -eq 1 ]]; then
            args+=("--expect" "DRIVER_TASK_SDIO_DEDICATED=yes")
        fi
        args+=("--expect" "DRIVER_TASK_PCIE_DEDICATED=yes")
        args+=("--expect" "DRIVER_TASK_SUBSTRATE_READY=yes")
        args+=("--expect" "DRIVER_TASK_FAILED_COUNT=0")
        args+=("--expect" "DRIVER_TASK_CAPSET_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_FAULT_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_REVOKE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_SCHED_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_AFFINITY_PROOF=yes")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_CONFIGURED=5")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_APPLIED=5")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_PROOF=yes")
        args+=("--expect-min" "DRIVER_TASK_AFFINITY_MANIFEST_MATCHES=5")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_MISSING=0")
        args+=("--expect" "DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES=0")
        args+=("--expect" "DRIVER_TASK_VSPACE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_POINTER_FREE_IPC_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_OWNER_STATE_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes")
        args+=("--expect" "DRIVER_TASK_BUDGET_OVERRUNS=0")
        args+=("--expect" "TIMER_BACKEND=arch-counter")
        args+=("--expect" "TIMER_CLOCK_HZ=54000000")
        args+=("--expect" "TIMER_EL0_COUNTER=vct")
        args+=("--expect" "DUMMY_TIMER_SEEN=no")
        args+=("--expect-min" "DRIVER_TASK_LATENCY_PROOFS=5")
        args+=("--expect" "DRIVER_TASK_RING_CALL_OUTSTANDING=0")
        args+=("--expect" "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT=0")
        args+=("--expect" "DRIVER_TASK_BOOTSTRAP_DEFERRED=0")
        if [[ "${require_sdio_proof}" -eq 1 ]]; then
            args+=("--expect-min" "DRIVER_TASK_DMA_PROOFS=6")
        else
            args+=("--expect-min" "DRIVER_TASK_DMA_PROOFS=5")
        fi
        args+=("--expect" "DRIVER_TASK_DMA_BLOCKER=none")
        args+=("--expect" "PI4_RUNTIME_DMA_PROOF=fresh-pi")
        args+=("--expect" "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified")
    fi
    if [[ "${REQUIRE_INPUT_RESPONSIVE}" -eq 1 ]]; then
        args+=("--expect" "SERIAL_RESPONSIVE_PROOF=yes")
        args+=("--expect" "USB_POST_FIRST_BYTE_BLOCKER=none")
        args+=("--expect" "USB_BURST_PROOF=yes")
        args+=("--expect" "USB_BURST_DROPS=0")
        args+=("--expect" "USB_CURRENT_LIVENESS=pass")
        args+=("--expect" "USB_PHYSICAL_INPUT_PROOF=yes")
        args+=("--expect" "HDMI_RESPONSIVE_PROOF=yes")
    fi
    for ((index = 0; index < ${#EXPECTATIONS[@]}; index++)); do
        args+=("--expect" "${EXPECTATIONS[$index]}")
    done
    for ((index = 0; index < ${#MIN_EXPECTATIONS[@]}; index++)); do
        args+=("--expect-min" "${MIN_EXPECTATIONS[$index]}")
    done
    for ((index = 0; index < ${#NOT_EXPECTATIONS[@]}; index++)); do
        args+=("--expect-not" "${NOT_EXPECTATIONS[$index]}")
    done

    set +e
    output="$("${args[@]}")"
    status=$?
    set -e
    printf '%s\n' "${output}"
    if [[ "${status}" -ne 0 ]]; then
        return "${status}"
    fi
    if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]]; then
        write_runtime_dma_proof "${output}"
    fi
    if [[ -n "${CYW43_COEXISTENCE_RECORD_PATH}" ]]; then
        write_cyw43_coexistence_record "${output}"
    fi
}

runtime_dma_proof_path() {
    if [[ -n "${RUNTIME_DMA_PROOF_PATH}" ]]; then
        printf '%s\n' "${RUNTIME_DMA_PROOF_PATH}"
        return 0
    fi
    printf '%s.runtime-dma-proof.env\n' "${LOG_PATH%.*}"
}

write_runtime_dma_proof() {
    local summary="$1"
    local proof_path
    local proof_temp_path
    local proof_temp_seal
    local serial_proof_source
    local build_proof="${ROOT_DIR}/out/pi4-sd/pi4-runtime-dma-proof.env"
    proof_path="$(runtime_dma_proof_path)"
    [[ -n "${RUNTIME_OUTPUT_SEAL}" ]] \
      || fail "runtime/DMA proof output path was not prepared"
    IFS=$'\t' read -r proof_temp_path proof_temp_seal \
      <<<"$(create_sealed_temp_file "cohesix-pi4-runtime-proof-")"
    if [[ "${NORMALIZE_ONLY}" -eq 0 && "${NO_CAPTURE}" -eq 0 ]]; then
        refresh_serial_snapshot
        serial_proof_source="${SERIAL_SNAPSHOT_PATH}"
    else
        serial_proof_source="${LOG_PATH}"
    fi
    if ! {
        printf 'PI4_RUNTIME_DMA_PROOF_ARTIFACT_VERSION=1\n'
        printf 'PI4_RUNTIME_DMA_SERIAL_LOG=%s\n' "${LOG_PATH}"
        printf 'PI4_RUNTIME_DMA_SERIAL_LOG_SHA256=%s\n' "$(shasum -a 256 "${serial_proof_source}" | awk '{print $1}')"
        printf 'PI4_RUNTIME_DMA_SERIAL_LOG_BYTES=%s\n' "$(stat -f '%z' "${serial_proof_source}")"
        printf 'PI4_RUNTIME_DMA_MANIFEST_SOURCE=%s\n' "${MANIFEST_PATH}"
        if [[ "${NETWORK_CAPTURE_CONTROLLED}" -eq 1 ]]; then
            printf 'PI4_RUNTIME_DMA_CAPTURE_PAIRING=controlled-concurrent\n'
            printf 'PI4_RUNTIME_DMA_CAPTURE_ID=%s\n' "${NETWORK_CAPTURE_ID}"
            printf 'PI4_RUNTIME_DMA_NETWORK_INTERFACE=%s\n' "${NETWORK_INTERFACE}"
            printf 'PI4_RUNTIME_DMA_NETWORK_CAPTURE=%s\n' "${NETWORK_CAPTURE_PATH}"
            printf 'PI4_RUNTIME_DMA_NETWORK_CAPTURE_SHA256=%s\n' "$(shasum -a 256 "${NETWORK_CAPTURE_SNAPSHOT_PATH}" | awk '{print $1}')"
            printf 'PI4_RUNTIME_DMA_NETWORK_CAPTURE_BYTES=%s\n' "$(stat -f '%z' "${NETWORK_CAPTURE_SNAPSHOT_PATH}")"
            printf 'PI4_RUNTIME_DMA_CAPTURE_STARTED_AT_UTC=%s\n' "${NETWORK_CAPTURE_STARTED_AT_UTC}"
            printf 'PI4_RUNTIME_DMA_CAPTURE_FINISHED_AT_UTC=%s\n' "${NETWORK_CAPTURE_FINISHED_AT_UTC}"
            printf 'PI4_RUNTIME_DMA_CAPTURE_STARTED_UNIX_NS=%s\n' "${NETWORK_CAPTURE_STARTED_UNIX_NS}"
            printf 'PI4_RUNTIME_DMA_CAPTURE_FINISHED_UNIX_NS=%s\n' "${NETWORK_CAPTURE_FINISHED_UNIX_NS}"
            if [[ -n "${GATEWAY_STATUS_ENDPOINT}" ]]; then
                printf 'PI4_RUNTIME_DMA_GATEWAY_CONTINUITY=connected-single-session\n'
                printf 'PI4_RUNTIME_DMA_GATEWAY_STATUS_ENDPOINT=%s\n' "${GATEWAY_STATUS_ENDPOINT}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_TARGET_HOST=%s\n' "${GATEWAY_TARGET_HOST}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_TARGET_PORT=%s\n' "${GATEWAY_TARGET_PORT}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_CAPTURED_UNIX_NS=%s\n' "${GATEWAY_START_CAPTURED_UNIX_NS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_CONNECTED=%s\n' "${GATEWAY_START_CONNECTED}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_CONNECTS=%s\n' "${GATEWAY_START_CONNECTS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_RECONNECTS=%s\n' "${GATEWAY_START_RECONNECTS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS=%s\n' "${GATEWAY_START_LAST_CHANGE_UNIX_MS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_TARGET_HOST=%s\n' "${GATEWAY_START_TARGET_HOST}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_START_TARGET_PORT=%s\n' "${GATEWAY_START_TARGET_PORT}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_CAPTURED_UNIX_NS=%s\n' "${GATEWAY_END_CAPTURED_UNIX_NS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_CONNECTED=%s\n' "${GATEWAY_END_CONNECTED}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_CONNECTS=%s\n' "${GATEWAY_END_CONNECTS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS=%s\n' "${GATEWAY_END_RECONNECTS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS=%s\n' "${GATEWAY_END_LAST_CHANGE_UNIX_MS}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_TARGET_HOST=%s\n' "${GATEWAY_END_TARGET_HOST}"
                printf 'PI4_RUNTIME_DMA_GATEWAY_END_TARGET_PORT=%s\n' "${GATEWAY_END_TARGET_PORT}"
            fi
        fi
        if [[ -n "${TEST_PLAN_STATE_DIR:-}" ]]; then
            printf 'PI4_RUNTIME_DMA_TEST_PLAN_STATE_DIR=%s\n' "${TEST_PLAN_STATE_DIR}"
        fi
        if [[ -f "${build_proof}" ]]; then
            printf 'PI4_RUNTIME_DMA_STAGE_BUILD_PROOF=%s\n' "${build_proof}"
            printf 'PI4_RUNTIME_DMA_STAGE_BUILD_PROOF_SHA256=%s\n' "$(shasum -a 256 "${build_proof}" | awk '{print $1}')"
        fi
        while IFS= read -r line; do
            case "${line}" in
                PI4_RUNTIME_DMA_*|DRIVER_TASK_DMA_*|DRIVER_TASK_COUNTER_*|DRIVER_TASK_RESOURCE_*|DRIVER_TASK_RING_CALL_*|DRIVER_TASK_BOOTSTRAP_DEFERRED=*|DRIVER_TASK_ACTIVE_NET=*|DRIVER_TASK_OWNER_STATE_PROOF=*|DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_*|DRIVER_TASK_POINTER_FREE_IPC_PROOF=*|DRIVER_TASK_VSPACE_PROOF=*|TIMER_BACKEND=*|TIMER_CLOCK_HZ=*|TIMER_EL0_COUNTER=*|DUMMY_TIMER_SEEN=*)
                    printf '%s\n' "${line}"
                    ;;
            esac
        done <<<"${summary}"
    } | write_sealed_temp_file \
      "runtime/DMA proof" "${proof_temp_path}" "${proof_temp_seal}"; then
        remove_sealed_temp_file \
          "runtime/DMA proof" "${proof_temp_path}" "${proof_temp_seal}" \
          2>/dev/null || true
        fail "could not write the sealed runtime/DMA proof temporary"
    fi
    if ! RUNTIME_OUTPUT_SEAL="$(publish_file_exclusively \
      "runtime/DMA proof" "${proof_temp_path}" "${proof_path}" \
      "${RUNTIME_OUTPUT_SEAL}" "${proof_temp_seal}")"; then
        remove_sealed_temp_file \
          "runtime/DMA proof" "${proof_temp_path}" "${proof_temp_seal}" \
          2>/dev/null || true
        fail "could not publish the runtime/DMA proof without replacement"
    fi
    RUNTIME_PROOF_SNAPSHOT_PATH="$(snapshot_sealed_output \
      "runtime/DMA proof" "${RUNTIME_OUTPUT_SEAL}")"
    remove_sealed_temp_file \
      "runtime/DMA proof" "${proof_temp_path}" "${proof_temp_seal}" \
      || fail "could not remove the sealed runtime/DMA proof temporary"
    log "runtime/DMA proof artifact: ${proof_path}"
}

write_cyw43_coexistence_record() {
    local summary="$1"
    local record_temp_path
    local record_temp_seal
    local summary_path
    [[ -n "${RUNTIME_PROOF_SNAPSHOT_PATH}" ]] \
      || fail "runtime/DMA proof snapshot is unavailable"
    [[ -n "${CYW43_OUTPUT_SEAL}" ]] \
      || fail "CYW43 coexistence output path was not prepared"
    summary_path="$(mktemp "${TMPDIR:-/tmp}/cohesix-pi4-wifi-summary.XXXXXX")"
    IFS=$'\t' read -r record_temp_path record_temp_seal \
      <<<"$(create_sealed_temp_file "cohesix-pi4-cyw43-")"
    printf '%s\n' "${summary}" >"${summary_path}"
    if ! "${PYTHON}" - \
      "${ROOT_DIR}" "${RUNTIME_PROOF_SNAPSHOT_PATH}" \
      "${NETWORK_CAPTURE_ID}" "${NETWORK_CAPTURE_STARTED_UNIX_NS}" \
      "${NETWORK_CAPTURE_FINISHED_UNIX_NS}" "${NETWORK_INTERFACE}" \
      "${summary_path}" <<'PY' \
      | write_sealed_temp_file "CYW43 coexistence record" \
        "${record_temp_path}" "${record_temp_seal}"; then
import hashlib
import json
import re
import struct
import sys
from pathlib import Path

(
    repo_value,
    runtime_value,
    capture_id,
    started_value,
    finished_value,
    interface,
    summary_value,
) = sys.argv[1:]
repo = Path(repo_value)
sys.path.insert(0, str(repo))
from scripts import rest_perf_harness as harness  # noqa: E402


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def exact_int(value: str, label: str, *, minimum: int = 0) -> int:
    if not re.fullmatch(r"0|[1-9][0-9]*", value):
        raise SystemExit(f"{label} is not an exact unsigned integer")
    parsed = int(value)
    if parsed < minimum:
        raise SystemExit(f"{label} is below its required minimum")
    return parsed


def require_summary(summary: dict[str, str], key: str, value: str) -> None:
    if summary.get(key) != value:
        raise SystemExit(f"normalized WiFi gate lacks {key}={value}")


def classic_pcap_metadata(raw: bytes) -> dict[str, object]:
    formats = {
        b"\xd4\xc3\xb2\xa1": ("<", 1_000_000, "pcap-us"),
        b"\xa1\xb2\xc3\xd4": (">", 1_000_000, "pcap-us"),
        b"\x4d\x3c\xb2\xa1": ("<", 1_000_000_000, "pcap-ns"),
        b"\xa1\xb2\x3c\x4d": (">", 1_000_000_000, "pcap-ns"),
    }
    selected = formats.get(raw[:4])
    if selected is None or len(raw) < 24:
        raise SystemExit("controlled tcpdump output is not classic pcap")
    endian, scale, format_name = selected
    major, minor, _zone, _sigfigs, snaplen, link_type = struct.unpack_from(
        f"{endian}HHiIII", raw, 4
    )
    if (major, minor) != (2, 4) or snaplen == 0:
        raise SystemExit("controlled pcap has an invalid global header")
    offset = 24
    first_ns = None
    last_ns = None
    packets = 0
    while offset < len(raw):
        if len(raw) - offset < 16:
            raise SystemExit("controlled pcap has a truncated packet header")
        seconds, fraction, captured, original = struct.unpack_from(
            f"{endian}IIII", raw, offset
        )
        offset += 16
        if (
            seconds == 0
            or fraction >= scale
            or captured == 0
            or captured > original
            or captured > snaplen
            or captured > len(raw) - offset
        ):
            raise SystemExit("controlled pcap has an invalid packet record")
        timestamp_ns = seconds * 1_000_000_000 + (
            fraction * (1_000 if scale == 1_000_000 else 1)
        )
        first_ns = timestamp_ns if first_ns is None else min(first_ns, timestamp_ns)
        last_ns = timestamp_ns if last_ns is None else max(last_ns, timestamp_ns)
        offset += captured
        packets += 1
    if packets == 0 or first_ns is None or last_ns is None:
        raise SystemExit("controlled pcap contains no packets")
    return {
        "format": format_name,
        "link_type": link_type,
        "first_packet_unix_ns": first_ns,
        "last_packet_unix_ns": last_ns,
    }


summary_raw = Path(summary_value).read_bytes()
summary = harness.parse_exact_env(summary_raw, "normalized Pi WiFi gate")
runtime_raw, _runtime_metadata = harness.read_frozen_artifact(
    runtime_value,
    "live Pi runtime proof",
    harness.BENCHMARK_EVIDENCE_MAX_BYTES,
)
runtime = harness.parse_exact_env(runtime_raw, "live Pi runtime proof")
stage_value = runtime.get("PI4_RUNTIME_DMA_STAGE_BUILD_PROOF", "")
stage_raw, _stage_metadata = harness.read_frozen_artifact(
    stage_value,
    "Pi stage build proof",
    harness.BENCHMARK_EVIDENCE_MAX_BYTES,
)
if sha256(stage_raw) != runtime.get("PI4_RUNTIME_DMA_STAGE_BUILD_PROOF_SHA256"):
    raise SystemExit("live Pi proof does not bind the exact stage proof")
stage = harness.parse_exact_env(stage_raw, "Pi stage build proof")

artifact_fields = {
    "manifest": "PI4_RUNTIME_DMA_MANIFEST",
    "topology": "PI4_RUNTIME_DMA_TOPOLOGY",
    "image": "PI4_RUNTIME_DMA_STAGED_IMAGE",
    "kernel": "PI4_IMAGE_IDENTITY_KERNEL_ELF",
    "root": "PI4_IMAGE_IDENTITY_ROOT_ELF",
    "driver_archive": "PI4_RUNTIME_DMA_RUNTIME_CPIO",
    "driver_manifest": "PI4_IMAGE_IDENTITY_DRIVER_MANIFEST",
    "worker_archive": "PI4_IMAGE_IDENTITY_WORKER_ARCHIVE",
    "worker_manifest": "PI4_IMAGE_IDENTITY_WORKER_MANIFEST",
    "source": "PI4_IMAGE_IDENTITY_SOURCE_INVENTORY",
    "worker_abi": "PI4_IMAGE_IDENTITY_WORKER_ABI",
    "metadata": "PI4_IMAGE_IDENTITY_METADATA",
}
artifacts: dict[str, tuple[str, bytes]] = {}
for name, field in artifact_fields.items():
    path_value, raw, _metadata = harness.read_stage_artifact(
        stage,
        field,
        f"Pi CYW43 {name}",
        harness.BENCHMARK_IMAGE_MAX_BYTES if name == "image" else harness.BENCHMARK_EVIDENCE_MAX_BYTES,
    )
    artifacts[name] = (path_value, raw)

metadata = harness.parse_strict_json_object(
    artifacts["metadata"][1],
    "Pi image identity metadata",
)
topology = harness.parse_strict_json_object(
    artifacts["topology"][1],
    "Pi generated topology",
)
serial_value = runtime.get("PI4_RUNTIME_DMA_SERIAL_LOG", "")
serial_raw, _serial_metadata = harness.read_frozen_artifact(
    serial_value,
    "Pi same-boot serial log",
    harness.BENCHMARK_EVIDENCE_MAX_BYTES,
)
capture_value = runtime.get("PI4_RUNTIME_DMA_NETWORK_CAPTURE", "")
capture_raw, _capture_metadata = harness.read_frozen_artifact(
    capture_value,
    "Pi controlled network capture",
    harness.BENCHMARK_EVIDENCE_MAX_BYTES,
)
harness.validate_pi_network_capture(capture_raw)
latest_boot_offset = harness.validate_serial_image_identity(serial_raw, metadata)
harness.validate_pi_network_log(serial_raw, harness.BENCHMARK_TRANSPORT_WIFI)
harness.validate_pi_correlated_network_capture(
    capture_raw,
    serial_raw,
    harness.BENCHMARK_TRANSPORT_WIFI,
)
derived_outcomes = harness.pi_cyw43_outcomes_from_normalized_gate(summary_raw)
pcap = classic_pcap_metadata(capture_raw)
started_ns = exact_int(started_value, "capture start", minimum=1)
finished_ns = exact_int(finished_value, "capture finish", minimum=started_ns)
slack_ns = 2_000_000_000
if (
    int(pcap["first_packet_unix_ns"]) < started_ns - slack_ns
    or int(pcap["last_packet_unix_ns"]) > finished_ns + slack_ns
):
    raise SystemExit("pcap packet timestamps fall outside the controlled capture window")
if (
    not re.fullmatch(r"[0-9a-f]{32}", capture_id)
    or runtime.get("PI4_RUNTIME_DMA_CAPTURE_PAIRING") != "controlled-concurrent"
    or runtime.get("PI4_RUNTIME_DMA_CAPTURE_ID") != capture_id
    or runtime.get("PI4_RUNTIME_DMA_NETWORK_INTERFACE") != interface
    or runtime.get("PI4_RUNTIME_DMA_NETWORK_CAPTURE") != capture_value
    or runtime.get("PI4_RUNTIME_DMA_NETWORK_CAPTURE_SHA256") != sha256(capture_raw)
    or runtime.get("PI4_RUNTIME_DMA_NETWORK_CAPTURE_BYTES") != str(len(capture_raw))
    or runtime.get("PI4_RUNTIME_DMA_CAPTURE_STARTED_UNIX_NS") != started_value
    or runtime.get("PI4_RUNTIME_DMA_CAPTURE_FINISHED_UNIX_NS") != finished_value
    or runtime.get("PI4_RUNTIME_DMA_SERIAL_LOG_SHA256") != sha256(serial_raw)
    or runtime.get("PI4_RUNTIME_DMA_SERIAL_LOG_BYTES") != str(len(serial_raw))
):
    raise SystemExit("live Pi proof does not bind the controlled serial/pcap pair")

required = {
    "DRIVER_TASK_ACTIVE_NET": "cyw43",
    "PI4_RUNTIME_DMA_PROOF": "fresh-pi",
    "PI4_RUNTIME_DMA_COUNTER_PROOF": "counter-qualified",
    "DRIVER_TASK_DMA_BLOCKER": "none",
    "DRIVER_TASK_RING_CALL_OUTSTANDING": "0",
    "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT": "0",
    "TIMER_BACKEND": "arch-counter",
    "TIMER_CLOCK_HZ": "54000000",
    "TIMER_EL0_COUNTER": "vct",
    "DUMMY_TIMER_SEEN": "no",
    "NET_ACTIVE": "wifi",
    "NET_DHCP": "bound",
    "NET_TCP_READY": "yes",
    "NETTEST_PROOF": "yes",
    "COHSH_TCP_AUTH_PROOF": "yes",
    "WIFI_GATE": "10",
    "WIFI_BLOCKER": "none",
    "WIFI_DPC_PROOF": "yes",
    "DRIVER_TASK_SDIO_DEDICATED": "yes",
    "DRIVER_TASK_NET_DEDICATED": "yes",
    "DRIVER_TASK_OWNER_STATE_PROOF": "yes",
    "CYW43_BOOTSTRAP_SUPERVISOR_READY": "yes",
    "WIFI_FIRMWARE_IDENTITY_PROOF": "yes",
    "WIFI_CLM_READY_PROOF": "yes",
    "WIFI_FIRMWARE_VERSION_PROOF": "yes",
    "WIFI_CLM_VERSION_PROOF": "yes",
    "WIFI_GATE7_COMPLETE": "yes",
    "SDIO_IRQ158_INBAND_PROOF": "yes",
}
for key, value in required.items():
    require_summary(summary, key, value)
for key in (
    "DRIVER_TASK_ACTIVE_NET",
    "PI4_RUNTIME_DMA_PROOF",
    "PI4_RUNTIME_DMA_COUNTER_PROOF",
    "DRIVER_TASK_DMA_BLOCKER",
    "DRIVER_TASK_RING_CALL_OUTSTANDING",
    "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT",
    "TIMER_BACKEND",
    "TIMER_CLOCK_HZ",
    "TIMER_EL0_COUNTER",
    "DUMMY_TIMER_SEEN",
):
    if runtime.get(key) != required[key]:
        raise SystemExit(f"live Pi proof differs from normalized WiFi gate: {key}")

session_projection = {
    "target": "pi4",
    "source_sha256": sha256(artifacts["source"][1]),
    "manifest_sha256": sha256(artifacts["manifest"][1]),
    "kernel_sha256": sha256(artifacts["kernel"][1]),
    "root_image_sha256": sha256(artifacts["root"][1]),
    "driver_archive_sha256": sha256(artifacts["driver_archive"][1]),
    "driver_manifest_sha256": sha256(artifacts["driver_manifest"][1]),
    "worker_archive_sha256": sha256(artifacts["worker_archive"][1]),
    "worker_image_manifest_sha256": sha256(artifacts["worker_manifest"][1]),
    "worker_abi_sha256": sha256(artifacts["worker_abi"][1]),
}
record = {
    "schema": "cohesix-cyw43-coexistence-binding/v2",
    "producer": "pi4_gate_proof/v1",
    "target": "pi4",
    "transport": "wifi",
    "capture_id": capture_id,
    "captured_unix_s": finished_ns // 1_000_000_000,
    "selected": True,
    "classification": "positive-exact-image-live-closure",
    "session_projection": session_projection,
    "topology_sha256": topology.get("topology_sha256"),
    "image_identity": {
        "image_sha256": sha256(artifacts["image"][1]),
        "image_id": metadata.get("image_id"),
        "git_commit": metadata.get("git_commit"),
        "build_timestamp": metadata.get("build_timestamp"),
        "build_marker": metadata.get("build_marker"),
        "build_marker_sha256": metadata.get("build_marker_sha256"),
    },
    "runtime": {
        "runtime_evidence_sha256": sha256(runtime_raw),
        "serial_sha256": sha256(serial_raw),
        "serial_bytes": len(serial_raw),
        "latest_boot_offset": latest_boot_offset,
        "normalized_gate_sha256": sha256(summary_raw),
    },
    "network_capture": {
        "sha256": sha256(capture_raw),
        "bytes": len(capture_raw),
        "format": pcap["format"],
        "link_type": pcap["link_type"],
        "interface": interface,
        "capture_started_unix_ns": started_ns,
        "capture_finished_unix_ns": finished_ns,
    },
    "outcomes": derived_outcomes,
}
if not isinstance(record["topology_sha256"], str) or not re.fullmatch(
    r"[0-9a-f]{64}", record["topology_sha256"]
):
    raise SystemExit("Pi topology lacks an exact topology SHA-256")
encoded = (json.dumps(record, indent=2, sort_keys=True) + "\n").encode("utf-8")
sys.stdout.buffer.write(encoded)
PY
        rm -f "${summary_path}"
        remove_sealed_temp_file \
          "CYW43 coexistence record" "${record_temp_path}" \
          "${record_temp_seal}" 2>/dev/null || true
        fail "failed to produce the exact-image CYW43 coexistence record"
    fi
    rm -f "${summary_path}"
    if ! CYW43_OUTPUT_SEAL="$(publish_file_exclusively \
      "CYW43 coexistence record" "${record_temp_path}" \
      "${CYW43_COEXISTENCE_RECORD_PATH}" "${CYW43_OUTPUT_SEAL}" \
      "${record_temp_seal}")"; then
        remove_sealed_temp_file \
          "CYW43 coexistence record" "${record_temp_path}" \
          "${record_temp_seal}" 2>/dev/null || true
        fail "could not publish the CYW43 coexistence record without replacement"
    fi
    remove_sealed_temp_file \
      "CYW43 coexistence record" "${record_temp_path}" \
      "${record_temp_seal}" \
      || fail "could not remove the sealed CYW43 coexistence temporary"
    log "positive exact-image CYW43 coexistence record: ${CYW43_COEXISTENCE_RECORD_PATH}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest)
            require_arg "$1" "$#"
            MANIFEST_PATH="$2"
            shift 2
            ;;
        --venv)
            require_arg "$1" "$#"
            VENV_DIR="$2"
            PYTHON="${VENV_DIR}/bin/python"
            shift 2
            ;;
        --flash-disk)
            require_arg "$1" "$#"
            FLASH_DISK="$2"
            shift 2
            ;;
        --disk-label)
            require_arg "$1" "$#"
            DISK_LABEL="$2"
            shift 2
            ;;
        --serial-device)
            require_arg "$1" "$#"
            SERIAL_DEVICE="$2"
            shift 2
            ;;
        --log)
            require_arg "$1" "$#"
            LOG_PATH="$2"
            shift 2
            ;;
        --runtime-dma-proof-out)
            require_arg "$1" "$#"
            RUNTIME_DMA_PROOF_PATH="$2"
            shift 2
            ;;
        --network-interface)
            require_arg "$1" "$#"
            NETWORK_INTERFACE="$2"
            shift 2
            ;;
        --network-capture-out)
            require_arg "$1" "$#"
            NETWORK_CAPTURE_PATH="$2"
            shift 2
            ;;
        --gateway-status-url)
            require_arg "$1" "$#"
            GATEWAY_STATUS_URL="$2"
            shift 2
            ;;
        --gateway-target-host)
            require_arg "$1" "$#"
            GATEWAY_TARGET_HOST="$2"
            shift 2
            ;;
        --cyw43-coexistence-record-out)
            require_arg "$1" "$#"
            CYW43_COEXISTENCE_RECORD_PATH="$2"
            shift 2
            ;;
        --boot-wait)
            require_arg "$1" "$#"
            BOOT_WAIT_SECONDS="$2"
            shift 2
            ;;
        --console-ready-timeout)
            require_arg "$1" "$#"
            CONSOLE_READY_TIMEOUT_SECONDS="$2"
            shift 2
            ;;
        --capture-seconds)
            require_arg "$1" "$#"
            CAPTURE_SECONDS="$2"
            shift 2
            ;;
        --command-delay)
            require_arg "$1" "$#"
            COMMAND_DELAY_SECONDS="$2"
            shift 2
            ;;
        --command-char-delay)
            require_arg "$1" "$#"
            COMMAND_CHAR_DELAY_SECONDS="$2"
            shift 2
            ;;
        --command-prompt-timeout)
            require_arg "$1" "$#"
            COMMAND_PROMPT_TIMEOUT_SECONDS="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --no-capture)
            NO_CAPTURE=1
            shift
            ;;
        --normalize-only)
            NORMALIZE_ONLY=1
            shift
            ;;
        --no-default-commands)
            DEFAULT_COMMANDS=()
            shift
            ;;
        --probe-usb-keyboard)
            EXTRA_COMMANDS+=("usb probe-kbd")
            shift
            ;;
        --command)
            require_arg "$1" "$#"
            EXTRA_COMMANDS+=("$2")
            shift 2
            ;;
        --expect)
            require_arg "$1" "$#"
            EXPECTATIONS+=("$2")
            shift 2
            ;;
        --expect-min)
            require_arg "$1" "$#"
            MIN_EXPECTATIONS+=("$2")
            shift 2
            ;;
        --expect-not)
            require_arg "$1" "$#"
            NOT_EXPECTATIONS+=("$2")
            shift 2
            ;;
        --allow-summary-only)
            ALLOW_SUMMARY_ONLY=1
            shift
            ;;
        --require-usb-ready)
            REQUIRE_USB_READY=1
            shift
            ;;
        --require-wifi-ready)
            REQUIRE_WIFI_READY=1
            shift
            ;;
        --require-wired-ready)
            REQUIRE_WIRED_READY=1
            shift
            ;;
        --require-driver-task-proof)
            REQUIRE_DRIVER_TASK_PROOF=1
            shift
            ;;
        --require-input-responsive)
            REQUIRE_INPUT_RESPONSIVE=1
            shift
            ;;
        --require-ready)
            REQUIRE_USB_READY=1
            REQUIRE_WIFI_READY=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown option: $1"
            ;;
    esac
done

require_nonnegative_integer "--boot-wait" "${BOOT_WAIT_SECONDS}"
require_nonnegative_integer "--console-ready-timeout" "${CONSOLE_READY_TIMEOUT_SECONDS}"
require_nonnegative_integer "--capture-seconds" "${CAPTURE_SECONDS}"
require_nonnegative_integer "--command-delay" "${COMMAND_DELAY_SECONDS}"
require_nonnegative_integer "--command-prompt-timeout" "${COMMAND_PROMPT_TIMEOUT_SECONDS}"
require_nonnegative_integer \
  "COHESIX_PI4_GATEWAY_READY_TIMEOUT_SECONDS" \
  "${GATEWAY_READY_TIMEOUT_SECONDS}"
require_file "${PYTHON}"

if { [[ -n "${NETWORK_INTERFACE}" ]] && [[ -z "${NETWORK_CAPTURE_PATH}" ]]; } \
    || { [[ -z "${NETWORK_INTERFACE}" ]] && [[ -n "${NETWORK_CAPTURE_PATH}" ]]; }; then
    echo "[pi4-gate] error: --network-interface and --network-capture-out must be supplied together" >&2
    exit 2
fi
if [[ -n "${NETWORK_CAPTURE_PATH}" ]] \
    && { [[ "${NORMALIZE_ONLY}" -eq 1 ]] || [[ "${NO_CAPTURE}" -eq 1 ]]; }; then
    echo "[pi4-gate] error: controlled network capture requires the active serial capture path" >&2
    exit 2
fi
if { [[ -n "${GATEWAY_STATUS_URL}" ]] && [[ -z "${GATEWAY_TARGET_HOST}" ]]; } \
    || { [[ -z "${GATEWAY_STATUS_URL}" ]] && [[ -n "${GATEWAY_TARGET_HOST}" ]]; }; then
    echo "[pi4-gate] error: --gateway-status-url and --gateway-target-host must be supplied together" >&2
    exit 2
fi
if [[ -n "${GATEWAY_STATUS_URL}" ]]; then
    if [[ -z "${NETWORK_CAPTURE_PATH}" ]] \
        || [[ "${REQUIRE_DRIVER_TASK_PROOF}" -ne 1 ]] \
        || [[ "${NORMALIZE_ONLY}" -eq 1 ]] \
        || [[ "${NO_CAPTURE}" -eq 1 ]]; then
        echo "[pi4-gate] error: --gateway-status-url requires active serial/network capture and --require-driver-task-proof" >&2
        exit 2
    fi
    if ! GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url \
      "${GATEWAY_STATUS_URL}")"; then
        echo "[pi4-gate] error: --gateway-status-url is invalid" >&2
        exit 2
    fi
    if ! GATEWAY_TARGET_HOST="$(normalize_gateway_target_host \
      "${GATEWAY_TARGET_HOST}")"; then
        echo "[pi4-gate] error: --gateway-target-host is invalid" >&2
        exit 2
    fi
fi
if [[ -n "${NETWORK_CAPTURE_PATH}" ]]; then
    if ! NETWORK_OUTPUT_SEAL="$(prepare_fresh_output_path \
      "network capture" "${NETWORK_CAPTURE_PATH}")"; then
        fail "refusing unsafe or existing network capture: ${NETWORK_CAPTURE_PATH}"
    fi
fi
if [[ -n "${CYW43_COEXISTENCE_RECORD_PATH}" ]]; then
    if [[ "${REQUIRE_WIFI_READY}" -ne 1 ]] \
        || [[ "${REQUIRE_DRIVER_TASK_PROOF}" -ne 1 ]] \
        || [[ -z "${NETWORK_CAPTURE_PATH}" ]]; then
        echo "[pi4-gate] error: positive CYW43 output requires WiFi ready, driver-task proof, and controlled network capture" >&2
        exit 2
    fi
    if ! CYW43_OUTPUT_SEAL="$(prepare_fresh_output_path \
      "CYW43 coexistence record" "${CYW43_COEXISTENCE_RECORD_PATH}")"; then
        fail "refusing unsafe or existing CYW43 coexistence record: ${CYW43_COEXISTENCE_RECORD_PATH}"
    fi
fi
if [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]]; then
    if ! RUNTIME_OUTPUT_SEAL="$(prepare_fresh_output_path \
      "runtime/DMA proof" "$(runtime_dma_proof_path)")"; then
        fail "refusing unsafe or existing runtime/DMA proof: $(runtime_dma_proof_path)"
    fi
fi

trap cleanup_capture EXIT

if [[ "${ALLOW_SUMMARY_ONLY}" -eq 1 ]] \
    && { [[ "${REQUIRE_USB_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_WIFI_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_WIRED_READY}" -eq 1 ]] \
        || [[ "${REQUIRE_DRIVER_TASK_PROOF}" -eq 1 ]] \
        || [[ "${REQUIRE_INPUT_RESPONSIVE}" -eq 1 ]]; }; then
    echo "[pi4-gate] error: --allow-summary-only cannot be combined with ready-gate requirements" >&2
    exit 2
fi

if [[ "${NORMALIZE_ONLY}" -eq 0 ]]; then
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        ensure_capture_log_is_fresh
    fi
    run_image_build
    if [[ "${NO_CAPTURE}" -eq 0 ]]; then
        run_capture
    fi
fi

run_normalizer
