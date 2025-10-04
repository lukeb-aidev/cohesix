# CLASSIFICATION: COMMUNITY
# Filename: check-qemu-deps.sh v0.3
# Author: Lukas Bower
# Date Modified: 2029-11-24
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
TARGET_MANIFEST="$ROOT/target-sel4.json"
TRACE_ROOT="${COHESIX_TRACE_TMP:-$ROOT/log/trace}"
TRACE_ID="${COHESIX_TRACE_ID:-trace-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
EPIC_ID="E3-F9"
SCRIPT_NAME="check-qemu-deps"

mkdir -p "$TRACE_ROOT"
TRACE_FILE="$TRACE_ROOT/${SCRIPT_NAME}.jsonl"

missing=0

log_entries=()

log_trace() {
    local component="$1"
    local status="$2"
    local detail="$3"
    local timestamp
    timestamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    detail="${detail//\\/\\\\}"
    detail="${detail//\"/\\\"}"
    log_entries+=("{\"timestamp\":\"$timestamp\",\"trace_id\":\"$TRACE_ID\",\"epic\":\"$EPIC_ID\",\"script\":\"$SCRIPT_NAME\",\"component\":\"$component\",\"status\":\"$status\",\"detail\":\"$detail\"}")
}

report_missing() {
    local message="$1"
    local hint="$2"
    missing=1
    printf 'ERROR: %s\n' "$message" >&2
    if [ -n "$hint" ]; then
        printf '       %s\n' "$hint" >&2
    fi
}

report_ok() {
    local message="$1"
    printf '%s\n' "$message" >&2
}

# Check QEMU for aarch64
if command -v qemu-system-aarch64 >/dev/null 2>&1; then
    report_ok "✔ qemu-system-aarch64 located ($(command -v qemu-system-aarch64))"
    log_trace "qemu-system-aarch64" "pass" "qemu-system-aarch64 present"
else
    report_missing "qemu-system-aarch64 not found." "Install QEMU with aarch64 support (e.g., 'sudo apt install qemu-system-arm' or 'brew install qemu'). Align launch flags with $TARGET_MANIFEST."
    log_trace "qemu-system-aarch64" "fail" "binary missing; review $TARGET_MANIFEST for platform configuration"
fi

# Check cross toolchain components
required_toolchain=(
    aarch64-linux-gnu-gcc
    aarch64-linux-gnu-ld
    aarch64-linux-gnu-objcopy
    aarch64-linux-gnu-readelf
)
for tool in "${required_toolchain[@]}"; do
    if command -v "$tool" >/dev/null 2>&1; then
        report_ok "✔ $tool located ($(command -v "$tool"))"
        log_trace "$tool" "pass" "$tool present"
    else
        report_missing "$tool not found." "Install the aarch64-linux-gnu cross toolchain (e.g., 'sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu'). Ensure tool versions satisfy $TARGET_MANIFEST."
        log_trace "$tool" "fail" "$tool missing; ensure cross toolchain matches $TARGET_MANIFEST"
    fi
done

# Validate elfloader binary availability
elfloader_candidates=(
    "$ROOT/out/bin/elfloader"
    "$ROOT/out/bin/elfloader.efi"
    "$ROOT/third_party/seL4/artefacts/elfloader"
)
found_elfloader=""
for candidate in "${elfloader_candidates[@]}"; do
    if [ -f "$candidate" ]; then
        found_elfloader="$candidate"
        break
    fi
done

if [ -n "$found_elfloader" ]; then
    report_ok "✔ Cohesix elfloader located at $found_elfloader"
    log_trace "elfloader" "pass" "elfloader located at $found_elfloader"
else
    report_missing "Cohesix elfloader binary not located under out/ or third_party." "Run './cohesix_fetch_build.sh --stage elfloader' or consult the seL4 build pipeline described in $TARGET_MANIFEST."
    log_trace "elfloader" "fail" "elfloader missing from expected locations"
fi

# Validate presence of bootable CPIO archive
cpio_candidates=(
    "$ROOT/out/cohesix.cpio"
    "$ROOT/out/initrd.cpio"
    "$ROOT/out/initfs.cpio"
    "$ROOT/out/boot/cohesix.cpio"
)
found_cpio=""
for candidate in "${cpio_candidates[@]}"; do
    if [ -f "$candidate" ]; then
        found_cpio="$candidate"
        break
    fi
done

if [ -n "$found_cpio" ]; then
    report_ok "✔ seL4 boot CPIO archive located at $found_cpio"
    log_trace "cpio" "pass" "boot CPIO archive located at $found_cpio"
else
    report_missing "Bootable CPIO archive not found under out/." "Run the seL4 build to regenerate the archive; see $TARGET_MANIFEST for target inputs."
    log_trace "cpio" "fail" "bootable CPIO archive missing under out/"
fi

# Persist trace log entries atomically
{
    for entry in "${log_entries[@]}"; do
        printf '%s\n' "$entry"
    done
} >> "$TRACE_FILE"

if [ "$missing" -eq 1 ]; then
    printf 'Refer to %s for the authoritative seL4 target manifest and rebuild the missing artefacts.\n' "$TARGET_MANIFEST" >&2
    exit 1
fi

echo "All seL4 QEMU prerequisites satisfied. Review target manifest at $TARGET_MANIFEST for configuration details."
exit 0
