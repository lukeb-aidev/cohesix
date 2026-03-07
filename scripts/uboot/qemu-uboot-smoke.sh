#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run a deterministic QEMU U-Boot smoke harness for script/env networking validation.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
U_BOOT_BIN="${ROOT_DIR}/third_party/u-boot/u-boot.bin"
OUT_DIR="${ROOT_DIR}/out/uboot"
LOG_FILE=""
MACHINE="virt"
CPU="cortex-a57"
MEMORY_MB=2048
NET_MODE="none"
TIMEOUT_SEC=25
BOARD_IP="10.0.2.15"
SERVER_IP="10.0.2.2"

declare -a EXTRA_ARGS=()

usage() {
    cat <<'USAGE'
Usage: scripts/uboot/qemu-uboot-smoke.sh [options] [-- <extra-qemu-args>]

Run U-Boot under QEMU and validate prompt + deterministic env/network setup commands.

Options:
  --qemu <bin>          QEMU binary (default: qemu-system-aarch64)
  --u-boot-bin <file>   U-Boot firmware image (default: third_party/u-boot/u-boot.bin)
  --out-dir <dir>       Output directory for logs (default: out/uboot)
  --log-file <file>     Explicit log file path (default: <out-dir>/qemu-uboot-smoke.log)
  --machine <name>      QEMU machine (default: virt)
  --cpu <name>          QEMU CPU model (default: cortex-a57)
  --memory-mb <n>       Guest memory in MiB (default: 2048)
  --net <none|user>     Network mode (default: none)
  --timeout-sec <n>     Prompt/command timeout in seconds (default: 25)
  --board-ip <ip>       Static board IP used in smoke commands (default: 10.0.2.15)
  --server-ip <ip>      Static server IP used in smoke commands (default: 10.0.2.2)
  -h, --help            Show this help text

Examples:
  scripts/uboot/qemu-uboot-smoke.sh
  scripts/uboot/qemu-uboot-smoke.sh --net user
USAGE
}

