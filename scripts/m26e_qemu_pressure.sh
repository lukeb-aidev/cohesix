#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Clean-build and collect same-boot Milestone 26e QEMU Worker/MCS pressure evidence.
# Copyright 2026 Lukas Bower

set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"

usage() {
    cat <<'USAGE'
Usage: scripts/m26e_qemu_pressure.sh [options]

Clean-build the canonical four-core QEMU/GICv3 profile, then run separate
same-artifact medium and high executable-Worker pressure boots. This command
deletes only the validated repository target/ and out/ contents after moving
the explicit seL4/toolchain inputs to a temporary directory.

Options:
  --run-dir DIR          Fresh evidence directory under out/
                         (default: out/m26e-qemu-pressure)
  --sel4-source DIR      Clean upstream seL4 source input
                         (default: out/sel4/source-v16-clean)
  --sel4-build DIR       Fresh qemu_smp_production build directory
                         (default: out/sel4/profile-v2/qemu-smp-production)
  --profile-python FILE  Profile virtualenv Python
                         (default: out/toolchain/sel4-profile-venv/bin/python)
  --compiler-dir DIR     Pinned AArch64 compiler installation
  --compiler-archive FILE
                         Pinned AArch64 compiler archive
  --qemu FILE            qemu-system-aarch64 executable
                         (default: /opt/homebrew/bin/qemu-system-aarch64)
  --gdb FILE             aarch64-none-elf-gdb executable
  --jobs N               seL4 build jobs, 1..32 (default: 10)
  --check-only           Validate immutable inputs and print the plan; this
                         never cleans, builds, boots, or emits acceptance
  -h, --help             Show this help

Required environment (values are never printed):
  COH_AUTH_TOKEN
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN

The command is QEMU-only. It never executes Raspberry Pi hardware tests and
never classifies the bootstrap-trace GPU/LoRA snapshot as provider-live.
USAGE
}

die() {
    printf 'm26e-qemu-pressure: %s\n' "$*" >&2
    exit 2
}

log() {
    printf '[m26e-qemu-pressure] %s\n' "$*"
}

canonical_existing_dir() {
    python3 - "$REPO_ROOT" "$1" "$2" <<'PY'
import os
from pathlib import Path
import sys

repo, raw, base_raw = sys.argv[1:]
if not raw or any(ord(character) < 0x20 for character in raw):
    raise SystemExit("directory path contains an empty/control component")
candidate = Path(raw) if Path(raw).is_absolute() else Path(repo) / raw
if ".." in candidate.parts:
    raise SystemExit(f"directory path may not contain '..': {raw}")
lexical = Path(os.path.abspath(candidate))
resolved = lexical.resolve(strict=True)
if lexical != resolved or not resolved.is_dir():
    raise SystemExit(f"directory must be a canonical non-symlink path: {raw}")
if base_raw:
    base = Path(base_raw).resolve(strict=True)
    if resolved == base or not resolved.is_relative_to(base):
        raise SystemExit(f"directory is outside its required root {base}: {resolved}")
print(resolved)
PY
}

canonical_existing_file() {
    python3 - "$REPO_ROOT" "$1" "$2" "$3" <<'PY'
import os
from pathlib import Path
import stat
import sys

repo, raw, base_raw, executable_raw = sys.argv[1:]
if not raw or any(ord(character) < 0x20 for character in raw):
    raise SystemExit("file path contains an empty/control component")
candidate = Path(raw) if Path(raw).is_absolute() else Path(repo) / raw
if ".." in candidate.parts:
    raise SystemExit(f"file path may not contain '..': {raw}")
lexical = Path(os.path.abspath(candidate))
resolved = lexical.resolve(strict=True)
info = resolved.stat()
if not stat.S_ISREG(info.st_mode):
    raise SystemExit(f"file must resolve to a regular path: {raw}")
if base_raw and lexical != resolved:
    raise SystemExit(f"repository input file must be canonical and non-symlinked: {raw}")
if base_raw:
    base = Path(base_raw).resolve(strict=True)
    if not resolved.is_relative_to(base):
        raise SystemExit(f"file is outside its required root {base}: {resolved}")
if executable_raw == "yes" and not os.access(resolved, os.X_OK):
    raise SystemExit(f"file is not executable: {resolved}")
print(resolved)
PY
}

canonical_profile_python() {
    python3 - "$REPO_ROOT" "$1" "$REPO_ROOT/out/toolchain" <<'PY'
import os
from pathlib import Path
import sys

repo, raw, toolchain_raw = sys.argv[1:]
if not raw or any(ord(character) < 0x20 for character in raw):
    raise SystemExit("profile Python path contains an empty/control component")
candidate = Path(raw) if Path(raw).is_absolute() else Path(repo) / raw
if ".." in candidate.parts:
    raise SystemExit(f"profile Python path may not contain '..': {raw}")
lexical = Path(os.path.abspath(candidate))
venv = lexical.parent.parent
toolchain = Path(toolchain_raw).resolve(strict=True)
if (
    lexical.parent.name != "bin"
    or not lexical.exists()
    or not lexical.is_file()
    or not os.access(lexical, os.X_OK)
    or venv.resolve(strict=True) != venv
    or venv == toolchain
    or not venv.is_relative_to(toolchain)
):
    raise SystemExit(f"profile Python is not inside a canonical repository virtualenv: {raw}")
lexical.resolve(strict=True)
print(lexical)
PY
}

canonical_future_dir() {
    python3 - "$REPO_ROOT" "$1" "$2" <<'PY'
import os
from pathlib import Path
import sys

repo, raw, base_raw = sys.argv[1:]
if not raw or any(ord(character) < 0x20 for character in raw):
    raise SystemExit("future directory path contains an empty/control component")
candidate = Path(raw) if Path(raw).is_absolute() else Path(repo) / raw
if ".." in candidate.parts:
    raise SystemExit(f"future directory path may not contain '..': {raw}")
lexical = Path(os.path.abspath(candidate))
base = Path(base_raw).resolve(strict=True)
resolved = lexical.resolve(strict=False)
if lexical != resolved or lexical == base or not lexical.is_relative_to(base):
    raise SystemExit(f"future directory is aliased or outside {base}: {raw}")
cursor = lexical.parent
while cursor != base:
    if cursor.exists() and cursor.resolve(strict=True) != cursor:
        raise SystemExit(f"future directory traverses an aliased parent: {cursor}")
    if cursor == cursor.parent:
        raise SystemExit(f"future directory parent chain escaped {base}")
    cursor = cursor.parent
print(lexical)
PY
}

