# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.17
# Author: Lukas Bower
# Date Modified: 2026-10-16
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/test_boot_efi_${TIMESTAMP}.log"
SUMMARY_FILE="$LOG_DIR/test_summary.txt"
START_TIME="$(date +%s)"
FATAL_ERROR=""

exec > >(tee "$LOG_FILE") 2>&1

fail() { FATAL_ERROR="$1"; echo "ERROR: $1" >&2; exit 1; }

write_summary() {
    local verdict=$1
    local end_time
    end_time="$(date +%s)"
    local duration=$(( end_time - START_TIME ))
    cat <<EOF > "$SUMMARY_FILE"
Timestamp: $(date '+%Y-%m-%d %H:%M:%S')
Verdict: $verdict
Duration: ${duration}s
Fatal Error: ${FATAL_ERROR:-none}
EOF
}

trap 'code=$?; verdict=PASS; [ $code -ne 0 ] && verdict=FAIL; write_summary "$verdict"' EXIT

# Ensure writable directories for QEMU and temporary files
if ! command -v qemu-system-x86_64 >/dev/null; then
    echo "⚠️ QEMU not installed; skipping UEFI boot test." >&2
    exit 0
fi

if [ -z "${TMPDIR:-}" ]; then
    TMPDIR="$(mktemp -d)"
fi
export TMPDIR
mkdir -p "$HOME/cohesix/out"
touch "$HOME/cohesix/out/boot-ready.txt"

TOOLCHAIN="${CC:-}"
if [[ -z "$TOOLCHAIN" ]]; then
    if command -v clang >/dev/null; then
        TOOLCHAIN=clang
    else
        TOOLCHAIN=gcc
    fi
fi
echo "Using $TOOLCHAIN toolchain for UEFI build..."
if [[ ! -f /usr/include/efi/efi.h ]]; then
    fail "gnu-efi headers not found"
fi
if [[ ! -f /usr/include/efi/x86_64/efibind.h && ! -f /usr/include/efi/$(uname -m)/efibind.h ]]; then
    echo "WARNING: architecture headers missing; build may fail" >&2
fi

"$TOOLCHAIN" --version | head -n 1
"$TOOLCHAIN" -E -x c - -v </dev/null 2>&1 | sed -n '/search starts here:/,/End of search list/p'

make print-env CC="$TOOLCHAIN"
make -n bootloader kernel CC="$TOOLCHAIN" > out/make_debug.log
if ! make bootloader kernel CC="$TOOLCHAIN"; then
    fail "Build failed"
fi
tools/make_iso.sh
if [ ! -f out/cohesix.iso ]; then
    ls -R out > /tmp/out_manifest.txt 2>/dev/null || true  # non-blocking info
    fail "cohesix.iso missing in out/"
fi
if [ ! -f out_iso/EFI/BOOT/bootx64.efi ]; then
    fail "bootx64.efi missing in out_iso/"
fi

SERIAL_LOG="$TMPDIR/qemu_boot.log"
QEMU_LOG="$LOG_DIR/qemu_boot.log"
if [ -f "$QEMU_LOG" ]; then
    mv "$QEMU_LOG" "$QEMU_LOG.$TIMESTAMP"
fi
QEMU_ARGS=(-cdrom out/cohesix.iso -net none -M q35 -m 256M \
    -no-reboot -monitor none)

if ! qemu-system-x86_64 "${QEMU_ARGS[@]}" -nographic -serial file:"${SERIAL_LOG}"; then
    cat "$SERIAL_LOG" >> "$QEMU_LOG" 2>/dev/null || true
    tail -n 20 "$SERIAL_LOG" || true
    fail "QEMU execution failed"
fi
cat "$SERIAL_LOG" >> "$QEMU_LOG" 2>/dev/null || true
tail -n 20 "$SERIAL_LOG" || echo "Boot log unavailable — check TMPDIR or QEMU exit code"

if ! grep -q "EFI loader" "$SERIAL_LOG" || ! grep -q "Kernel launched" "$SERIAL_LOG"; then
    tail -n 20 "$SERIAL_LOG" || true
    fail "Boot log verification failed"
fi