fail() {
    echo "[uboot-smoke] error: $*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --qemu)
            [[ $# -ge 2 ]] || fail "--qemu requires a value"
            QEMU_BIN="$2"
            shift 2
            ;;
        --u-boot-bin)
            [[ $# -ge 2 ]] || fail "--u-boot-bin requires a value"
            U_BOOT_BIN="$2"
            shift 2
            ;;
        --out-dir)
            [[ $# -ge 2 ]] || fail "--out-dir requires a value"
            OUT_DIR="$2"
            shift 2
            ;;
        --log-file)
            [[ $# -ge 2 ]] || fail "--log-file requires a value"
            LOG_FILE="$2"
            shift 2
            ;;
        --machine)
            [[ $# -ge 2 ]] || fail "--machine requires a value"
            MACHINE="$2"
            shift 2
            ;;
        --cpu)
            [[ $# -ge 2 ]] || fail "--cpu requires a value"
            CPU="$2"
            shift 2
            ;;
        --memory-mb)
            [[ $# -ge 2 ]] || fail "--memory-mb requires a value"
            MEMORY_MB="$2"
            shift 2
            ;;
        --net)
            [[ $# -ge 2 ]] || fail "--net requires a value"
            NET_MODE="$2"
            shift 2
            ;;
        --timeout-sec)
            [[ $# -ge 2 ]] || fail "--timeout-sec requires a value"
            TIMEOUT_SEC="$2"
            shift 2
            ;;
        --board-ip)
            [[ $# -ge 2 ]] || fail "--board-ip requires a value"
            BOARD_IP="$2"
            shift 2
            ;;
        --server-ip)
            [[ $# -ge 2 ]] || fail "--server-ip requires a value"
            SERVER_IP="$2"
            shift 2
            ;;
        --)
            shift
            EXTRA_ARGS+=("$@")
            break
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

case "$NET_MODE" in
    none|user) ;;
    *) fail "--net must be one of: none, user" ;;
esac

command -v "$QEMU_BIN" >/dev/null 2>&1 || fail "qemu binary not found: ${QEMU_BIN}"
[[ -f "$U_BOOT_BIN" ]] || fail "u-boot binary missing: ${U_BOOT_BIN}"

if [[ "$U_BOOT_BIN" == "${ROOT_DIR}/third_party/u-boot/u-boot.bin" ]]; then
    uboot_config="${ROOT_DIR}/third_party/u-boot/.config"
    if [[ -f "$uboot_config" ]] && ! rg -q '^CONFIG_TARGET_QEMU_ARM_64=y$' "$uboot_config"; then
        fail "third_party/u-boot/.config is not a qemu_arm64 build; run 'make -C third_party/u-boot qemu_arm64_defconfig' before using this harness"
    fi
fi

mkdir -p "$OUT_DIR"

if [[ -z "$LOG_FILE" ]]; then
    LOG_FILE="${OUT_DIR}/qemu-uboot-smoke.log"
fi
: >"$LOG_FILE"

log() {
    echo "[uboot-smoke] $*" | tee -a "$LOG_FILE"
}

declare -a QEMU_CMD=(
    "$QEMU_BIN"
    -machine "$MACHINE"
    -cpu "$CPU"
    -m "$MEMORY_MB"
    -nographic
    -monitor none
    -serial stdio
    -bios "$U_BOOT_BIN"
)

if [[ "$NET_MODE" == "user" ]]; then
    QEMU_CMD+=(
        -netdev user,id=net0
        -device virtio-net-device,netdev=net0
    )
else
    QEMU_CMD+=(
        -nic none
    )
fi

if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    QEMU_CMD+=("${EXTRA_ARGS[@]}")
fi

log "running QEMU U-Boot smoke net=${NET_MODE} machine=${MACHINE} cpu=${CPU} mem=${MEMORY_MB}MiB"
log "log file: ${LOG_FILE}"

python3 - "$LOG_FILE" "$TIMEOUT_SEC" "$NET_MODE" "$BOARD_IP" "$SERVER_IP" "${QEMU_CMD[@]}" <<'PY'
import os
import select
import signal
import subprocess
import sys
import time


def fail(msg: str) -> None:
    print(f"[uboot-smoke] error: {msg}", file=sys.stderr)
    sys.exit(1)


log_path = sys.argv[1]
timeout_sec = int(sys.argv[2])
net_mode = sys.argv[3]
board_ip = sys.argv[4]
server_ip = sys.argv[5]
qemu_cmd = sys.argv[6:]

commands = [
    "version",
    "printenv bootcmd",
    "setenv autoload no",
    "printenv autoload",
]
if net_mode == "user":
    commands.extend(
        [
            f"setenv ipaddr {board_ip}",
            f"setenv serverip {server_ip}",
            "setenv coh_net_mode dhcp",
            "setenv coh_net_interface wired",
            "printenv ipaddr",
            "printenv serverip",
            "printenv coh_net_mode",
            "printenv coh_net_interface",
            "printenv ethact",
        ]
    )

try:
    proc = subprocess.Popen(
        qemu_cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        bufsize=0,
    )
except OSError as exc:
    fail(f"failed to launch QEMU: {exc}")

assert proc.stdout is not None
assert proc.stdin is not None

buffer = bytearray()

with open(log_path, "ab", buffering=0) as log_file:

    def pump(deadline: float, needle: bytes) -> bool:
        while time.time() < deadline:
            if proc.poll() is not None:
                return needle in buffer
            ready, _, _ = select.select([proc.stdout], [], [], 0.2)
            if not ready:
                continue
            chunk = os.read(proc.stdout.fileno(), 4096)
            if not chunk:
                return needle in buffer
            buffer.extend(chunk)
            log_file.write(chunk)
            if needle in buffer:
                return True
        return needle in buffer

    overall_deadline = time.time() + timeout_sec
    if not pump(overall_deadline, b"=>"):
        proc.terminate()
        fail("U-Boot prompt not observed before timeout")

    for command in commands:
        proc.stdin.write((command + "\n").encode("utf-8"))
        proc.stdin.flush()
        if not pump(overall_deadline, b"=>"):
            proc.terminate()
            fail(f"prompt did not return after command: {command}")

    # Graceful stop for a deterministic one-shot smoke run.
    proc.terminate()
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=3)

print("[uboot-smoke] ok: prompt and scripted env/network setup commands completed")
PY
