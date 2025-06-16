# CLASSIFICATION: COMMUNITY
# Filename: test_boot_efi.sh v0.13
# Author: Lukas Bower
# Date Modified: 2025-08-25
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
if [ ! -f "$TMPDIR/OVMF_VARS.fd" ]; then
    if ! cp /usr/share/OVMF/OVMF_VARS.fd "$TMPDIR/" 2>/dev/null; then
        echo "OVMF firmware not found — install 'ovmf' package" >&2
    fi
fi
OVMF_CODE="/usr/share/qemu/OVMF.fd"
if [ ! -f "$OVMF_CODE" ]; then
    for p in /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
        if [ -f "$p" ]; then
            OVMF_CODE="$p"
            break
        fi
    done
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
if [ ! -f out/EFI/BOOT/BOOTX64.EFI ]; then
    ls -R /out > /tmp/out_manifest.txt 2>/dev/null || true  # non-blocking info
    fail "bootx64.efi missing in out/"
fi
if [ ! -d out ] || [ -z "$(ls -A out 2>/dev/null)" ]; then
    ls -R /out > /tmp/out_manifest.txt 2>/dev/null || true  # non-blocking info
    fail "FAT source directory 'out' missing or empty"
fi
objdump -h out/EFI/BOOT/BOOTX64.EFI > out/BOOTX64_sections.txt

LOGFILE="$TMPDIR/qemu_boot.log"
QEMU_ARGS=(-bios "$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$TMPDIR/OVMF_VARS.fd" \
    -drive format=raw,file=fat:rw:out/ -net none -M q35 -m 256M \
    -no-reboot -monitor none)

if ! qemu-system-x86_64 "${QEMU_ARGS[@]}" -nographic -serial file:"${LOGFILE}"; then
    ls -R /out > /tmp/out_manifest.txt 2>/dev/null || true  # non-blocking info
    fail "QEMU execution failed"
fi
tail -n 20 "${LOGFILE}" || echo "Boot log unavailable — check TMPDIR or QEMU exit code"

if ! grep -q "EFI loader" "${LOGFILE}" || ! grep -q "Kernel launched" "${LOGFILE}"; then
    ls -R /out > /tmp/out_manifest.txt 2>/dev/null || true  # non-blocking info
    fail "Boot log verification failed"
fi