RUN_DIR="out/m26e-qemu-pressure"
SEL4_SOURCE="out/sel4/source-v16-clean"
SEL4_BUILD="out/sel4/profile-v2/qemu-smp-production"
PROFILE_PYTHON="out/toolchain/sel4-profile-venv/bin/python"
COMPILER_DIR="out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf"
COMPILER_ARCHIVE="out/toolchain/downloads/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz"
QEMU_BIN="/opt/homebrew/bin/qemu-system-aarch64"
GDB_BIN="out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf/bin/aarch64-none-elf-gdb"
JOBS=10
CHECK_ONLY=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --run-dir) [[ $# -ge 2 ]] || die "--run-dir requires a value"; RUN_DIR=$2; shift 2 ;;
        --sel4-source) [[ $# -ge 2 ]] || die "--sel4-source requires a value"; SEL4_SOURCE=$2; shift 2 ;;
        --sel4-build) [[ $# -ge 2 ]] || die "--sel4-build requires a value"; SEL4_BUILD=$2; shift 2 ;;
        --profile-python) [[ $# -ge 2 ]] || die "--profile-python requires a value"; PROFILE_PYTHON=$2; shift 2 ;;
        --compiler-dir) [[ $# -ge 2 ]] || die "--compiler-dir requires a value"; COMPILER_DIR=$2; shift 2 ;;
        --compiler-archive) [[ $# -ge 2 ]] || die "--compiler-archive requires a value"; COMPILER_ARCHIVE=$2; shift 2 ;;
        --qemu) [[ $# -ge 2 ]] || die "--qemu requires a value"; QEMU_BIN=$2; shift 2 ;;
        --gdb) [[ $# -ge 2 ]] || die "--gdb requires a value"; GDB_BIN=$2; shift 2 ;;
        --jobs) [[ $# -ge 2 ]] || die "--jobs requires a value"; JOBS=$2; shift 2 ;;
        --check-only) CHECK_ONLY=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

[[ "$JOBS" =~ ^[0-9]+$ ]] && (( JOBS >= 1 && JOBS <= 32 )) || \
    die "--jobs must be an integer in 1..32"

cd "$REPO_ROOT"
[[ "$(git branch --show-current)" == "main" ]] || die "worktree must be on main"
[[ "$REPO_ROOT" == "/Users/lukasbower/GitHub/cohesix" ]] || \
    die "refusing to clean an unexpected repository root: $REPO_ROOT"
OUT_DIR="$(canonical_existing_dir "$REPO_ROOT/out" "$REPO_ROOT")"
TARGET_DIR="$(canonical_existing_dir "$REPO_ROOT/target" "$REPO_ROOT")"
[[ "$OUT_DIR" == "$REPO_ROOT/out" && "$TARGET_DIR" == "$REPO_ROOT/target" ]] || \
    die "out/ and target/ must resolve to the exact repository directories"
RUN_DIR="$(canonical_future_dir "$RUN_DIR" "$OUT_DIR")"
[[ "$(dirname "$RUN_DIR")" == "$OUT_DIR" ]] || \
    die "--run-dir must be a fresh direct child of the repository out directory"
SEL4_SOURCE="$(canonical_existing_dir "$SEL4_SOURCE" "$OUT_DIR/sel4")"
SEL4_BUILD="$(canonical_future_dir "$SEL4_BUILD" "$OUT_DIR/sel4")"
PROFILE_PYTHON="$(canonical_profile_python "$PROFILE_PYTHON")"
PROFILE_VENV="$(canonical_existing_dir "$(dirname "$(dirname "$PROFILE_PYTHON")")" "$OUT_DIR/toolchain")"
COMPILER_DIR="$(canonical_existing_dir "$COMPILER_DIR" "$OUT_DIR/toolchain")"
COMPILER_ARCHIVE="$(canonical_existing_file "$COMPILER_ARCHIVE" "$OUT_DIR/toolchain" no)"
QEMU_BIN="$(canonical_existing_file "$QEMU_BIN" "" yes)"
GDB_BIN="$(canonical_existing_file "$GDB_BIN" "$COMPILER_DIR" yes)"
[[ "$SEL4_BUILD" != "$SEL4_SOURCE" && "$SEL4_SOURCE" != "$SEL4_BUILD"/* ]] || \
    die "seL4 build must not alias or contain the preserved source"
[[ "$PROFILE_VENV" != "$COMPILER_DIR" && "$COMPILER_ARCHIVE" != "$COMPILER_DIR"/* ]] || \
    die "preserved toolchain inputs must be non-overlapping"
[[ "$SEL4_SOURCE" == "$OUT_DIR/sel4/source-v16-clean" ]] || \
    die "--sel4-source must select the canonical source-v16-clean input"
[[ "$SEL4_BUILD" == "$OUT_DIR/sel4/profile-v2/qemu-smp-production" ]] || \
    die "--sel4-build must select the qemu_smp_production contract path"
[[ "$PROFILE_VENV" == "$OUT_DIR/toolchain/sel4-profile-venv" && \
   "$PROFILE_PYTHON" == "$PROFILE_VENV/bin/python" ]] || \
    die "--profile-python must select the canonical profile virtualenv"
[[ "$COMPILER_DIR" == "$OUT_DIR/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf" ]] || \
    die "--compiler-dir must select the compiler contract install path"
[[ "$COMPILER_ARCHIVE" == "$OUT_DIR/toolchain/downloads/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz" ]] || \
    die "--compiler-archive must select the compiler contract archive"
[[ "$GDB_BIN" == "$COMPILER_DIR/bin/aarch64-none-elf-gdb" ]] || \
    die "--gdb must select the compiler contract GDB"
[[ ! -e "$RUN_DIR" && ! -L "$RUN_DIR" ]] || die "fresh --run-dir already exists: $RUN_DIR"
HOST_VENV="$(canonical_existing_dir "$REPO_ROOT/.venv" "$REPO_ROOT")"
HARNESS_PYTHON="$HOST_VENV/bin/python"
[[ -x "$HARNESS_PYTHON" ]] || die "repository .venv Python is unavailable"

for executable in cargo cpio git lsof shasum /usr/bin/script "$PROFILE_PYTHON" "$QEMU_BIN" "$GDB_BIN" "$HARNESS_PYTHON"; do
    [[ -x "$executable" ]] || command -v "$executable" >/dev/null 2>&1 || \
        die "required executable is unavailable: $executable"
done
COMPILER_ARCHIVE_DIGEST="$(shasum -a 256 "$COMPILER_ARCHIVE" | cut -d ' ' -f 1)"
[[ "$COMPILER_ARCHIVE_DIGEST" =~ ^[0-9a-f]{64}$ ]] || die "cannot hash compiler archive"
python3 - "$REPO_ROOT/configs/sel4/profiles.toml" "$COMPILER_ARCHIVE" \
    "$COMPILER_ARCHIVE_DIGEST" "$COMPILER_DIR" <<'PY'
import hashlib
import json
from pathlib import Path
import stat
import sys
import tomllib

contract_path, archive_raw, digest, install_raw = sys.argv[1:]
contract_raw = Path(contract_path).read_bytes()
contract = tomllib.loads(contract_raw.decode("utf-8"))
compiler = contract["toolchain"]["compiler"]
archive = Path(archive_raw)
install = Path(install_raw)
if (
    digest != compiler["source_archive_sha256"]
    or archive.stat().st_size != compiler["source_archive_size"]
):
    raise SystemExit("compiler archive bytes differ from configs/sel4/profiles.toml")
provenance_path = install / "cohesix-compiler-provenance.json"
provenance_info = provenance_path.lstat()
if not stat.S_ISREG(provenance_info.st_mode) or stat.S_ISLNK(provenance_info.st_mode):
    raise SystemExit("extracted compiler provenance is missing or aliased")
provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
if (
    provenance.get("schema") != "cohesix-compiler-provenance/v1"
    or provenance.get("profile_contract_sha256")
    != hashlib.sha256(contract_raw).hexdigest()
    or provenance.get("source", {}).get("archive_sha256") != digest
    or provenance.get("source", {}).get("archive_size") != archive.stat().st_size
):
    raise SystemExit("extracted compiler provenance differs from the pinned contract")
field_for_suffix = {
    "gcc": "gcc_sha256",
    "g++": "gxx_sha256",
    "cpp": "cpp_sha256",
    "as": "as_sha256",
    "ld": "ld_sha256",
    "objcopy": "objcopy_sha256",
    "ar": "ar_sha256",
    "ranlib": "ranlib_sha256",
}
recorded = provenance.get("compiler", {}).get("program_sha256")
if not isinstance(recorded, dict):
    raise SystemExit("extracted compiler provenance lacks program hashes")
for program in compiler["required_programs"]:
    suffix = program.removeprefix(contract["toolchain"]["cross_prefix"])
    path = install / "bin" / program
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
        raise SystemExit(f"compiler program is missing or aliased: {path}")
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != compiler[field_for_suffix[suffix]] or recorded.get(suffix) != actual:
        raise SystemExit(f"compiler program differs from pinned provenance: {program}")
PY
"$QEMU_BIN" --version >/dev/null
"$GDB_BIN" --version >/dev/null
"$HARNESS_PYTHON" -c 'import json, urllib.request' >/dev/null
"$PROFILE_PYTHON" --version >/dev/null
"$PROFILE_PYTHON" scripts/sel4_profile.py prepare-source \
    --contract configs/sel4/profiles.toml \
    --profile qemu_smp_production \
    --source "$SEL4_SOURCE" \
    --dry-run >/dev/null

require_no_repo_output_writers() {
    python3 - "$OUT_DIR" "$TARGET_DIR" <<'PY'
import os
from pathlib import Path
import subprocess
import sys

roots = tuple(os.fsencode(Path(raw)) for raw in sys.argv[1:])
completed = subprocess.run(
    ("lsof", "-nP", "-w", "-F", "pcan"),
    check=False,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)
if completed.returncode != 0:
    detail = os.fsdecode(completed.stderr).strip() or "no diagnostic"
    raise SystemExit(f"cannot inspect repository output writers with lsof: {detail}")

pid = b"?"
command = b"?"
descriptor = b"?"
access = b""
writers = []
for raw_line in completed.stdout.splitlines():
    field, value = raw_line[:1], raw_line[1:]
    if field == b"p":
        pid = value
        command = b"?"
    elif field == b"c":
        command = value
    elif field == b"f":
        descriptor = value
        access = b""
    elif field == b"a":
        access = value
    elif field == b"n" and access in {b"w", b"u"}:
        if any(value == root or value.startswith(root + b"/") for root in roots):
            writers.append((pid, command, descriptor, access, value))

if writers:
    lines = []
    for writer_pid, writer_command, writer_fd, writer_access, path in writers[:8]:
        lines.append(
            "pid={} command={} fd={} access={} path={}".format(
                os.fsdecode(writer_pid),
                os.fsdecode(writer_command),
                os.fsdecode(writer_fd),
                os.fsdecode(writer_access),
                os.fsdecode(path),
            )
        )
    if len(writers) > len(lines):
        lines.append(f"and {len(writers) - len(lines)} more writable descriptors")
    raise SystemExit(
        "repository out/target has active writable descriptors:\n" + "\n".join(lines)
    )
PY
}

require_quiescent_host() {
    if pgrep -f 'cargo|rustc|cmake|ninja|sel4_profile.py|test_plan_run.sh|qemu-system-aarch64|hive-gateway|rest_perf_harness.py|worker_task_evidence.py|aarch64-none-elf-gdb|host-ticket-agent|gpu-bridge-host|cohesix-build-run.sh|(^|/)cohsh( |$)' >/dev/null 2>&1; then
        die "build, target, or gateway writer process is already active"
    fi
    require_no_repo_output_writers
    local port
    for port in 31337 31339 31349 8080 1234; do
        if lsof -nP -iTCP:"$port" >/dev/null 2>&1; then
            die "required local port is already in use: $port"
        fi
    done
    if lsof -nP -iUDP:31338 >/dev/null 2>&1; then
        die "required local UDP port is already in use: 31338"
    fi
}

validate_clean_ownership() {
    python3 - "$OUT_DIR" "$TARGET_DIR" <<'PY'
import os
from pathlib import Path
import sys

uid = os.getuid()
for root_raw in sys.argv[1:]:
    root = Path(root_raw)
    if root.lstat().st_uid != uid:
        raise SystemExit(f"clean target directory is not user-owned: {root}")
    for directory, names, files in os.walk(root, followlinks=False):
        for name in [*names, *files]:
            path = Path(directory) / name
            if path.lstat().st_uid != uid:
                raise SystemExit(f"clean target contains a non-user-owned entry: {path}")
PY
}

require_quiescent_host
validate_clean_ownership
M26E_AVAILABLE_KIB="$(df -Pk "$REPO_ROOT" | awk 'NR == 2 {print $4}')"
[[ "$M26E_AVAILABLE_KIB" =~ ^[0-9]+$ ]] || die "cannot determine available disk capacity"
(( M26E_AVAILABLE_KIB >= 40 * 1024 * 1024 )) || \
    die "clean QEMU pressure requires at least 40 GiB free"

if (( CHECK_ONLY == 1 )); then
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-gdb --help >/dev/null
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-service-gdb --help >/dev/null
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-critical-gdb --help >/dev/null
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py collect-qemu-preflight --help >/dev/null
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py collect-qemu --help >/dev/null
    log "check-only PASS: inputs are present; no files or processes were changed"
    log "plan: clean target/out, run staged QEMU gates, rebuild release-qemu,bootstrap-trace, collect medium/high"
    exit 0
fi

: "${COH_AUTH_TOKEN:?m26e-qemu-pressure: COH_AUTH_TOKEN is required}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?m26e-qemu-pressure: HIVE_GATEWAY_REQUEST_AUTH_TOKEN is required}"
[[ "$COH_AUTH_TOKEN" != "changeme" ]] || die "placeholder TCP auth token is forbidden"
[[ "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" != "changeme" ]] || \
    die "placeholder REST auth token is forbidden"
M26E_CONSOLE_AUTH_TOKEN=$COH_AUTH_TOKEN
M26E_REST_AUTH_TOKEN=$HIVE_GATEWAY_REQUEST_AUTH_TOKEN
M26E_CONSOLE_AUTH_TOKEN="$M26E_CONSOLE_AUTH_TOKEN" \
M26E_REST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
python3 - <<'PY'
import os

console = os.environ["M26E_CONSOLE_AUTH_TOKEN"]
gateway = os.environ["M26E_REST_AUTH_TOKEN"]
for label, value in (("console", console), ("gateway", gateway)):
    if (
        not value
        or value.strip() != value
        or any(ord(character) < 0x21 or ord(character) == 0x7F for character in value)
    ):
        raise SystemExit(f"{label} secret must be nonempty, trimmed, and control-free")
if console == gateway:
    raise SystemExit("console and gateway request secrets must be distinct")
PY
unset COH_AUTH_TOKEN COHSH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN

PRESERVE_ROOT="$(mktemp -d /private/tmp/cohesix-m26e-qemu-inputs.XXXXXX)"
if [[ "$(stat -f '%d' "$PRESERVE_ROOT")" != "$(stat -f '%d' "$OUT_DIR")" ]]; then
    rmdir "$PRESERVE_ROOT"
    die "preservation directory must share the repository out filesystem"
fi
PRESERVED_ORIGINALS=()
PRESERVED_TEMP=()
QEMU_PID=""
QEMU_LAUNCHER_PID=""
GATEWAY_PID=""
AGENT_PID=""
GPU_REFRESH_PID=""
GDB_RUNNER_PID=""

stop_pid() {
    local pid=${1:-}
    [[ -n "$pid" && "$pid" =~ ^[0-9]+$ ]] || return 0
    if kill -0 "$pid" >/dev/null 2>&1; then
        kill -TERM "$pid" >/dev/null 2>&1 || true
        local deadline=$(( $(date +%s) + 10 ))
        while kill -0 "$pid" >/dev/null 2>&1 && (( $(date +%s) < deadline )); do
            sleep 0.1
        done
        if kill -0 "$pid" >/dev/null 2>&1; then
            kill -KILL "$pid" >/dev/null 2>&1 || true
        fi
    fi
    wait "$pid" >/dev/null 2>&1 || true
}

stop_process_tree() {
    local pid=${1:-}
    [[ -n "$pid" && "$pid" =~ ^[0-9]+$ ]] || return 0
    local child
    while IFS= read -r child; do
        [[ -n "$child" ]] || continue
        stop_process_tree "$child"
    done < <(pgrep -P "$pid" 2>/dev/null || true)
    stop_pid "$pid"
}

restore_preserved_inputs() {
    local index
    for (( index=0; index<${#PRESERVED_ORIGINALS[@]}; index++ )); do
        if [[ -e "${PRESERVED_TEMP[$index]}" ]]; then
            mkdir -p "$(dirname "${PRESERVED_ORIGINALS[$index]}")"
            mv "${PRESERVED_TEMP[$index]}" "${PRESERVED_ORIGINALS[$index]}"
        fi
    done
}

cleanup() {
    local status=$?
    stop_process_tree "$GDB_RUNNER_PID"
    stop_pid "$AGENT_PID"
    stop_pid "$GPU_REFRESH_PID"
    stop_pid "$GATEWAY_PID"
    stop_pid "$QEMU_PID"
    stop_pid "$QEMU_LAUNCHER_PID"
    restore_preserved_inputs
    if [[ -d "$PRESERVE_ROOT" ]]; then
        find "$PRESERVE_ROOT" -depth -mindepth 1 -delete 2>/dev/null || true
        rmdir "$PRESERVE_ROOT" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM HUP

preserve_input() {
    local source=$1
    local index=${#PRESERVED_ORIGINALS[@]}
    local destination="$PRESERVE_ROOT/input-$index"
    [[ -e "$source" ]] || die "preserved input disappeared: $source"
    PRESERVED_TEMP+=("$destination")
    PRESERVED_ORIGINALS+=("$source")
    mv "$source" "$destination"
}

preserve_input "$SEL4_SOURCE"
preserve_input "$COMPILER_DIR"
preserve_input "$PROFILE_VENV"
preserve_input "$COMPILER_ARCHIVE"

log "cleaning validated repository target/ and out/"
cargo clean --manifest-path "$REPO_ROOT/Cargo.toml" --target-dir "$REPO_ROOT/target"
mkdir -p "$REPO_ROOT/target" "$REPO_ROOT/out"
find "$REPO_ROOT/target" -depth -mindepth 1 -delete
find "$REPO_ROOT/out" -depth -mindepth 1 -delete
restore_preserved_inputs
PRESERVED_ORIGINALS=()
PRESERVED_TEMP=()
[[ "$(shasum -a 256 "$COMPILER_ARCHIVE" | cut -d ' ' -f 1)" == "$COMPILER_ARCHIVE_DIGEST" ]] || \
    die "compiler archive changed during clean preservation"

"$PROFILE_PYTHON" scripts/sel4_profile.py configure \
    --contract configs/sel4/profiles.toml \
    --profile qemu_smp_production \
    --source "$SEL4_SOURCE" \
    --build-dir "$SEL4_BUILD"
"$PROFILE_PYTHON" scripts/sel4_profile.py build \
    --contract configs/sel4/profiles.toml \
    --profile qemu_smp_production \
    --source "$SEL4_SOURCE" \
    --build-dir "$SEL4_BUILD" \
    --jobs "$JOBS"
"$PROFILE_PYTHON" scripts/sel4_profile.py validate \
    --contract configs/sel4/profiles.toml \
    --profile qemu_smp_production \
    --source "$SEL4_SOURCE" \
    --build-dir "$SEL4_BUILD" \
    --require-source \
    --require-artifacts \
    --for-release \
    --for-runtime

[[ "$(python3 scripts/lib/detect_gic_version.py "$SEL4_BUILD/kernel/gen_config/kernel/gen_config.h")" == "3" ]] || \
    die "selected seL4 build is not GICv3"

unset QEMU_SMP QEMU_SMP_TOPO QEMU_VIRT QEMU_ACCEL QEMU_MACHINE_EXTRA
unset COHSH_QEMU_ARGS SEL4_BUILD_DIR SEL4_LD COH_RTC_MANIFEST
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS RUSTC RUSTC_WRAPPER
unset CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_TARGET CC CXX AR LD
export CARGO_TARGET_DIR="$REPO_ROOT/target"
export COHESIX_SEL4_PROFILE=qemu_smp_production
export COHESIX_QEMU_SMP_TOPO=4,cores=4,threads=1,sockets=1
export COHESIX_QEMU_VIRT=on
export COHESIX_QEMU_ACCEL=tcg
export COHESIX_QEMU_MACHINE_EXTRA=kernel-irqchip=off

BUILD_RUN="$REPO_ROOT/scripts/cohesix-build-run.sh"
BUILD_ARGS=(
    --sel4-build "$SEL4_BUILD"
    --profile release
    --cargo-target aarch64-unknown-none
    --root-task-features release-qemu,bootstrap-trace
    --qemu "$QEMU_BIN"
    --transport tcp
    --tcp-port 31337
)

log "building canonical release-qemu,bootstrap-trace artifacts"
"$BUILD_RUN" --clean --no-run "${BUILD_ARGS[@]}"

TEST_PLAN_STATE_DIR="$RUN_DIR/test-plan"
log "running the canonical five-stage QEMU test plan"
COH_AUTH_TOKEN="$M26E_CONSOLE_AUTH_TOKEN" \
HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
scripts/ci/test_plan_run.sh \
    --target qemu \
    --state-dir "$TEST_PLAN_STATE_DIR"

require_quiescent_host
log "rebuilding the exact pressure artifact set after staged QEMU validation"
"$BUILD_RUN" --clean --no-run "${BUILD_ARGS[@]}"

OUT_ROOT="$REPO_ROOT/out/cohesix"
HOST_TOOLS="$OUT_ROOT/host-tools"
WORKER_ARCHIVE="$OUT_ROOT/worker-images/cohesix-worker-images.cpio"
WORKER_MANIFEST="$OUT_ROOT/worker-images/cohesix-worker-image-manifest.json"
DRIVER_ARCHIVE="$OUT_ROOT/driver-runtimes/cohesix-driver-runtimes.cpio"
DRIVER_MANIFEST="$OUT_ROOT/driver-runtimes/cohesix-driver-runtime-manifest.json"
ROOT_IMAGE="$OUT_ROOT/staging/rootserver"
ELFLOADER_IMAGE="$OUT_ROOT/staging/elfloader"
SYSTEM_CPIO="$OUT_ROOT/cohesix-system.cpio"
KERNEL_IMAGE="$SEL4_BUILD/kernel/kernel.elf"
GENERATED_INVENTORY="$REPO_ROOT/configs/generated/root_task_topology.json"
RESOLVED_MANIFEST="$REPO_ROOT/configs/generated/root_task_resolved.json"
TARGET_ARTIFACT_DIR="$REPO_ROOT/target/aarch64-unknown-none/release"
WORKER_HEART_ELF="$TARGET_ARTIFACT_DIR/worker-heart"
WORKER_GPU_ELF="$TARGET_ARTIFACT_DIR/worker-gpu"
WORKER_LORA_ELF="$TARGET_ARTIFACT_DIR/worker-lora"
NINEDOOR_ELF="$TARGET_ARTIFACT_DIR/nine-door-runtime"
CONSOLE_NETWORK_ELF="$TARGET_ARTIFACT_DIR/console-network-runtime"
ROOT_ELF="$TARGET_ARTIFACT_DIR/root-task"
LAUNCH_ARTIFACT_RECORD="$OUT_ROOT/cohesix-qemu-launch-artifacts.json"

for artifact in \
    "$HOST_TOOLS/cohsh" "$HOST_TOOLS/hive-gateway" \
    "$HOST_TOOLS/gpu-bridge-host" "$HOST_TOOLS/host-ticket-agent" \
    "$WORKER_ARCHIVE" "$WORKER_MANIFEST" "$DRIVER_ARCHIVE" "$DRIVER_MANIFEST" \
    "$ROOT_IMAGE" "$ELFLOADER_IMAGE" "$SYSTEM_CPIO" \
    "$LAUNCH_ARTIFACT_RECORD" \
    "$KERNEL_IMAGE" "$GENERATED_INVENTORY" "$RESOLVED_MANIFEST" \
    "$WORKER_HEART_ELF" "$WORKER_GPU_ELF" "$WORKER_LORA_ELF" \
    "$NINEDOOR_ELF" "$CONSOLE_NETWORK_ELF" "$ROOT_ELF"; do
    [[ -f "$artifact" && ! -L "$artifact" ]] || die "build artifact is missing or aliased: $artifact"
done

mkdir -p "$RUN_DIR/session"
TARGET_SESSION="$RUN_DIR/session/target-session.json"

python3 - \
    "$REPO_ROOT" "$RUN_DIR/session" "$TARGET_SESSION" \
    "$RESOLVED_MANIFEST" "$KERNEL_IMAGE" "$ROOT_IMAGE" \
    "$DRIVER_ARCHIVE" "$DRIVER_MANIFEST" "$WORKER_ARCHIVE" "$WORKER_MANIFEST" \
    "$GENERATED_INVENTORY" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

(
    repo_raw,
    session_dir_raw,
    session_path_raw,
    manifest_raw,
    kernel_raw,
    root_raw,
    driver_archive_raw,
    driver_manifest_raw,
    worker_archive_raw,
    worker_manifest_raw,
    topology_raw,
) = sys.argv[1:]
repo = Path(repo_raw)
session_dir = Path(session_dir_raw)
session_path = Path(session_path_raw)


def frozen(path_raw: str) -> tuple[Path, bytes]:
    path = Path(path_raw)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
        raise SystemExit(f"target-session input is not a regular non-symlink file: {path}")
    data = path.read_bytes()
    if not data:
        raise SystemExit(f"target-session input is empty: {path}")
    return path, data


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


paths_raw = subprocess.check_output(
    ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=repo
)
paths = sorted({Path(os.fsdecode(item)) for item in paths_raw.split(b"\0") if item})
source_rows = []
for relative in paths:
    path = repo / relative
    if not path.exists() and not path.is_symlink():
        source_rows.append(
            {
                "path": relative.as_posix(),
                "kind": "deleted",
                "mode": 0,
                "sha256": digest(b""),
                "bytes": 0,
            }
        )
        continue
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode):
        kind = "symlink"
        payload = os.readlink(path).encode("utf-8")
    elif stat.S_ISREG(info.st_mode):
        kind = "file"
        payload = path.read_bytes()
    else:
        raise SystemExit(f"unsupported source inventory entry: {relative}")
    source_rows.append(
        {
            "path": relative.as_posix(),
            "kind": kind,
            "mode": stat.S_IMODE(info.st_mode),
            "sha256": digest(payload),
            "bytes": len(payload),
        }
    )
source_inventory = {
    "schema": "cohesix-source-inventory/v1",
    "algorithm": "git-visible-paths-sha256",
    "entries": source_rows,
}
source_bytes = (json.dumps(source_inventory, sort_keys=True, separators=(",", ":")) + "\n").encode()
source_path = session_dir / "source-inventory.json"
source_path.write_bytes(source_bytes)

abi_files = []
for relative in ("crates/worker-task-abi/Cargo.toml", "crates/worker-task-abi/src/lib.rs"):
    path, data = frozen(str(repo / relative))
    abi_files.append({"path": path.relative_to(repo).as_posix(), "sha256": digest(data), "bytes": len(data)})
abi_record = {
    "schema": "cohesix-worker-abi-identity/v1",
    "task_abi_schema": "worker-task-abi/v1",
    "task_abi_version": 1,
    "files": abi_files,
}
abi_bytes = (json.dumps(abi_record, sort_keys=True, separators=(",", ":")) + "\n").encode()
(session_dir / "worker-abi-identity.json").write_bytes(abi_bytes)

_, driver_archive = frozen(driver_archive_raw)
cyw43_record = {
    "schema": "cohesix-cyw43-coexistence-binding/v1",
    "target": "qemu",
    "selected": False,
    "classification": "not-applicable-physical-driver",
    "driver_archive_sha256": digest(driver_archive),
}
cyw43_bytes = (json.dumps(cyw43_record, sort_keys=True, separators=(",", ":")) + "\n").encode()
(session_dir / "qemu-cyw43-coexistence.json").write_bytes(cyw43_bytes)

_, manifest = frozen(manifest_raw)
_, kernel = frozen(kernel_raw)
_, root = frozen(root_raw)
_, driver_manifest = frozen(driver_manifest_raw)
_, worker_archive = frozen(worker_archive_raw)
_, worker_manifest = frozen(worker_manifest_raw)
_, topology_bytes = frozen(topology_raw)
topology = json.loads(topology_bytes)
if topology.get("manifest_sha256") != digest(manifest):
    raise SystemExit("generated topology and resolved manifest hashes differ")

session = {
    "target": "qemu",
    "source_sha256": digest(source_bytes),
    "manifest_sha256": digest(manifest),
    "kernel_sha256": digest(kernel),
    "root_image_sha256": digest(root),
    "driver_archive_sha256": digest(driver_archive),
    "driver_manifest_sha256": digest(driver_manifest),
    "cyw43_coexistence_record_sha256": digest(cyw43_bytes),
    "worker_archive_sha256": digest(worker_archive),
    "worker_image_manifest_sha256": digest(worker_manifest),
    "worker_abi_sha256": digest(abi_bytes),
}
session_path.write_text(json.dumps(session, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

log "target session emitted: $TARGET_SESSION"

ARTIFACT_BINDINGS="$RUN_DIR/session/artifact-bindings.json"
python3 - "$ARTIFACT_BINDINGS" \
    qemu="$QEMU_BIN" \
    gdb="$GDB_BIN" \
    qemu-launch-record="$LAUNCH_ARTIFACT_RECORD" \
    elfloader="$ELFLOADER_IMAGE" \
    system-cpio="$SYSTEM_CPIO" \
    generated-topology="$GENERATED_INVENTORY" \
    resolved-manifest="$RESOLVED_MANIFEST" \
    kernel="$KERNEL_IMAGE" \
    root-image="$ROOT_IMAGE" \
    driver-archive="$DRIVER_ARCHIVE" \
    driver-manifest="$DRIVER_MANIFEST" \
    worker-archive="$WORKER_ARCHIVE" \
    worker-manifest="$WORKER_MANIFEST" \
    root-elf="$ROOT_ELF" \
    ninedoor-elf="$NINEDOOR_ELF" \
    console-network-elf="$CONSOLE_NETWORK_ELF" \
    worker-heartbeat-elf="$WORKER_HEART_ELF" \
    worker-gpu-elf="$WORKER_GPU_ELF" \
    worker-lora-elf="$WORKER_LORA_ELF" \
    cohsh="$HOST_TOOLS/cohsh" \
    hive-gateway="$HOST_TOOLS/hive-gateway" \
    gpu-bridge-host="$HOST_TOOLS/gpu-bridge-host" \
    host-ticket-agent="$HOST_TOOLS/host-ticket-agent" \
    source-inventory="$RUN_DIR/session/source-inventory.json" \
    worker-abi-identity="$RUN_DIR/session/worker-abi-identity.json" <<'PY'
import hashlib
import json
from pathlib import Path
import stat
import sys

out = Path(sys.argv[1])
rows = []
for item in sys.argv[2:]:
    identifier, separator, raw_path = item.partition("=")
    if not identifier or separator != "=" or not raw_path:
        raise SystemExit("malformed artifact binding")
    path = Path(raw_path)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
        raise SystemExit(f"artifact binding is not a regular non-symlink file: {path}")
    raw = path.read_bytes()
    if not raw:
        raise SystemExit(f"artifact binding is empty: {path}")
    rows.append(
        {
            "id": identifier,
            "path": str(path),
            "sha256": hashlib.sha256(raw).hexdigest(),
            "bytes": len(raw),
        }
    )
if len(rows) != len({row["id"] for row in rows}):
    raise SystemExit("artifact binding ids are not unique")
payload = {"schema": "cohesix-qemu-artifact-bindings/v1", "artifacts": sorted(rows, key=lambda row: row["id"])}
out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

SYSTEM_CPIO_BYTES="$(stat -f '%z' "$SYSTEM_CPIO")"
[[ "$SYSTEM_CPIO_BYTES" =~ ^[0-9]+$ ]] && (( SYSTEM_CPIO_BYTES < 4 * 1024 * 1024 )) || \
    die "QEMU rootfs CPIO is not below 4 MiB"

# The live boot/receipt/fault functions below deliberately consume only existing
# console paths and external GDB hooks. They fail closed until every marker and
# artifact required by scripts/worker_task_evidence.py is present.

wait_for_file() {
    local path=$1
    local timeout=$2
    local deadline=$(( $(date +%s) + timeout ))
    while [[ ! -s "$path" ]]; do
        (( $(date +%s) < deadline )) || die "timed out waiting for file: $path"
        sleep 0.1
    done
}

wait_for_port() {
    local host=$1
    local port=$2
    local timeout=$3
    python3 - "$host" "$port" "$timeout" <<'PY'
import socket
import sys
import time

host, port_raw, timeout_raw = sys.argv[1:]
deadline = time.monotonic() + float(timeout_raw)
while time.monotonic() < deadline:
    try:
        with socket.create_connection((host, int(port_raw)), timeout=0.25):
            raise SystemExit(0)
    except OSError:
        time.sleep(0.1)
raise SystemExit(f"timed out waiting for {host}:{port_raw}")
PY
}

wait_for_marker_count() {
    local file=$1
    local literal=$2
    local expected=$3
    local timeout=$4
    local deadline=$(( $(date +%s) + timeout ))
    while true; do
        local observed=0
        if [[ -f "$file" ]]; then
            observed=$(grep -F -c "$literal" "$file" 2>/dev/null || true)
        fi
        (( observed >= expected )) && return 0
        (( $(date +%s) < deadline )) || \
            die "timed out waiting for marker count $expected: $literal"
        sleep 0.1
    done
}

run_cohsh_command() {
    local boot_dir=$1
    local command=$2
    local ordinal=$3
    local expectation=${4:-OK}
    local script_path="$boot_dir/cohsh-command-$ordinal.coh"
    {
        printf '# Author: Lukas Bower\n'
        printf '# Purpose: Drive one existing Milestone 26e QEMU control operation.\n'
        printf '# Copyright 2026 Lukas Bower\n'
        printf 'attach queen\nEXPECT OK\n%s\n' "$command"
        if [[ "$expectation" != "NONE" ]]; then
            printf 'EXPECT %s\n' "$expectation"
        fi
        printf 'quit\n'
    } > "$script_path"
    COH_AUTH_TOKEN="$M26E_CONSOLE_AUTH_TOKEN" \
    "$HOST_TOOLS/cohsh" \
        --transport tcp \
        --tcp-host 127.0.0.1 \
        --tcp-port 31337 \
        --script "$script_path" >> "$boot_dir/cohsh.log" 2>&1
}

wait_for_cohsh_success() {
    local boot_dir=$1
    local command=$2
    local ordinal=$3
    local deadline=$(( $(date +%s) + 60 ))
    while (( $(date +%s) < deadline )); do
        if run_cohsh_command "$boot_dir" "$command" "$ordinal" OK; then
            return 0
        fi
        sleep 0.2
    done
    die "timed out waiting for cohsh command after service reconstruction: $command"
}

current_worker_id() {
    local role=$1
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    python3 - "$REPO_ROOT/scripts/rest_perf_harness.py" "$role" <<'PY'
import importlib.util
import os
import sys

module_path, role = sys.argv[1:]
spec = importlib.util.spec_from_file_location("m26e_worker_lookup", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load REST harness for Worker lookup")
rest = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rest
spec.loader.exec_module(rest)
client = rest.RestClient(
    "http://127.0.0.1:8080",
    10.0,
    os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"],
)
bounds = client.get_json("/v1/meta/bounds")
instances, _ = rest.discover_executable_workers(client, bounds)
matches = [
    instance.worker_id
    for instance in instances
    if instance.role == role and instance.lifecycle == "ready"
]
if len(matches) != 1:
    raise SystemExit(f"expected exactly one READY {role}, got {len(matches)}")
print(matches[0])
PY
}

spawn_command_for_role() {
    case "$1" in
        worker-heartbeat) printf '%s\n' 'spawn heartbeat ticks=100 ttl_s=120 ops=500' ;;
        worker-gpu) printf '%s\n' 'spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120 priority=1' ;;
        worker-lora) printf '%s\n' 'spawn lora' ;;
        *) die "unsupported executable Worker role: $1" ;;
    esac
}

freeze_prefix() {
    local source=$1
    local destination=$2
    python3 - "$source" "$destination" <<'PY'
from pathlib import Path
import os
import sys

source = Path(sys.argv[1])
destination = Path(sys.argv[2])
data = source.read_bytes()
end = data.rfind(b"\n")
if end < 0:
    raise SystemExit("UART snapshot has no complete line")
payload = data[: end + 1]
temporary = destination.with_name(f".{destination.name}.{os.getpid()}")
temporary.write_bytes(payload)
temporary.replace(destination)
PY
}

verify_live_artifacts() {
    python3 - "$REPO_ROOT" "$TARGET_SESSION" "$ARTIFACT_BINDINGS" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

repo = Path(sys.argv[1])
session = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
binding = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
if binding.get("schema") != "cohesix-qemu-artifact-bindings/v1":
    raise SystemExit("live artifact binding schema is invalid")
rows = binding.get("artifacts")
if not isinstance(rows, list) or not rows:
    raise SystemExit("live artifact binding is empty")
artifacts = {}
for row in rows:
    if set(row) != {"id", "path", "sha256", "bytes"} or row["id"] in artifacts:
        raise SystemExit("live artifact binding row is malformed")
    path = Path(row["path"])
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
        raise SystemExit(f"live artifact is missing or aliased: {path}")
    raw = path.read_bytes()
    if len(raw) != row["bytes"] or hashlib.sha256(raw).hexdigest() != row["sha256"]:
        raise SystemExit(f"live artifact bytes drifted: {row['id']}")
    artifacts[row["id"]] = (path, raw)

field_ids = {
    "manifest_sha256": "resolved-manifest",
    "kernel_sha256": "kernel",
    "root_image_sha256": "root-image",
    "driver_archive_sha256": "driver-archive",
    "driver_manifest_sha256": "driver-manifest",
    "worker_archive_sha256": "worker-archive",
    "worker_image_manifest_sha256": "worker-manifest",
}
for field, identifier in field_ids.items():
    if hashlib.sha256(artifacts[identifier][1]).hexdigest() != session[field]:
        raise SystemExit(f"live target session drift: {field}")

source = json.loads(artifacts["source-inventory"][1])
source_rows = source.get("entries")
if source.get("schema") != "cohesix-source-inventory/v1" or not isinstance(source_rows, list):
    raise SystemExit("source inventory binding is invalid")
current_paths_raw = subprocess.check_output(
    ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=repo
)
current_paths = sorted({
    Path(os.fsdecode(item)).as_posix()
    for item in current_paths_raw.split(b"\0")
    if item
})
if current_paths != [row["path"] for row in source_rows]:
    raise SystemExit("git-visible source path set changed after target-session binding")
for row in source_rows:
    path = repo / row["path"]
    if row["kind"] == "deleted":
        if path.exists() or path.is_symlink():
            raise SystemExit(f"deleted source entry reappeared: {row['path']}")
        raw = b""
        mode = 0
    else:
        info = path.lstat()
        mode = stat.S_IMODE(info.st_mode)
        if row["kind"] == "symlink" and stat.S_ISLNK(info.st_mode):
            raw = os.readlink(path).encode("utf-8")
        elif row["kind"] == "file" and stat.S_ISREG(info.st_mode):
            raw = path.read_bytes()
        else:
            raise SystemExit(f"source entry kind changed: {row['path']}")
    if (
        len(raw) != row["bytes"]
        or mode != row["mode"]
        or hashlib.sha256(raw).hexdigest() != row["sha256"]
    ):
        raise SystemExit(f"source entry bytes changed: {row['path']}")
if hashlib.sha256(artifacts["source-inventory"][1]).hexdigest() != session["source_sha256"]:
    raise SystemExit("source inventory differs from target session")

abi = json.loads(artifacts["worker-abi-identity"][1])
for row in abi.get("files", []):
    raw = (repo / row["path"]).read_bytes()
    if len(raw) != row["bytes"] or hashlib.sha256(raw).hexdigest() != row["sha256"]:
        raise SystemExit(f"Worker ABI source changed: {row['path']}")
if hashlib.sha256(artifacts["worker-abi-identity"][1]).hexdigest() != session["worker_abi_sha256"]:
    raise SystemExit("Worker ABI identity differs from target session")

topology = json.loads(artifacts["generated-topology"][1])
if topology.get("manifest_sha256") != session["manifest_sha256"]:
    raise SystemExit("generated topology differs from target session manifest")
if len(artifacts["system-cpio"][1]) >= 4 * 1024 * 1024:
    raise SystemExit("live rootfs CPIO exceeded the 4 MiB guard")
PY
}

verify_qemu_command() {
    local boot_dir=$1
    local paused=$2
    python3 - "$boot_dir/qemu-command.txt" "$QEMU_BIN" "$ELFLOADER_IMAGE" \
        "$SYSTEM_CPIO" "$boot_dir/qemu.pid" "$paused" <<'PY'
from pathlib import Path
import shlex
import sys

command_path, qemu, elfloader, system_cpio, pidfile, paused = sys.argv[1:]
tokens = shlex.split(Path(command_path).read_text(encoding="utf-8"))
if not tokens or Path(tokens[0]).resolve(strict=True) != Path(qemu):
    raise SystemExit("QEMU command does not bind the hashed executable")


def exact_option(option: str, expected: str) -> None:
    positions = [index for index, token in enumerate(tokens) if token == option]
    if len(positions) != 1 or positions[0] + 1 >= len(tokens):
        raise SystemExit(f"QEMU command has an ambiguous {option}")
    if tokens[positions[0] + 1] != expected:
        raise SystemExit(f"QEMU command {option} differs from exact target truth")


exact_option("-machine", "virt,gic-version=3,virtualization=on,kernel-irqchip=off")
exact_option("-smp", "4,cores=4,threads=1,sockets=1")
exact_option("-kernel", elfloader)
exact_option("-initrd", system_cpio)
exact_option("-pidfile", pidfile)
exact_option("-gdb", "tcp:127.0.0.1:1234")
if ("-S" in tokens) != (paused == "yes"):
    raise SystemExit("QEMU paused-boot state differs from the declared evidence phase")
if "-accel" not in tokens or "tcg" not in tokens:
    raise SystemExit("QEMU command does not select the canonical TCG accelerator")
if not any("virtio-net-device" in token for token in tokens):
    raise SystemExit("QEMU command lacks the canonical virtio network device")
PY
}

start_gateway() {
    local boot_dir=$1
    local evidence=${2:-}
    local target_session=${3:-}
    stop_pid "$GATEWAY_PID"
    GATEWAY_PID=""
    local gateway_args=(--bind 127.0.0.1:8080)
    if [[ -n "$evidence" ]]; then
        gateway_args+=(
            --worker-acceptance-root "$boot_dir"
            --worker-acceptance-evidence "$evidence"
            --target-session "$target_session"
        )
    fi
    COH_AUTH_TOKEN="$M26E_CONSOLE_AUTH_TOKEN" \
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    COH_TCP_HOST=127.0.0.1 \
    COH_TCP_PORT=31337 \
    COH_ROLE=queen \
    "$HOST_TOOLS/hive-gateway" "${gateway_args[@]}" \
        > "$boot_dir/gateway.log" 2>&1 &
    GATEWAY_PID=$!
    wait_for_port 127.0.0.1 8080 60
}

assert_gateway_unproven() {
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    python3 - "$REPO_ROOT/scripts/rest_perf_harness.py" <<'PY'
import importlib.util
import os
import sys

spec = importlib.util.spec_from_file_location("m26e_unproven_gateway", sys.argv[1])
if spec is None or spec.loader is None:
    raise SystemExit("cannot load REST harness for gateway proof check")
rest = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rest
spec.loader.exec_module(rest)
client = rest.RestClient(
    "http://127.0.0.1:8080",
    10.0,
    os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"],
)
status = client.get_json("/v1/meta/status")
diagnostic = status.get("worker_acceptance_diagnostic")
if (
    status.get("backend_class") != "console-projection"
    or status.get("worker_acceptance") is not None
    or not isinstance(diagnostic, dict)
    or diagnostic.get("code") != "not-configured"
):
    raise SystemExit("preflight gateway falsely projects executable acceptance")
PY
}

publish_gpu_fixture() {
    local boot_dir=$1
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    "$HOST_TOOLS/gpu-bridge-host" \
        --mock \
        --registry "$boot_dir/peft-registry" \
        --publish \
        --rest-url http://127.0.0.1:8080 \
        >> "$boot_dir/gpu-fixture.log" 2>&1
}

start_pressure_helpers() {
    local boot_dir=$1
    stop_pid "$AGENT_PID"
    stop_pid "$GPU_REFRESH_PID"
    AGENT_PID=""
    GPU_REFRESH_PID=""
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    "$HOST_TOOLS/gpu-bridge-host" \
        --mock \
        --registry "$boot_dir/peft-registry" \
        --publish \
        --interval-ms 5000 \
        --rest-url http://127.0.0.1:8080 \
        >> "$boot_dir/gpu-fixture.log" 2>&1 &
    GPU_REFRESH_PID=$!
    local state_dir="$boot_dir/host-ticket-agent"
    mkdir -p "$state_dir"
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    "$HOST_TOOLS/host-ticket-agent" \
        --manifest "$RESOLVED_MANIFEST" \
        --cursor "$state_dir/cursor.json" \
        --execution-journal "$state_dir/execution-journal.json" \
        --agent-lock "$state_dir/agent.lock" \
        --poll-ms 100 \
        --rest-url http://127.0.0.1:8080 \
        --registry-root "$boot_dir/peft-registry" \
        --export-root "$boot_dir/peft-exports" \
        --adapter-root "$boot_dir/peft-adapters" \
        >> "$boot_dir/host-ticket-agent.log" 2>&1 &
    AGENT_PID=$!
    sleep 1
    kill -0 "$GPU_REFRESH_PID" >/dev/null 2>&1 || die "GPU fixture refresh exited before pressure"
    kill -0 "$AGENT_PID" >/dev/null 2>&1 || die "host-ticket-agent exited before pressure"
}

stop_pressure_helpers() {
    stop_pid "$AGENT_PID"
    AGENT_PID=""
    stop_pid "$GPU_REFRESH_PID"
    GPU_REFRESH_PID=""
}

prepare_host_fixture() {
    local boot_dir=$1
    python3 - "$boot_dir" <<'PY'
import hashlib
from pathlib import Path
import sys

root = Path(sys.argv[1])
registry = root / "peft-registry"
base = registry / "available" / "vision-base-v1"
lora = registry / "available" / "vision-lora-edge"
adapter = root / "peft-adapters" / "fixture-adapter"
for path in (base, lora, adapter, root / "peft-exports"):
    path.mkdir(parents=True, exist_ok=True)

adapter_bytes = b"fixture-adapter\n"
lora_bytes = b'{"rank":8}\n'
policy_bytes = b"[policy]\nmode = \"fixture\"\n"
telemetry_bytes = b"fixture-telemetry\n"
sha = lambda value: hashlib.sha256(value).hexdigest()
(adapter / "adapter.safetensors").write_bytes(adapter_bytes)
(adapter / "lora.json").write_bytes(lora_bytes)
(base / "manifest.toml").write_text(
    '[model]\nid = "vision-base-v1"\n'
    f'cas_sha256 = "{sha(b"fixture-base")}"\nformat = "gguf"\n',
    encoding="utf-8",
)
(lora / "manifest.toml").write_text(
    '[model]\nid = "vision-lora-edge"\n'
    f'cas_sha256 = "{sha(adapter_bytes)}"\n'
    'base = "vision-base-v1"\n'
    f'adapter_sha256 = "{sha(adapter_bytes)}"\n'
    'format = "safetensors+lora"\nadapter = "adapter.safetensors"\n'
    'lora = "lora.json"\n\n[provenance]\njob_id = "qemu-fixture-job"\n'
    'approval = "fixture"\n\n[hashes]\n'
    f'adapter_sha256 = "{sha(adapter_bytes)}"\nadapter_bytes = {len(adapter_bytes)}\n'
    f'lora_sha256 = "{sha(lora_bytes)}"\nlora_bytes = {len(lora_bytes)}\n'
    f'policy_sha256 = "{sha(policy_bytes)}"\npolicy_bytes = {len(policy_bytes)}\n'
    f'telemetry_sha256 = "{sha(telemetry_bytes)}"\ntelemetry_bytes = {len(telemetry_bytes)}\n',
    encoding="utf-8",
)
(registry / "active").write_text("vision-lora-edge\n", encoding="utf-8")
PY
}

trigger_disposable_worker_control() {
    local boot_dir=$1
    local role=$2
    local ordinal=$3
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    python3 - "$REPO_ROOT/scripts/rest_perf_harness.py" \
        "$HOST_TOOLS/host-ticket-agent" "$RESOLVED_MANIFEST" \
        "$boot_dir" "$role" "$ordinal" <<'PY'
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

module_path, agent_raw, manifest_raw, boot_raw, role, ordinal = sys.argv[1:]
spec = importlib.util.spec_from_file_location("m26e_fault_control", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load REST harness for disposable Worker control")
rest = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rest
spec.loader.exec_module(rest)
client = rest.RestClient(
    "http://127.0.0.1:8080",
    10.0,
    os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"],
)
bounds = client.get_json("/v1/meta/bounds")
instances, _ = rest.discover_executable_workers(client, bounds)
ready = [
    instance
    for instance in instances
    if instance.role == role and instance.lifecycle == "ready"
]
if len(ready) != 1:
    raise SystemExit(f"expected exactly one READY {role} for disposable control")
gpu_id, job_id = rest.require_qemu_fixture_receipt_paths(client)
if role == "worker-gpu":
    action = "gpu.lease.grant"
    args = {"ttl_s": 60, "priority": 1}
    subject = gpu_id
    operation = f"gpu-fault-{ordinal}"
elif role == "worker-lora":
    action = "peft.export"
    args = {}
    subject = job_id
    operation = f"peft-fault-{ordinal}"
else:
    raise SystemExit(f"disposable v2 control is not defined for {role}")
worker = ready[0]
ticket = {
    "schema": "host-ticket/v2",
    "id": f"m26e-fault-{role}-{ordinal}",
    "idempotency_key": f"m26e-fault-idem-{role}-{ordinal}",
    "action": action,
    "args": args,
    "receipt_mode": "worker",
    "operation_id": operation,
    "subject_ref": subject,
    "receipt_worker_role": role,
    "receipt_worker_id": worker.worker_id,
    "receipt_supervisor_generation": worker.supervisor_generation,
    "receipt_cap_generation": worker.cap_generation,
}
response = client.echo(
    "/host/tickets/spec",
    json.dumps(ticket, separators=(",", ":")),
)
if response.status != "OK":
    raise SystemExit(f"disposable control ticket was not admitted: {response.error}")
boot = Path(boot_raw)
state = boot / "fault-agent"
state.mkdir(parents=True, exist_ok=True)
command = [
    agent_raw,
    "--manifest", manifest_raw,
    "--cursor", str(state / "cursor.json"),
    "--execution-journal", str(state / "execution-journal.json"),
    "--agent-lock", str(state / "agent.lock"),
    "--run-once",
    "--rest-url", "http://127.0.0.1:8080",
    "--registry-root", str(boot / "peft-registry"),
    "--export-root", str(boot / "peft-exports"),
    "--adapter-root", str(boot / "peft-adapters"),
]
completed = subprocess.run(
    command,
    check=False,
    capture_output=True,
    text=True,
    timeout=30,
)
with (boot / "fault-agent.log").open("a", encoding="utf-8") as handle:
    handle.write(completed.stdout)
    handle.write(completed.stderr)
if completed.returncode != 0:
    raise SystemExit(f"disposable control agent pass failed: {completed.returncode}")
PY
}

drive_worker_fault_plan() {
    local boot_dir=$1
    local role=$2
    local ordinal_base=$3
    local gdb_log="$boot_dir/$role.gdb.log"
    local before ready_before
    before=$(grep -F -c "WORKER_TASK_TEARDOWN role=$role " "$boot_dir/uart.live.log" 2>/dev/null || true)
    ready_before=$(grep -F -c "WORKER_TASK_READY role=$role " "$boot_dir/uart.live.log" 2>/dev/null || true)
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-gdb \
        --gdb "$GDB_BIN" \
        --remote 127.0.0.1:1234 \
        --target-session "$boot_dir/target-session.json" \
        --generated-inventory "$GENERATED_INVENTORY" \
        --worker-image-manifest "$WORKER_MANIFEST" \
        --worker-elf "worker-heartbeat=$WORKER_HEART_ELF" \
        --worker-elf "worker-gpu=$WORKER_GPU_ELF" \
        --worker-elf "worker-lora=$WORKER_LORA_ELF" \
        --inject-role "$role" \
        --timeout-secs 600 \
        --out "$gdb_log" &
    local gdb_pid=$!
    GDB_RUNNER_PID=$gdb_pid
    sleep 1
    run_cohsh_command "$boot_dir" "$(spawn_command_for_role "$role")" "$ordinal_base" NONE || true
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_TEARDOWN role=$role " $(( before + 1 )) 120
    run_cohsh_command "$boot_dir" "$(spawn_command_for_role "$role")" $(( ordinal_base + 1 )) NONE || true
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_READY role=$role " $(( ready_before + 1 )) 120
    if [[ "$role" != "worker-heartbeat" ]]; then
        trigger_disposable_worker_control "$boot_dir" "$role" 1
    fi
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_TEARDOWN role=$role " $(( before + 2 )) 120
    run_cohsh_command "$boot_dir" "$(spawn_command_for_role "$role")" $(( ordinal_base + 2 )) NONE || true
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_READY role=$role " $(( ready_before + 2 )) 120
    if [[ "$role" != "worker-heartbeat" ]]; then
        trigger_disposable_worker_control "$boot_dir" "$role" 2
    fi
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_TEARDOWN role=$role " $(( before + 3 )) 120
    if ! wait "$gdb_pid"; then
        GDB_RUNNER_PID=""
        die "Worker GDB plan failed for $role"
    fi
    GDB_RUNNER_PID=""
    run_cohsh_command "$boot_dir" "$(spawn_command_for_role "$role")" $(( ordinal_base + 3 ))
    wait_for_marker_count "$boot_dir/uart.live.log" "WORKER_TASK_READY role=$role " $(( ready_before + 3 )) 120
}

drive_service_fault_plan() {
    local boot_dir=$1
    local service=$2
    local service_elf=$3
    local teardown_marker=$4
    local ordinal=$5
    local gdb_log="$boot_dir/$service.gdb.log"
    local teardown_before
    teardown_before=$(grep -F -c "$teardown_marker" "$boot_dir/uart.live.log" 2>/dev/null || true)
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-service-gdb \
        --gdb "$GDB_BIN" \
        --remote 127.0.0.1:1234 \
        --target-session "$boot_dir/target-session.json" \
        --generated-inventory "$GENERATED_INVENTORY" \
        --service "$service" \
        --service-elf "$service_elf" \
        --timeout-secs 600 \
        --out "$gdb_log" &
    local gdb_pid=$!
    GDB_RUNNER_PID=$gdb_pid
    sleep 1
    run_cohsh_command "$boot_dir" 'ls /' "$ordinal" NONE || true
    wait_for_marker_count "$boot_dir/uart.live.log" "$teardown_marker" $(( teardown_before + 1 )) 120
    wait_for_port 127.0.0.1 31337 120
    if [[ "$service" == "console-network" ]]; then
        run_cohsh_command "$boot_dir" 'ls /' $(( ordinal + 1 )) NONE || true
        wait_for_marker_count "$boot_dir/uart.live.log" "$teardown_marker" $(( teardown_before + 2 )) 120
        wait_for_port 127.0.0.1 31337 120
    fi
    if ! wait "$gdb_pid"; then
        GDB_RUNNER_PID=""
        die "service GDB plan failed for $service"
    fi
    GDB_RUNNER_PID=""
    wait_for_cohsh_success "$boot_dir" 'ls /' $(( ordinal + 2 ))
}

drive_operator_lifecycle() {
    local boot_dir=$1
    local heartbeat_id teardown_before ready_before
    heartbeat_id="$(current_worker_id worker-heartbeat)"
    teardown_before=$(grep -F -c 'WORKER_TASK_TEARDOWN role=worker-heartbeat ' "$boot_dir/uart.live.log" 2>/dev/null || true)
    ready_before=$(grep -F -c 'WORKER_TASK_READY role=worker-heartbeat ' "$boot_dir/uart.live.log" 2>/dev/null || true)
    run_cohsh_command "$boot_dir" "kill $heartbeat_id" 401 OK
    wait_for_marker_count "$boot_dir/uart.live.log" 'WORKER_TASK_TEARDOWN role=worker-heartbeat ' $(( teardown_before + 1 )) 120
    run_cohsh_command "$boot_dir" 'spawn heartbeat ticks=100 ttl_s=120 ops=500' 402 OK
    wait_for_marker_count "$boot_dir/uart.live.log" 'WORKER_TASK_READY role=worker-heartbeat ' $(( ready_before + 1 )) 120
    run_cohsh_command "$boot_dir" 'spawn heartbeat ticks=100 ttl_s=120 ops=500' 403 ERR
    run_cohsh_command "$boot_dir" 'spawn worker-bus' 404 ERR
    run_cohsh_command "$boot_dir" 'cat /gpu/bridge/status' 405 OK
    run_cohsh_command "$boot_dir" 'ls /shard' 406 OK
}

drive_receipt_matrix() {
    local boot_dir=$1
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    python3 - \
        "$REPO_ROOT/scripts/rest_perf_harness.py" \
        "$HOST_TOOLS/host-ticket-agent" \
        "$HOST_TOOLS/gpu-bridge-host" \
        "$RESOLVED_MANIFEST" \
        "$boot_dir" <<'PY'
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time

module_path, agent_raw, bridge_raw, manifest_raw, boot_raw = sys.argv[1:]
spec = importlib.util.spec_from_file_location("m26e_rest_perf", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load REST harness")
rest = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rest
spec.loader.exec_module(rest)

agent = Path(agent_raw)
bridge = Path(bridge_raw)
manifest = Path(manifest_raw)
boot = Path(boot_raw)
client = rest.RestClient(
    "http://127.0.0.1:8080",
    10.0,
    os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"],
)
bounds = client.get_json("/v1/meta/bounds")
gpu_id, export_job = rest.require_qemu_fixture_receipt_paths(client)
state_dir = boot / "host-ticket-agent"
state_dir.mkdir(parents=True, exist_ok=True)
agent_log = boot / "host-ticket-agent.log"
records = []
sequence = 0


def instances():
    rows, _ = rest.discover_executable_workers(client, bounds)
    return rows


def ready(role):
    rows = [row for row in instances() if row.role == role and row.lifecycle == "ready"]
    if len(rows) != 1:
        raise RuntimeError(f"expected exactly one READY {role}, got {len(rows)}")
    return rows[0]


def wait_fresh(role, generation):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            row = ready(role)
        except Exception:
            time.sleep(0.1)
            continue
        if row.supervisor_generation > generation:
            return row
        time.sleep(0.1)
    raise RuntimeError(f"fresh {role} generation was not observed")


def run_agent():
    command = [
        str(agent),
        "--manifest", str(manifest),
        "--cursor", str(state_dir / "cursor.json"),
        "--execution-journal", str(state_dir / "execution-journal.json"),
        "--agent-lock", str(state_dir / "agent.lock"),
        "--run-once",
        "--rest-url", "http://127.0.0.1:8080",
        "--registry-root", str(boot / "peft-registry"),
        "--export-root", str(boot / "peft-exports"),
        "--adapter-root", str(boot / "peft-adapters"),
    ]
    result = subprocess.run(command, check=False, capture_output=True, text=True, timeout=30)
    with agent_log.open("a", encoding="utf-8") as handle:
        handle.write(result.stdout)
        handle.write(result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"host-ticket-agent --run-once failed: {result.returncode}")


def republish():
    result = subprocess.run(
        [
            str(bridge), "--mock", "--registry", str(boot / "peft-registry"),
            "--publish", "--rest-url", "http://127.0.0.1:8080",
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
        env=os.environ.copy(),
    )
    with (boot / "gpu-fixture.log").open("a", encoding="utf-8") as handle:
        handle.write(result.stdout)
        handle.write(result.stderr)
    if result.returncode != 0:
        raise RuntimeError("GPU fixture republish failed")


def terminal(ticket_id):
    for path in ("/host/tickets/status", "/host/tickets/deadletter"):
        response = client.tail(path, 8192)
        if response.status != "OK":
            raise RuntimeError(f"cannot read {path}")
        for line in response.lines:
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if row.get("schema") == "host-ticket-result/v2" and row.get("id") == ticket_id:
                if row.get("state") in {"succeeded", "failed", "expired"}:
                    return row["state"]
    return None


def submit(action, role, args, subject, expected, operation_id):
    global sequence
    sequence += 1
    before = ready(role)
    ticket_id = f"m26e-{sequence:03d}-{expected}"
    payload = {
        "schema": "host-ticket/v2",
        "id": ticket_id,
        "idempotency_key": f"m26e-idem-{sequence:03d}",
        "action": action,
        "args": args,
        "receipt_mode": "worker",
        "operation_id": operation_id,
        "subject_ref": subject,
        "receipt_worker_role": role,
        "receipt_worker_id": before.worker_id,
        "receipt_supervisor_generation": before.supervisor_generation,
        "receipt_cap_generation": before.cap_generation,
    }
    response = client.echo("/host/tickets/spec", json.dumps(payload, separators=(",", ":")))
    if response.status != "OK":
        raise RuntimeError(f"ticket admission failed for {action}/{expected}: {response.error}")
    if expected == "expired":
        response = client.echo(
            "/queen/ctl",
            json.dumps({"kill": before.worker_id}, separators=(",", ":")),
        )
        if response.status != "OK":
            raise RuntimeError(f"stale driver could not kill {before.worker_id}")
        response = client.echo(
            "/queen/ctl",
            json.dumps({"spawn": "gpu" if role == "worker-gpu" else "lora"}),
        )
        if response.status != "OK":
            raise RuntimeError(f"stale driver could not recreate {role}")
        wait_fresh(role, before.supervisor_generation)
    run_agent()
    deadline = time.monotonic() + 20
    observed = None
    while time.monotonic() < deadline:
        observed = terminal(ticket_id)
        if observed is not None:
            break
        time.sleep(0.1)
    if observed != expected:
        raise RuntimeError(f"{action} expected {expected}, observed {observed}")
    records.append({"action": action, "role": role, "outcome": expected, "ticket_id": ticket_id})


confirmed = [
    ("gpu.lease.grant", "worker-gpu", {"ttl_s": 60, "priority": 1}, gpu_id, "lease-confirmed"),
    ("gpu.lease.renew", "worker-gpu", {"ttl_s": 60, "priority": 1}, gpu_id, "lease-confirmed"),
    ("gpu.lease.release", "worker-gpu", {"reason": "m26e-confirmed"}, gpu_id, "lease-confirmed"),
    ("peft.export", "worker-lora", {}, export_job, "peft-export-confirmed"),
    ("peft.import", "worker-lora", {"adapter_ref": "fixture-adapter", "job_id": export_job}, "m26e-lora", "peft-import-confirmed"),
]
for row in confirmed:
    submit(*row[:4], "succeeded", row[4])
republish()
submit("peft.activate", "worker-lora", {}, "m26e-lora", "succeeded", "peft-activate-confirmed")
submit("peft.rollback", "worker-lora", {}, "vision-lora-edge", "succeeded", "peft-rollback-confirmed")

rejected = [
    ("gpu.lease.grant", "worker-gpu", {"ttl_s": 60}, "GPU-MISSING", "gpu-reject-grant"),
    ("gpu.lease.renew", "worker-gpu", {"ttl_s": 60}, "GPU-MISSING", "gpu-reject-renew"),
    ("gpu.lease.release", "worker-gpu", {"reason": "m26e-reject"}, "GPU-MISSING", "gpu-reject-release"),
    ("peft.export", "worker-lora", {}, "missing-export-job", "peft-reject-export"),
    ("peft.import", "worker-lora", {"adapter_ref": "missing-adapter", "job_id": export_job}, "missing-import", "peft-reject-import"),
    ("peft.activate", "worker-lora", {}, "missing-model", "peft-reject-activate"),
    ("peft.rollback", "worker-lora", {}, "missing-model", "peft-reject-rollback"),
]
for row in rejected:
    submit(*row[:4], "failed", row[4])

stale = [
    ("gpu.lease.grant", "worker-gpu", {"ttl_s": 60}, gpu_id, "gpu-stale-grant"),
    ("gpu.lease.renew", "worker-gpu", {"ttl_s": 60}, gpu_id, "gpu-stale-renew"),
    ("gpu.lease.release", "worker-gpu", {"reason": "m26e-stale"}, gpu_id, "gpu-stale-release"),
    ("peft.export", "worker-lora", {}, export_job, "peft-stale-export"),
    ("peft.import", "worker-lora", {"adapter_ref": "fixture-adapter", "job_id": export_job}, "stale-import", "peft-stale-import"),
    ("peft.activate", "worker-lora", {}, "m26e-lora", "peft-stale-activate"),
    ("peft.rollback", "worker-lora", {}, "vision-lora-edge", "peft-stale-rollback"),
]
for row in stale:
    submit(*row[:4], "expired", row[4])

expected_actions = {row[0] for row in confirmed + rejected + stale}
if len(records) != 21 or {row["action"] for row in records} != expected_actions:
    raise RuntimeError("receipt matrix is incomplete")
(boot / "receipt-matrix.json").write_text(
    json.dumps({"schema": "cohesix-qemu-receipt-matrix/v1", "records": records}, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
}

emit_host_integration() {
    local boot_dir=$1
    local uart=$2
    local cohsh=$3
    local observations="$boot_dir/host-integration-observations.json"
    python3 - "$REPO_ROOT/configs/generated/host_integration_dependency.json" \
        "$RESOLVED_MANIFEST" "$uart" "$cohsh" "$boot_dir/receipt-matrix.json" "$observations" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

graph_path, manifest_path, uart_path, cohsh_path, matrix_path, out_path = map(Path, sys.argv[1:])
graph_raw = graph_path.read_bytes()
manifest_raw = manifest_path.read_bytes()
matrix_raw = matrix_path.read_bytes()
uart_raw = uart_path.read_bytes()
cohsh_raw = cohsh_path.read_bytes()
matrix = json.loads(matrix_raw)
if matrix.get("schema") != "cohesix-qemu-receipt-matrix/v1":
    raise SystemExit("receipt matrix schema is invalid")
records = matrix.get("records")
expected = {
    "gpu.lease.grant": "worker-gpu",
    "gpu.lease.renew": "worker-gpu",
    "gpu.lease.release": "worker-gpu",
    "peft.export": "worker-lora",
    "peft.import": "worker-lora",
    "peft.activate": "worker-lora",
    "peft.rollback": "worker-lora",
}
if not isinstance(records, list) or len(records) != 21:
    raise SystemExit("receipt matrix must contain the exact 7x3 target results")
observed = {(row.get("action"), row.get("role"), row.get("outcome")) for row in records}
required = {
    (action, role, outcome)
    for action, role in expected.items()
    for outcome in ("succeeded", "failed", "expired")
}
if observed != required or len(observed) != len(records):
    raise SystemExit("receipt matrix does not contain exact unique target outcomes")
uart = uart_raw.decode("utf-8")
for literal in (
    "WORKER_TASK_RECEIPT",
    "WORKER_TASK_COMPLETION",
    "WORKER_TASK_READY role=worker-heartbeat",
    "WORKER_TASK_READY role=worker-gpu",
    "WORKER_TASK_READY role=worker-lora",
    "NINEDOOR_SERVICE_TEARDOWN",
    "CONSOLE_NETWORK_TEARDOWN",
    "GPU_BRIDGE_FIXTURE_ADMISSION",
    "LORA_EXPORT_FIXTURE_ADMISSION",
):
    if literal not in uart:
        raise SystemExit(f"target UART lacks semantic integration marker: {literal}")
cohsh = cohsh_raw.decode("utf-8")
for literal in ("OK SPAWN", "OK KILL", "worker-bus", "model-only", "mode=fixture"):
    if literal not in cohsh:
        raise SystemExit(f"cohsh transcript lacks semantic integration outcome: {literal}")
if "ERR" not in cohsh:
    raise SystemExit("cohsh transcript lacks bounded control refusal")
artifacts = {
    "gpu-receipt-path": ("confirmed-rejected-stale-worker-receipts-observed", (matrix_path, uart_path)),
    "peft-receipt-path": ("confirmed-rejected-stale-worker-receipts-observed", (matrix_path, uart_path)),
    "worker-control": ("spawn-kill-admission-ready-and-refusal-observed", (cohsh_path, uart_path)),
}
rows = []
for row_id in sorted(artifacts):
    result, paths = artifacts[row_id]
    raw_evidence = []
    for path in paths:
        raw = path.read_bytes()
        if not raw:
            raise SystemExit(f"empty host-integration artifact: {path}")
        raw_evidence.append(
            {"id": path.name, "sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}
        )
    raw_evidence.sort(key=lambda row: row["id"])
    rows.append(
        {
            "dependency_id": row_id,
            "observed_mode": "live",
            "outcomes": [{"id": f"{row_id}-live", "class": "receipt", "result": result}],
            "raw_evidence": raw_evidence,
        }
    )
payload = {
    "schema": "cohesix-host-integration-observations/v1",
    "dependency_graph_sha256": hashlib.sha256(graph_raw).hexdigest(),
    "manifest_sha256": hashlib.sha256(manifest_raw).hexdigest(),
    "observations": rows,
}
out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
    scripts/ci/host_integration_run.sh \
        --matrix configs/host_integration_acceptance.toml \
        --mode live \
        --target qemu \
        --target-session "$boot_dir/target-session.json" \
        --observations "$observations" \
        --state-dir "$boot_dir/host-integration"
}

run_critical_observation_boot() {
    local boot_dir=$1
    local critical_dir="$boot_dir/critical"
    mkdir -p "$critical_dir"
    /usr/bin/script -q -F "$critical_dir/uart.log" \
        "$BUILD_RUN" --launch-existing "${BUILD_ARGS[@]}" --raw-qemu -- \
        -pidfile "$critical_dir/qemu.pid" \
        -gdb tcp:127.0.0.1:1234 \
        -S \
        > "$critical_dir/qemu-launch.log" 2>&1 &
    QEMU_LAUNCHER_PID=$!
    wait_for_file "$critical_dir/qemu.pid" 180
    QEMU_PID="$(tr -d '[:space:]' < "$critical_dir/qemu.pid")"
    [[ "$QEMU_PID" =~ ^[0-9]+$ ]] || die "critical QEMU pidfile is malformed"
    ps -ww -p "$QEMU_PID" -o command= > "$critical_dir/qemu-command.txt"
    wait_for_port 127.0.0.1 1234 60
    verify_live_artifacts
    verify_qemu_command "$critical_dir" yes
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py qemu-critical-gdb \
        --gdb "$GDB_BIN" \
        --remote 127.0.0.1:1234 \
        --target-session "$boot_dir/target-session.json" \
        --generated-inventory "$GENERATED_INVENTORY" \
        --root-elf "$ROOT_ELF" \
        --timeout-secs 600 \
        --out "$boot_dir/critical.gdb.log" &
    GDB_RUNNER_PID=$!
    if ! wait "$GDB_RUNNER_PID"; then
        GDB_RUNNER_PID=""
        die "critical-duty GDB observation failed"
    fi
    GDB_RUNNER_PID=""
    stop_pid "$QEMU_PID"; QEMU_PID=""
    stop_pid "$QEMU_LAUNCHER_PID"; QEMU_LAUNCHER_PID=""
    verify_live_artifacts
}

run_pressure_boot() {
    local label=$1
    local intensity=$2
    local base_rps=$3
    local max_inflight=$4
    local seed=$5
    local boot_dir="$RUN_DIR/$label"
    mkdir -p "$boot_dir"
    cp "$TARGET_SESSION" "$boot_dir/target-session.json"
    prepare_host_fixture "$boot_dir"
    run_critical_observation_boot "$boot_dir"

    /usr/bin/script -q -F "$boot_dir/uart.live.log" \
        "$BUILD_RUN" --launch-existing "${BUILD_ARGS[@]}" --raw-qemu -- \
        -pidfile "$boot_dir/qemu.pid" \
        -gdb tcp:127.0.0.1:1234 \
        > "$boot_dir/qemu-launch.log" 2>&1 &
    QEMU_LAUNCHER_PID=$!
    wait_for_file "$boot_dir/qemu.pid" 180
    QEMU_PID="$(tr -d '[:space:]' < "$boot_dir/qemu.pid")"
    [[ "$QEMU_PID" =~ ^[0-9]+$ ]] || die "QEMU pidfile is malformed"
    ps -ww -p "$QEMU_PID" -o command= > "$boot_dir/qemu-command.txt"
    verify_qemu_command "$boot_dir" no
    wait_for_port 127.0.0.1 31337 180
    verify_live_artifacts
    start_gateway "$boot_dir"
    assert_gateway_unproven
    publish_gpu_fixture "$boot_dir"

    # Three role-specific GDB plans use only the qemu-evidence symbols and the
    # existing spawn/fault/recreate lifecycle.
    drive_worker_fault_plan "$boot_dir" worker-heartbeat 100
    drive_worker_fault_plan "$boot_dir" worker-gpu 200
    drive_worker_fault_plan "$boot_dir" worker-lora 300

    # Service containment uses external QEMU-only hooks and existing requests;
    # no operator fault route or target grammar is introduced.
    drive_service_fault_plan \
        "$boot_dir" ninedoor-service "$NINEDOOR_ELF" \
        NINEDOOR_SERVICE_TEARDOWN 350
    drive_service_fault_plan \
        "$boot_dir" console-network "$CONSOLE_NETWORK_ELF" \
        CONSOLE_NETWORK_TEARDOWN 360

    drive_receipt_matrix "$boot_dir"
    publish_gpu_fixture "$boot_dir"
    drive_operator_lifecycle "$boot_dir"
    freeze_prefix "$boot_dir/uart.live.log" "$boot_dir/preflight.uart.log"
    cp "$boot_dir/cohsh.log" "$boot_dir/preflight.cohsh.log"
    emit_host_integration "$boot_dir" "$boot_dir/preflight.uart.log" "$boot_dir/preflight.cohsh.log"

    mkdir -p "$boot_dir/preflight"
    "$HARNESS_PYTHON" scripts/worker_task_evidence.py collect-qemu-preflight \
        --target-session "$boot_dir/target-session.json" \
        --generated-inventory "$GENERATED_INVENTORY" \
        --uart "$boot_dir/preflight.uart.log" \
        --cohsh "$boot_dir/preflight.cohsh.log" \
        --gdb-log "$boot_dir/worker-heartbeat.gdb.log" \
        --gdb-log "$boot_dir/worker-gpu.gdb.log" \
        --gdb-log "$boot_dir/worker-lora.gdb.log" \
        --worker-archive "$WORKER_ARCHIVE" \
        --driver-archive "$DRIVER_ARCHIVE" \
        --worker-image-manifest "$WORKER_MANIFEST" \
        --worker-elf "worker-heartbeat=$WORKER_HEART_ELF" \
        --worker-elf "worker-gpu=$WORKER_GPU_ELF" \
        --worker-elf "worker-lora=$WORKER_LORA_ELF" \
        --service-elf "ninedoor-service=$NINEDOOR_ELF" \
        --service-elf "console-network=$CONSOLE_NETWORK_ELF" \
        --service-gdb-log "$boot_dir/ninedoor-service.gdb.log" \
        --service-gdb-log "$boot_dir/console-network.gdb.log" \
        --root-elf "$ROOT_ELF" \
        --critical-gdb-log "$boot_dir/critical.gdb.log" \
        --integration-dir "$boot_dir/host-integration/integration" \
        --out-dir "$boot_dir/preflight"

    start_gateway \
        "$boot_dir" \
        "$boot_dir/preflight/worker-task-evidence.json" \
        "$boot_dir/target-session.json"
    publish_gpu_fixture "$boot_dir"
    start_pressure_helpers "$boot_dir"

    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="$M26E_REST_AUTH_TOKEN" \
    HIVE_GATEWAY_URL=http://127.0.0.1:8080 \
    "$HARNESS_PYTHON" scripts/rest_perf_harness.py \
        --mode simulate \
        --population-mode executable \
        --no-qemu \
        --no-gateway \
        --rest-url http://127.0.0.1:8080 \
        --qemu-uart-log "$boot_dir/uart.live.log" \
        --qemu-gdb-log "$boot_dir/worker-heartbeat.gdb.log" \
        --workers-min 3 \
        --workers-max 3 \
        --intensity-min "$intensity" \
        --intensity-max "$intensity" \
        --duration-mins 2 \
        --base-rps "$base_rps" \
        --max-inflight "$max_inflight" \
        --seed "$seed" \
        --no-transient-retries \
        --strict-control-errors \
        --error-budget-rate 0.01 \
        --log-dir "$boot_dir/bench" \
        --log-prefix "m26e-executable-$label"

    stop_pressure_helpers
    local summary
    summary="$(python3 - "$boot_dir/bench" "m26e-executable-$label" <<'PY'
from pathlib import Path
import stat
import sys

directory = Path(sys.argv[1])
prefix = sys.argv[2]
candidates = sorted(directory.glob(f"{prefix}_*.summary.json"))
if len(candidates) != 1:
    raise SystemExit(f"expected one timestamped pressure summary, got {len(candidates)}")
info = candidates[0].lstat()
if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode) or info.st_size == 0:
    raise SystemExit("pressure summary is empty or aliased")
print(candidates[0])
PY
)"
    cp "$summary" "$boot_dir/pressure.summary.json"
    cmp -s "$summary" "$boot_dir/pressure.summary.json" || \
        die "canonical pressure summary copy differs from immutable harness output"
    summary="$boot_dir/pressure.summary.json"
    python3 - "$summary" "$boot_dir/uart.live.log" "$boot_dir/pressure.uart.log" \
        "$boot_dir/worker-heartbeat.gdb.log" "$boot_dir/pressure.gdb.log" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

summary_path, uart_live, uart_out, gdb_live, gdb_out = map(Path, sys.argv[1:])
summary = json.loads(summary_path.read_text(encoding="utf-8"))
faults = summary["report"]["executable_state"]["fault_artifacts"]
for name, source, destination in (
    ("uart", uart_live, uart_out),
    ("gdb", gdb_live, gdb_out),
):
    expected = faults[name]
    raw = source.read_bytes()
    length = expected["bytes"]
    if not isinstance(length, int) or length <= 0 or len(raw) < length:
        raise SystemExit(f"{name} artifact length is invalid")
    frozen = raw[:length]
    if hashlib.sha256(frozen).hexdigest() != expected["sha256"]:
        raise SystemExit(f"{name} artifact changed before immutable prefix capture")
    destination.write_bytes(frozen)
PY

    verify_live_artifacts
    stop_pid "$GATEWAY_PID"; GATEWAY_PID=""
    stop_pid "$QEMU_PID"; QEMU_PID=""
    stop_pid "$QEMU_LAUNCHER_PID"; QEMU_LAUNCHER_PID=""
}

run_pressure_boot medium 4 1 16 2604
run_pressure_boot high 8 4 32 2608

FINAL_DIR="$RUN_DIR/final"
mkdir -p "$FINAL_DIR"
verify_live_artifacts
"$HARNESS_PYTHON" scripts/worker_task_evidence.py collect-qemu \
    --target-session "$TARGET_SESSION" \
    --generated-inventory "$GENERATED_INVENTORY" \
    --preflight-uart "$RUN_DIR/medium/preflight.uart.log" \
    --preflight-gdb-log "$RUN_DIR/medium/worker-heartbeat.gdb.log" \
    --preflight-gdb-log "$RUN_DIR/medium/worker-gpu.gdb.log" \
    --preflight-gdb-log "$RUN_DIR/medium/worker-lora.gdb.log" \
    --preflight-service-gdb-log "$RUN_DIR/medium/ninedoor-service.gdb.log" \
    --preflight-service-gdb-log "$RUN_DIR/medium/console-network.gdb.log" \
    --preflight-critical-gdb-log "$RUN_DIR/medium/critical.gdb.log" \
    --uart "$RUN_DIR/medium/pressure.uart.log" \
    --gdb-log "$RUN_DIR/medium/pressure.gdb.log" \
    --pressure "$RUN_DIR/medium/pressure.summary.json" \
    --uart "$RUN_DIR/high/pressure.uart.log" \
    --gdb-log "$RUN_DIR/high/pressure.gdb.log" \
    --pressure "$RUN_DIR/high/pressure.summary.json" \
    --cohsh "$RUN_DIR/high/preflight.cohsh.log" \
    --worker-elf "worker-heartbeat=$WORKER_HEART_ELF" \
    --worker-elf "worker-gpu=$WORKER_GPU_ELF" \
    --worker-elf "worker-lora=$WORKER_LORA_ELF" \
    --service-elf "ninedoor-service=$NINEDOOR_ELF" \
    --service-elf "console-network=$CONSOLE_NETWORK_ELF" \
    --root-elf "$ROOT_ELF" \
    --worker-archive "$WORKER_ARCHIVE" \
    --driver-archive "$DRIVER_ARCHIVE" \
    --worker-image-manifest "$WORKER_MANIFEST" \
    --integration-dir "$RUN_DIR/high/host-integration/integration" \
    --run-dir "$TEST_PLAN_STATE_DIR" \
    --out-dir "$FINAL_DIR"

M26E_SCAN_CONSOLE_TOKEN="$M26E_CONSOLE_AUTH_TOKEN" \
M26E_SCAN_REST_TOKEN="$M26E_REST_AUTH_TOKEN" \
python3 - "$RUN_DIR" <<'PY'
import os
from pathlib import Path
import stat
import sys

root = Path(sys.argv[1])
secrets = (
    os.environ["M26E_SCAN_CONSOLE_TOKEN"].encode(),
    os.environ["M26E_SCAN_REST_TOKEN"].encode(),
)
for directory, names, files in os.walk(root, followlinks=False):
    for name in [*names, *files]:
        path = Path(directory) / name
        info = path.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise SystemExit(f"retained evidence contains an unexpected symlink: {path}")
        if stat.S_ISREG(info.st_mode):
            raw = path.read_bytes()
            if any(secret in raw for secret in secrets):
                raise SystemExit(f"retained evidence contains authentication secret bytes: {path}")
PY
unset M26E_CONSOLE_AUTH_TOKEN M26E_REST_AUTH_TOKEN

log "PASS: immutable medium/high QEMU evidence collected under $RUN_DIR"
