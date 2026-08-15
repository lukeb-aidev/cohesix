#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh target pack with bounded QEMU proofs and exact generated-output restoration.
# Copyright 2026 Lukas Bower

# Note: select a target with COHSH_BATCH_TARGET=qemu|pi4; qemu is the default.
# Note: override auth/readiness bounds via env vars, e.g. COHSH_AUTH_TOKEN=... READY_TIMEOUT=300 PORT_TIMEOUT=60 scripts/cohsh/run_regression_batch.sh
# Note: live Pi runs use COHSH_BATCH_TARGET=pi4 COHSH_TCP_HOST=<pi-ip> COHSH_TCP_PORT=31337.
# Note: archive root defaults to out/regression-logs; override via COHSH_LOG_ROOT=/path/to/logs.
# Note: set COHSH_BATCH_CLEAN_TARGET=1 for a forced clean rebuild before batch execution.
# Note: set COHSH_BATCH_GROUPS=base,base-telemetry,base-shard,gated to run a subset.
# ** Note: typical end-to-end runtime is ~25 minutes; plan for >= 30 minutes to avoid repeated retries.
#
# Operator note for live Pi final reboot proof:
# The Pi 4 batch uses TCP while Cohesix is running, but the final reboot/fresh-boot
# proof crosses a reset and the U-Boot menu. Use the active minicom UART capture
# for that section instead of trying to drive the Terminal UI. macOS may deny
# scripted Terminal keystrokes; direct writes to /dev/cu.usbserial-* still reach
# the same UART and minicom continues to capture the evidence.
#
# Repeatable flow:
#   1. Identify the active minicom session and capture log:
#        ps -ax -o pid=,tty=,command= | rg '[m]inicom'
#        lsof -p <minicom-pid> | rg 'pi4-serial|usbserial'
#      Use the lsof output as the source of truth for both the serial device and
#      the log file; for example:
#        serial_dev=/dev/cu.usbserial-0001
#        capture_log=/Users/lukasbower/pi4-serial-YYYYMMDD-HHMMSS.log
#      Ignore stale or zero-byte ~/pi4-serial-*.log files.
#   2. Prove the serial injection lane before resetting:
#        python3 -c 'import os, sys; fd=os.open(sys.argv[1], os.O_WRONLY | os.O_NOCTTY); os.write(fd, b"smp activity\r"); os.close(fd)' "$serial_dev"
#      Then grep the same capture log for a new "smp activity" block with
#      "OK SMP mode=activity" and, for Genet, "backend=bcmgenet-v5" plus the
#      expected DHCP lease.
#   3. Request reboot over authenticated cohsh while TCP is still up:
#        tmp_script="$(mktemp /tmp/coh-reboot.XXXXXX.coh)"
#        printf 'reboot\n' > "$tmp_script"
#        "${COHSH_BIN:-target/debug/cohsh}" --transport tcp --tcp-host "$COHSH_TCP_HOST" --tcp-port "${COHSH_TCP_PORT:-31337}" --auth-token "$COHSH_AUTH_TOKEN" --role queen --script "$tmp_script"
#        rc=$?; rm -f "$tmp_script"; test "$rc" -eq 0
#      Expected console evidence is "OK REBOOT detail=scheduled" followed by a
#      reset in the minicom log.
#   4. When U-Boot reaches the menu, press Enter over the UART device:
#        python3 -c 'import os, sys; fd=os.open(sys.argv[1], os.O_WRONLY | os.O_NOCTTY); os.write(fd, b"\r"); os.close(fd)' "$serial_dev"
#      If U-Boot reports "boot marker diagnostics", that is expected; pressing
#      Enter should continue the normal boot from the interactive menu.
#   5. Wait for fresh Cohesix proof in the same capture log: DHCP bound,
#      root prompt, then a new clean "smp activity" block. If USB/local-seat
#      boot chatter interleaves with a partial command, send a blank line,
#      retry "smp activity" after owner-state ready, and only count the full
#      block ending in "OK SMP mode=activity".
#   6. For scripts that require clean boot-local state, run one selected group
#      per fresh boot, for example:
#        COHSH_BATCH_TARGET=pi4 COHSH_BATCH_GROUPS=base ...
#        <reboot using the UART flow above>
#        COHSH_BATCH_TARGET=pi4 COHSH_BATCH_GROUPS=base-telemetry ...
#      The default remains the full sequence for QEMU parity and quick live
#      smoke runs; selected groups make the fresh-boot proof repeatable.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GENERATED_OUTPUT_LOCK_FILE="${PROJECT_ROOT}/out/.cohesix-locks/generated-outputs.lock"
# shellcheck source=scripts/ci/test_plan_resources.sh
source "${PROJECT_ROOT}/scripts/ci/test_plan_resources.sh"
tp_configure_resource_limits
GENERATED_CONFIG_DIR="$PROJECT_ROOT/configs/generated"
QEMU_ARTIFACT_HELPER="$PROJECT_ROOT/scripts/ci/qemu_artifact.py"
TEST_PLAN_CATALOG="$PROJECT_ROOT/scripts/ci/test_plan_catalog.py"
BUILD_RUN_BIN="${COHESIX_BUILD_RUN_BIN:-$PROJECT_ROOT/scripts/cohesix-build-run.sh}"
QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
QEMU_RESPONSE_MATRIX_SCRIPT="$PROJECT_ROOT/scripts/qemu_tcp_response_matrix.py"
QEMU_RESPONSE_MATRIX_FIXED_LABEL="qemu_tcp_response_matrix.fixed"
cd "$PROJECT_ROOT"

canonical_path() {
    python3 - "$1" <<'PY'
from pathlib import Path
import sys

print(Path(sys.argv[1]).resolve(strict=False))
PY
}

validate_output_roots() {
    python3 - \
        "$PROJECT_ROOT" \
        "$ARCHIVE_ROOT" \
        "$QEMU_ARTIFACT_ROOT" \
        "$TRANSPORT_RESULT_ROOT" \
        "$TRANSPORT_EVIDENCE_ROOT" \
        "$GENERATED_OUTPUT_LOCK_FILE" <<'PY'
from pathlib import Path
import tempfile
import sys

repo, archive, artifact, result, evidence, generated_lock = (
    Path(value).resolve() for value in sys.argv[1:]
)
repo_out = (repo / "out").resolve()
temp_root = Path(tempfile.gettempdir()).resolve()
home = Path.home().resolve()


def within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True


for label, path in (
    ("archive", archive),
    ("artifact", artifact),
    ("result", result),
    ("evidence", evidence),
):
    if path in {Path("/"), repo, repo_out, home, temp_root}:
        raise SystemExit(f"unsafe {label} root: {path}")
    if not within(path, repo_out) and not within(path, temp_root):
        raise SystemExit(
            f"{label} root must be below {repo_out} or temporary root "
            f"{temp_root}: {path}"
        )

if artifact == archive or result == archive:
    raise SystemExit("artifact/result roots must not alias the archive root")
if within(archive, artifact) or within(archive, result):
    raise SystemExit("archive root must not be nested below artifact/result roots")
if artifact == result or within(artifact, result) or within(result, artifact):
    raise SystemExit("artifact and result roots must not alias or overlap")
for label, path in (
    ("archive", archive),
    ("artifact", artifact),
    ("result", result),
):
    if not within(path, evidence):
        raise SystemExit(f"{label} root escapes transport evidence root: {path}")
    if within(generated_lock, path):
        raise SystemExit(
            f"{label} root contains the generated-output lock: {path}"
        )
PY
}

BASE_SCRIPTS=(
    "boot_v0.coh"
    "9p_batch.coh"
    "host_absent.coh"
    "observe_watch.coh"
    "root_cut_basic.coh"
    "session_lifecycle.coh"
    "busy_backpressure.coh"
    "cas_fixture_signature_rejected.coh"
    "tcp_basic.coh"
    "session_pool.coh"
)

BASE_TELEMETRY_SCRIPTS=(
    "telemetry_ring.coh"
    "telemetry_push_create.coh"
)

# The two-worker shard regression occupies the distinct heartbeat and LoRA
# executable-role slots; the selected topology admits one live slot per role.
BASE_SHARD_SCRIPTS=(
    "shard_1k.coh"
)

# Policy/audit coverage uses only session-local Queen controls. These scripts
# must not depend on a host provider having populated /host.
GATED_SCRIPTS=(
    "replay_journal.coh"
    "policy_gate.coh"
    "model_cas_bind.coh"
    "sidecar_integration.coh"
)

# These scripts consume repository-only fixture trust material and therefore
# run only against the QEMU gated manifest, never an operational base or Pi.
QEMU_GATED_FIXTURE_SCRIPTS=(
    "cas_roundtrip.coh"
)

BASE_MANIFEST="${PROJECT_ROOT}/configs/root_task.toml"
GATED_MANIFEST="${PROJECT_ROOT}/configs/root_task_regression.toml"
READY_MARKER="[mark] root-console.start.ok"
READY_TIMEOUT="${READY_TIMEOUT:-180}"
PORT_TIMEOUT="${PORT_TIMEOUT:-30}"
AUTH_READY_TIMEOUT="${AUTH_READY_TIMEOUT:-60}"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${SEL4_BUILD:-${PROJECT_ROOT}/out/sel4/profile-v2/qemu-smp-production}}"
BATCH_TARGET="${COHSH_BATCH_TARGET:-qemu}"
case "$BATCH_TARGET" in
    qemu)
        ;;
    pi|pi4|live|external)
        BATCH_TARGET="pi4"
        ;;
    *)
        echo "Unknown COHSH_BATCH_TARGET=${BATCH_TARGET}; expected qemu or pi4" >&2
        exit 2
        ;;
esac
if [[ "$BATCH_TARGET" == "qemu" ]]; then
    TEST_ACTION_ID="${TEST_PLAN_ACTION_ID:-qemu.tcp-regression}"
    TEST_CLAIM_TIER="qemu-integration"
else
    TEST_ACTION_ID="${TEST_PLAN_ACTION_ID:-pi4.tcp-regression}"
    TEST_CLAIM_TIER="pi4-transport"
fi
if [[ ! -x "$QEMU_ARTIFACT_HELPER" ]]; then
    echo "Missing QEMU artifact helper: $QEMU_ARTIFACT_HELPER" >&2
    exit 1
fi
if [[ ! -x "$TEST_PLAN_CATALOG" ]]; then
    echo "Missing test-plan action catalog helper: $TEST_PLAN_CATALOG" >&2
    exit 1
fi
if [[ ! -x "$BUILD_RUN_BIN" ]]; then
    echo "Missing Cohesix build wrapper: $BUILD_RUN_BIN" >&2
    exit 1
fi
TEST_ACTION_DIGEST="sha256:$(
    "$TEST_PLAN_CATALOG" action --id "$TEST_ACTION_ID" --field digest
)"
TEST_SOURCE_DIGEST="${TEST_PLAN_SOURCE_DIGEST:-$(
    "$QEMU_ARTIFACT_HELPER" source-digest --repo-root "$PROJECT_ROOT"
)}"
if [[ "$TEST_SOURCE_DIGEST" != sha256:* ]]; then
    TEST_SOURCE_DIGEST="sha256:${TEST_SOURCE_DIGEST}"
fi
TEST_ATTEMPT_MANIFEST="${TEST_PLAN_ATTEMPT_MANIFEST:-}"
QEMU_ARTIFACT_ROOT="${COHSH_QEMU_ARTIFACT_ROOT:-}"
TRANSPORT_RESULT_ROOT="${COHSH_TRANSPORT_RESULT_ROOT:-}"
REQUIRE_RESULT_EVIDENCE="${COHSH_REQUIRE_RESULT_EVIDENCE:-0}"
TARGET_EVIDENCE_FILE="${TEST_PLAN_TARGET_EVIDENCE_FILE:-${PI4_TARGET_EVIDENCE_FILE:-}}"
RUN_ID="${COHSH_BATCH_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ "$BATCH_TARGET" == "pi4" && -z "${COHSH_LOG_ROOT:-}" ]]; then
    ARCHIVE_ROOT="${PROJECT_ROOT}/out/regression-logs/pi4-full-${RUN_ID}"
else
    ARCHIVE_ROOT="${COHSH_LOG_ROOT:-${PROJECT_ROOT}/out/regression-logs}"
fi
ARCHIVE_ROOT="$(canonical_path "$ARCHIVE_ROOT")"
if [[ -z "$QEMU_ARTIFACT_ROOT" ]]; then
    QEMU_ARTIFACT_ROOT="${ARCHIVE_ROOT}/qemu-artifacts"
fi
QEMU_ARTIFACT_ROOT="$(canonical_path "$QEMU_ARTIFACT_ROOT")"
if [[ -z "$TRANSPORT_RESULT_ROOT" ]]; then
    TRANSPORT_RESULT_ROOT="${ARCHIVE_ROOT}/transport-results"
fi
TRANSPORT_RESULT_ROOT="$(canonical_path "$TRANSPORT_RESULT_ROOT")"
TRANSPORT_EVIDENCE_ROOT="$(
    python3 - \
        "$ARCHIVE_ROOT" \
        "$QEMU_ARTIFACT_ROOT" \
        "$TRANSPORT_RESULT_ROOT" <<'PY'
from pathlib import Path
import os
import sys

print(Path(os.path.commonpath(sys.argv[1:])).resolve())
PY
)"
validate_output_roots
case "${COHSH_BATCH_PRINT_PATHS:-0}" in
    0)
        ;;
    1)
        printf 'ARCHIVE_ROOT=%s\n' "$ARCHIVE_ROOT"
        printf 'QEMU_ARTIFACT_ROOT=%s\n' "$QEMU_ARTIFACT_ROOT"
        printf 'TRANSPORT_RESULT_ROOT=%s\n' "$TRANSPORT_RESULT_ROOT"
        printf 'TRANSPORT_EVIDENCE_ROOT=%s\n' "$TRANSPORT_EVIDENCE_ROOT"
        exit 0
        ;;
    *)
        echo "COHSH_BATCH_PRINT_PATHS must be 0 or 1" >&2
        exit 2
        ;;
esac
TCP_HOST="${COHSH_TCP_HOST:-${COHSH_HOST:-127.0.0.1}}"
TCP_PORT="${COHSH_TCP_PORT:-${COHSH_PORT:-31337}}"
QEMU_TCP_HOST="127.0.0.1"
QEMU_TCP_PORT="${COHSH_QEMU_TCP_PORT:-}"
QEMU_UDP_PORT="${COHSH_QEMU_UDP_PORT:-}"
QEMU_SMOKE_PORT="${COHSH_QEMU_SMOKE_PORT:-}"
COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"
if [[ "$BATCH_TARGET" == "pi4" ]]; then
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-1}"
else
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-0}"
fi

GENERATED_OUTPUT_PATHS=(
    "apps/root-task/src/generated"
    "configs/generated/root_task_resolved.json"
    "configs/generated/root_task_resolved.json.sha256"
    "configs/generated/root_task_topology.json"
    "configs/generated/cohesix_python_qemu_smp_production.json"
    "configs/generated/cohesix_python_pi4_production.json"
    "configs/generated/implementation_surface_inventory.json"
    "configs/generated/host_integration_dependency.json"
    "configs/generated/cas_manifest_template.json"
    "configs/generated/cas_manifest_template.json.sha256"
    "scripts/cohsh/boot_v0.coh"
    "docs/snippets/root_task_manifest.md"
    "docs/snippets/gpu_breadcrumbs.md"
    "docs/snippets/observability_interfaces.md"
    "docs/snippets/observability_security.md"
    "docs/snippets/ticket_quotas.md"
    "docs/snippets/trace_policy.md"
    "docs/snippets/cas_interfaces.md"
    "docs/snippets/cas_security.md"
    "docs/snippets/telemetry_cbor_schema.md"
    "tools/cohesix-py/cohesix/generated.py"
    "docs/snippets/cohesix_py_defaults.md"
    "docs/snippets/coh_doctor_checks.md"
    "docs/snippets/host_integration_dependency.md"
    "apps/cohsh/src/generated/policy.rs"
    "docs/snippets/cohsh_policy.md"
    "apps/cohsh/src/generated/client.rs"
    "docs/snippets/cohsh_client.md"
    "docs/snippets/cohsh_grammar.md"
    "docs/snippets/cohsh_ticket_policy.md"
    "apps/coh/src/generated/policy.rs"
    "docs/snippets/coh_policy.md"
    "apps/swarmui/src/generated.rs"
    "docs/snippets/swarmui_defaults.md"
    "configs/generated/cohsh_policy.toml"
    "configs/generated/cohsh_policy.toml.sha256"
    "configs/generated/coh_policy.toml"
    "configs/generated/coh_policy.toml.sha256"
    "configs/generated/swarmui_defaults.toml"
    "configs/generated/swarmui_defaults.toml.sha256"
)
generated_snapshot_dir=""
generated_snapshot_parent=""
generated_snapshot_ready=0
generated_output_lock_held=0
# Bash 3.2 treats an expanded declared-but-empty array as unset under nounset.
# Keep an inert sentinel so recovery cleanup remains portable to the macOS Bash.
generated_preserved_restore_work_dirs=("")

acquire_generated_output_lock() {
    if [[ "$generated_output_lock_held" == "1" ]]; then
        return 0
    fi
    local lock_parent
    lock_parent="$(dirname "$GENERATED_OUTPUT_LOCK_FILE")"
    mkdir -p "$lock_parent"
    if ! exec 9<>"$GENERATED_OUTPUT_LOCK_FILE"; then
        echo "Failed to open generated-output lock: $GENERATED_OUTPUT_LOCK_FILE" >&2
        return 1
    fi
    if ! python3 - 9 "${COHSH_GENERATED_LOCK_TIMEOUT:-900}" "$$" <<'PY'
import errno
import fcntl
import math
import os
import sys
import time

descriptor = int(sys.argv[1])
try:
    timeout = float(sys.argv[2])
except ValueError:
    raise SystemExit("COHSH_GENERATED_LOCK_TIMEOUT must be a number")
if not math.isfinite(timeout) or timeout < 0:
    raise SystemExit("COHSH_GENERATED_LOCK_TIMEOUT must be finite and non-negative")

deadline = time.monotonic() + timeout
while True:
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        break
    except OSError as error:
        if error.errno not in (errno.EACCES, errno.EAGAIN):
            raise
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            os.lseek(descriptor, 0, os.SEEK_SET)
            owner = os.read(descriptor, 4096).decode("utf-8", errors="replace").strip()
            detail = owner or "owner metadata unavailable"
            raise SystemExit(f"generated-output lock is busy: {detail}")
        time.sleep(min(0.05, remaining))

owner = f"pid={sys.argv[3]}\n"
os.ftruncate(descriptor, 0)
os.lseek(descriptor, 0, os.SEEK_SET)
os.write(descriptor, owner.encode("utf-8"))
os.fsync(descriptor)
PY
    then
        exec 9>&-
        return 1
    fi
    # The Python child and this shell share one inherited open-file
    # description, so the kernel lock remains live until descriptor 9 closes.
    # A killed owner cannot leave a stale lock; any old text is diagnostic only.
    generated_output_lock_held=1
}

release_generated_output_lock() {
    if [[ "$generated_output_lock_held" != "1" ]]; then
        return 0
    fi
    # Mark inactive first so an EXIT re-entry cannot close a subsequently
    # acquired descriptor. Closing the descriptor releases the kernel lock.
    generated_output_lock_held=0
    exec 9>&-
}

discard_generated_snapshot_dir() {
    local snapshot_dir="$1"
    local snapshot_parent="$2"
    case "$snapshot_dir" in
        "${snapshot_parent}/cohesix-generated."*)
            ;;
        *)
            echo "Refusing to discard generated snapshot directory: $snapshot_dir" >&2
            return 1
            ;;
    esac
    if [[ "$(dirname "$snapshot_dir")" != "$snapshot_parent" \
        || ! -d "$snapshot_dir" \
        || -L "$snapshot_dir" ]]; then
        echo "Invalid generated snapshot directory: $snapshot_dir" >&2
        return 1
    fi
    find "$snapshot_dir" -mindepth 1 -delete
    rmdir "$snapshot_dir"
}

validate_generated_snapshot_metadata() {
    local snapshot_dir="$1"
    local snapshot_parent="$2"
    if [[ -z "$snapshot_dir" \
        || -z "$snapshot_parent" \
        || "$(dirname "$snapshot_dir")" != "$snapshot_parent" \
        || ! -d "$snapshot_dir" \
        || -L "$snapshot_dir" ]]; then
        echo "Generated snapshot directory metadata is invalid: $snapshot_dir" >&2
        return 1
    fi
    python3 - "$snapshot_dir" "${GENERATED_OUTPUT_PATHS[@]}" <<'PY'
from pathlib import Path, PurePosixPath
import os
import sys

snapshot = Path(sys.argv[1])
expected = sys.argv[2:]


def fail(message: str) -> None:
    raise SystemExit(f"invalid generated snapshot metadata: {message}")


if not expected or len(expected) != len(set(expected)):
    fail("generated-output inventory is empty or duplicated")
for value in expected:
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(
        part in {"", ".", ".."} for part in path.parts
    ):
        fail(f"unsafe generated-output path {value!r}")

allowed_root_entries = {"complete", "files", "missing", "present"}
actual_root_entries = {path.name for path in snapshot.iterdir()}
if not actual_root_entries.issubset(allowed_root_entries):
    fail("snapshot contains unexpected root entries")

metadata: dict[str, list[str]] = {}
for name in ("present", "missing"):
    path = snapshot / name
    if not path.is_file() or path.is_symlink():
        fail(f"{name} is not a regular metadata file")
    try:
        values = path.read_text(encoding="utf-8").splitlines()
    except UnicodeError as error:
        fail(f"{name} is not UTF-8: {error}")
    if any(not value for value in values) or len(values) != len(set(values)):
        fail(f"{name} contains an empty or duplicate entry")
    metadata[name] = values

complete = snapshot / "complete"
if not complete.is_file() or complete.is_symlink():
    fail("complete marker is missing or not regular")
expected_marker = f"cohesix-generated-snapshot-v1 count={len(expected)}\n"
if complete.read_text(encoding="utf-8") != expected_marker:
    fail("complete marker does not bind the inventory size")

present = metadata["present"]
missing = metadata["missing"]
if set(present).intersection(missing):
    fail("present and missing inventories overlap")
if set(present).union(missing) != set(expected):
    fail("present and missing inventories do not exactly partition outputs")
positions = {value: index for index, value in enumerate(expected)}
if present != sorted(present, key=positions.__getitem__):
    fail("present inventory is out of canonical order")
if missing != sorted(missing, key=positions.__getitem__):
    fail("missing inventory is out of canonical order")

files_root = snapshot / "files"
if present and (not files_root.is_dir() or files_root.is_symlink()):
    fail("files tree is missing or not a directory")
for value in present:
    if not os.path.lexists(files_root / value):
        fail(f"present snapshot payload is missing: {value}")
for value in missing:
    if os.path.lexists(files_root / value):
        fail(f"missing output has a snapshot payload: {value}")

allowed_paths = [PurePosixPath(value) for value in present]
if files_root.is_symlink() or (files_root.exists() and not files_root.is_dir()):
    fail("files tree has an invalid type")
if files_root.exists():
    for current, directories, filenames in os.walk(files_root, followlinks=False):
        current_path = Path(current)
        for name in [*directories, *filenames]:
            relative = PurePosixPath(
                (current_path / name).relative_to(files_root).as_posix()
            )
            if not any(
                relative == allowed
                or relative in allowed.parents
                or allowed in relative.parents
                for allowed in allowed_paths
            ):
                fail(f"unbound snapshot payload: {relative}")
PY
}

discard_generated_restore_work_dir() {
    local work_dir="$1"
    case "$work_dir" in
        "${PROJECT_ROOT}/"*/.cohesix-restore.*)
            ;;
        *)
            echo "Refusing to discard generated restore work directory: $work_dir" >&2
            return 1
            ;;
    esac
    if [[ ! -e "$work_dir" && ! -L "$work_dir" ]]; then
        return 0
    fi
    if [[ -d "$work_dir" && ! -L "$work_dir" ]]; then
        find "$work_dir" -mindepth 1 -delete
        rmdir "$work_dir"
    else
        echo "Invalid generated restore work directory: $work_dir" >&2
        return 1
    fi
}

discard_preserved_generated_restore_work_dirs() {
    local work_dir
    for work_dir in "${generated_preserved_restore_work_dirs[@]}"; do
        if [[ -z "$work_dir" ]]; then
            continue
        fi
        discard_generated_restore_work_dir "$work_dir" || return 1
    done
    generated_preserved_restore_work_dirs=("")
}

restore_present_generated_path() {
    local relative="$1"
    local source="${generated_snapshot_dir}/files/${relative}"
    local target="${PROJECT_ROOT}/${relative}"
    local target_parent
    local target_name
    local work_dir
    local replacement
    local previous
    target_parent="$(dirname "$target")"
    target_name="$(basename "$target")"
    mkdir -p "$target_parent"
    work_dir="$(mktemp -d "${target_parent}/.cohesix-restore.${target_name}.XXXXXX")"
    replacement="${work_dir}/replacement"
    previous="${work_dir}/previous"

    # Materialize and verify the complete replacement before moving the live
    # path. A failed or interrupted copy therefore cannot clear a generated
    # directory such as apps/root-task/src/generated.
    if ! cp -pR "$source" "$replacement"; then
        discard_generated_restore_work_dir "$work_dir" || true
        return 1
    fi
    if ! diff -qr "$source" "$replacement" >/dev/null; then
        echo "Failed to stage generated output exactly: ${relative}" >&2
        discard_generated_restore_work_dir "$work_dir" || true
        return 1
    fi

    if [[ -e "$target" || -L "$target" ]]; then
        if ! mv "$target" "$previous"; then
            discard_generated_restore_work_dir "$work_dir" || true
            return 1
        fi
    fi
    if ! mv "$replacement" "$target"; then
        if [[ -e "$previous" || -L "$previous" ]]; then
            if ! mv "$previous" "$target"; then
                generated_preserved_restore_work_dirs+=("$work_dir")
                echo \
                    "Failed to roll back generated output; previous retained at ${previous}: ${relative}" \
                    >&2
                return 1
            fi
        fi
        discard_generated_restore_work_dir "$work_dir" || true
        return 1
    fi

    discard_generated_restore_work_dir "$work_dir"
}

restore_missing_generated_path() {
    local relative="$1"
    local target="${PROJECT_ROOT}/${relative}"
    local target_parent
    local target_name
    local work_dir
    local previous
    if [[ ! -e "$target" && ! -L "$target" ]]; then
        return 0
    fi
    target_parent="$(dirname "$target")"
    target_name="$(basename "$target")"
    work_dir="$(mktemp -d "${target_parent}/.cohesix-restore.${target_name}.XXXXXX")"
    previous="${work_dir}/previous"

    # Moving a path that was absent at snapshot time reaches the desired
    # repository state before recursive cleanup begins.
    if ! mv "$target" "$previous"; then
        discard_generated_restore_work_dir "$work_dir" || true
        return 1
    fi
    discard_generated_restore_work_dir "$work_dir"
}

snapshot_generated_outputs() {
    if [[ "$generated_snapshot_ready" == "1" ]]; then
        if [[ "$generated_output_lock_held" != "1" ]]; then
            echo "Generated snapshot is active without its exclusion lock" >&2
            return 1
        fi
        return 0
    fi
    acquire_generated_output_lock || return 1
    local requested_parent="${TMPDIR:-/tmp}"
    if ! mkdir -p "$requested_parent"; then
        release_generated_output_lock || true
        return 1
    fi
    if ! generated_snapshot_parent="$(cd "$requested_parent" && pwd -P)"; then
        generated_snapshot_parent=""
        release_generated_output_lock || true
        return 1
    fi
    if ! generated_snapshot_dir="$(
        mktemp -d "${generated_snapshot_parent}/cohesix-generated.XXXXXX"
    )"; then
        generated_snapshot_parent=""
        release_generated_output_lock || true
        return 1
    fi
    if ! : >"${generated_snapshot_dir}/present" \
        || ! : >"${generated_snapshot_dir}/missing"; then
        local failed_dir="$generated_snapshot_dir"
        local failed_parent="$generated_snapshot_parent"
        generated_snapshot_dir=""
        generated_snapshot_parent=""
        discard_generated_snapshot_dir "$failed_dir" "$failed_parent" || true
        release_generated_output_lock || true
        return 1
    fi
    local relative
    for relative in "${GENERATED_OUTPUT_PATHS[@]}"; do
        local source="${PROJECT_ROOT}/${relative}"
        if [[ -e "$source" || -L "$source" ]]; then
            if ! mkdir -p \
                "${generated_snapshot_dir}/files/$(dirname "$relative")" \
                || ! cp -pR \
                    "$source" \
                    "${generated_snapshot_dir}/files/${relative}" \
                || ! printf '%s\n' \
                    "$relative" >>"${generated_snapshot_dir}/present"; then
                local failed_dir="$generated_snapshot_dir"
                local failed_parent="$generated_snapshot_parent"
                generated_snapshot_dir=""
                generated_snapshot_parent=""
                discard_generated_snapshot_dir \
                    "$failed_dir" "$failed_parent" || true
                release_generated_output_lock || true
                return 1
            fi
        else
            if ! printf '%s\n' \
                "$relative" >>"${generated_snapshot_dir}/missing"; then
                local failed_dir="$generated_snapshot_dir"
                local failed_parent="$generated_snapshot_parent"
                generated_snapshot_dir=""
                generated_snapshot_parent=""
                discard_generated_snapshot_dir \
                    "$failed_dir" "$failed_parent" || true
                release_generated_output_lock || true
                return 1
            fi
        fi
    done
    if ! printf 'cohesix-generated-snapshot-v1 count=%s\n' \
        "${#GENERATED_OUTPUT_PATHS[@]}" >"${generated_snapshot_dir}/complete" \
        || ! validate_generated_snapshot_metadata \
            "$generated_snapshot_dir" "$generated_snapshot_parent"; then
        local failed_dir="$generated_snapshot_dir"
        local failed_parent="$generated_snapshot_parent"
        generated_snapshot_dir=""
        generated_snapshot_parent=""
        discard_generated_snapshot_dir "$failed_dir" "$failed_parent" || true
        release_generated_output_lock || true
        return 1
    fi
    generated_snapshot_ready=1
}

restore_generated_outputs() {
    if [[ "$generated_snapshot_ready" != "1" ]]; then
        return 0
    fi
    if [[ "$generated_output_lock_held" != "1" ]]; then
        echo "Refusing generated restore without its exclusion lock" >&2
        return 1
    fi
    # Validate the complete partition and every declared snapshot payload
    # before the first live generated path is moved.
    if ! validate_generated_snapshot_metadata \
        "$generated_snapshot_dir" "$generated_snapshot_parent"; then
        echo "Generated snapshot retained for inspection: $generated_snapshot_dir" >&2
        return 1
    fi
    local relative
    for relative in "${GENERATED_OUTPUT_PATHS[@]}"; do
        if grep -Fx "$relative" "${generated_snapshot_dir}/present" >/dev/null; then
            restore_present_generated_path "$relative" || return 1
        else
            restore_missing_generated_path "$relative" || return 1
        fi
    done
    for relative in "${GENERATED_OUTPUT_PATHS[@]}"; do
        if grep -Fx "$relative" "${generated_snapshot_dir}/present" >/dev/null; then
            if ! diff -qr \
                "${generated_snapshot_dir}/files/${relative}" \
                "${PROJECT_ROOT}/${relative}" >/dev/null; then
                echo "Failed to restore generated output exactly: ${relative}" >&2
                return 1
            fi
        elif [[ -e "${PROJECT_ROOT}/${relative}" \
            || -L "${PROJECT_ROOT}/${relative}" ]]; then
            echo "Generated output should have been removed during restore: ${relative}" >&2
            return 1
        fi
    done
    # A previous path retained after a double rename failure is no longer the
    # sole backup once every live output exactly matches the validated
    # snapshot. Dispose such recovery work only at this proven boundary.
    if ! discard_preserved_generated_restore_work_dirs; then
        return 1
    fi
    local completed_snapshot_dir="$generated_snapshot_dir"
    local completed_snapshot_parent="$generated_snapshot_parent"
    # The live tree is already exact. Make EXIT re-entry a no-op before
    # disposal, because disposal itself can fail or be interrupted midway.
    generated_snapshot_dir=""
    generated_snapshot_parent=""
    generated_snapshot_ready=0
    local completion_status=0
    if ! discard_generated_snapshot_dir \
        "$completed_snapshot_dir" "$completed_snapshot_parent"; then
        echo \
            "Restored generated outputs, but failed to discard snapshot: ${completed_snapshot_dir}" \
            >&2
        completion_status=1
    fi
    if ! release_generated_output_lock; then
        completion_status=1
    fi
    return "$completion_status"
}

group_selected() {
    local group="$1"
    local selected="${COHSH_BATCH_GROUPS:-all}"
    if [[ -z "$selected" || "$selected" == "all" ]]; then
        return 0
    fi
    selected="${selected//,/ }"
    local item
    for item in $selected; do
        if [[ "$item" == "$group" ]]; then
            return 0
        fi
    done
    return 1
}

log_skip_group() {
    local group="$1"
    if [[ -n "${SUMMARY_LOG:-}" ]]; then
        printf "SKIP group=%s reason=COHSH_BATCH_GROUPS\n" "$group" | tee -a "$SUMMARY_LOG"
    else
        printf "SKIP group=%s reason=COHSH_BATCH_GROUPS\n" "$group"
    fi
}

selected_script_count() {
    local total=0
    if group_selected "base"; then
        total=$((total + ${#BASE_SCRIPTS[@]}))
        if [[ "$BATCH_TARGET" == "qemu" ]]; then
            total=$((total + 1))
        fi
    fi
    if group_selected "base-telemetry"; then
        total=$((total + ${#BASE_TELEMETRY_SCRIPTS[@]}))
    fi
    if group_selected "base-shard"; then
        total=$((total + ${#BASE_SHARD_SCRIPTS[@]}))
    fi
    if group_selected "gated"; then
        total=$((total + ${#GATED_SCRIPTS[@]}))
        if [[ "$BATCH_TARGET" == "qemu" ]]; then
            total=$((total + ${#QEMU_GATED_FIXTURE_SCRIPTS[@]}))
        fi
    fi
    printf "%s\n" "$total"
}

resolve_manifest_auth_token() {
    local manifest_path="$1"
    python3 - "$manifest_path" <<'PY'
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
if not manifest.is_file():
    print("bootstrap")
    raise SystemExit(0)

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:
    print("bootstrap")
    raise SystemExit(0)

data = tomllib.loads(manifest.read_text(encoding="utf-8"))
tickets = data.get("tickets", [])
for ticket in tickets:
    if str(ticket.get("role", "")).strip() == "queen":
        secret = str(ticket.get("secret", "")).strip()
        if secret:
            print(secret)
            raise SystemExit(0)
print("bootstrap")
PY
}

DEFAULT_MANIFEST_AUTH_TOKEN="$(resolve_manifest_auth_token "${BASE_MANIFEST}")"
COHSH_AUTH_TOKEN="${COHSH_AUTH_TOKEN:-${COH_AUTH_TOKEN:-${DEFAULT_MANIFEST_AUTH_TOKEN}}}"

is_local_tcp_host() {
    case "$1" in
        127.0.0.1|localhost|::1)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

check_port_open() {
    local host="$1"
    local port="$2"
    python3 - "$host" "$port" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=0.5):
        sys.exit(0)
except OSError:
    sys.exit(1)
PY
    local status=$?
    if [ "$status" -eq 0 ]; then
        return 0
    fi
    if is_local_tcp_host "$host" && command -v lsof >/dev/null 2>&1; then
        if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
            return 0
        fi
    fi
    return 1
}

check_port_available() {
    local host="$1"
    local port="$2"
    local kind="$3"
    python3 - "$host" "$port" "$kind" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
kind = sys.argv[3]
sock_type = socket.SOCK_DGRAM if kind == "udp" else socket.SOCK_STREAM
try:
    with socket.socket(socket.AF_INET, sock_type) as sock:
        sock.bind((host, port))
        if sock_type == socket.SOCK_STREAM:
            sock.listen(1)
except (OSError, ValueError):
    raise SystemExit(1)
raise SystemExit(0)
PY
}

find_free_host_port() {
    local kind="$1"
    python3 - "$kind" <<'PY'
import socket
import sys

kind = sys.argv[1]
sock_type = socket.SOCK_DGRAM if kind == "udp" else socket.SOCK_STREAM
with socket.socket(socket.AF_INET, sock_type) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

resolve_qemu_host_ports() {
    if [[ -z "$QEMU_TCP_PORT" ]]; then
        QEMU_TCP_PORT="$(find_free_host_port tcp)"
    fi
    if [[ -z "$QEMU_UDP_PORT" ]]; then
        QEMU_UDP_PORT="$(find_free_host_port udp)"
    fi
    if [[ -z "$QEMU_SMOKE_PORT" ]]; then
        QEMU_SMOKE_PORT="$(find_free_host_port tcp)"
    fi
    while [[ "$QEMU_UDP_PORT" == "$QEMU_TCP_PORT" ]]; do
        QEMU_UDP_PORT="$(find_free_host_port udp)"
    done
    while [[ "$QEMU_SMOKE_PORT" == "$QEMU_TCP_PORT" \
        || "$QEMU_SMOKE_PORT" == "$QEMU_UDP_PORT" ]]; do
        QEMU_SMOKE_PORT="$(find_free_host_port tcp)"
    done
    printf \
        'INFO: QEMU host ports console=%s udp=%s smoke=%s\n' \
        "$QEMU_TCP_PORT" \
        "$QEMU_UDP_PORT" \
        "$QEMU_SMOKE_PORT"
}

wait_port_ready() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if (( pid > 0 )) && ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_port_open "$host" "$port"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

check_auth_ready() {
    local host="$1"
    local port="$2"
    local token="$3"
    python3 - "$host" "$port" "$token" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
token = sys.argv[3]
payload = f"AUTH {token}".encode()
frame_len = len(payload) + 4
frame = frame_len.to_bytes(4, "little") + payload
try:
    with socket.create_connection((host, port), timeout=0.5) as sock:
        sock.settimeout(0.8)
        sock.sendall(frame)
        header = sock.recv(4)
        if len(header) != 4:
            sys.exit(1)
        total = int.from_bytes(header, "little")
        if total < 4 or total > 4096:
            sys.exit(1)
        remaining = total - 4
        chunks = bytearray()
        while len(chunks) < remaining:
            part = sock.recv(remaining - len(chunks))
            if not part:
                break
            chunks.extend(part)
        if len(chunks) != remaining:
            sys.exit(1)
        data = bytes(chunks)
except OSError:
    sys.exit(1)

if data == b"OK AUTH":
    sys.exit(0)
sys.exit(1)
PY
}

wait_auth_ready() {
    local host="$1"
    local port="$2"
    local token="$3"
    local timeout="$4"
    local pid="$5"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if (( pid > 0 )) && ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_auth_ready "$host" "$port" "$token"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

log_has() {
    local file="$1"
    local pattern="$2"
    python3 - "$file" "$pattern" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
pattern = sys.argv[2]
if not path.exists():
    sys.exit(1)
data = path.read_bytes()
try:
    text = data.decode(errors="ignore")
except Exception:
    sys.exit(1)
sys.exit(0 if pattern in text else 1)
PY
}

wait_log_marker() {
    local file="$1"
    local pattern="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if log_has "$file" "$pattern"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

script_path_for() {
    local script="$1"
    case "$script" in
        boot_v0.coh)
            echo "scripts/cohsh/boot_v0.coh"
            ;;
        9p_batch.coh)
            echo "scripts/cohsh/9p_batch.coh"
            ;;
        host_absent.coh)
            echo "scripts/cohsh/host_absent.coh"
            ;;
        host_sidecar_mock.coh)
            echo "scripts/cohsh/host_sidecar_mock.coh"
            ;;
        telemetry_ring.coh)
            echo "scripts/cohsh/telemetry_ring.coh"
            ;;
        telemetry_push_create.coh)
            echo "scripts/cohsh/telemetry_push_create.coh"
            ;;
        shard_1k.coh)
            echo "scripts/cohsh/shard_1k.coh"
            ;;
        observe_watch.coh)
            echo "scripts/cohsh/observe_watch.coh"
            ;;
        root_cut_basic.coh)
            echo "scripts/cohsh/root_cut_basic.coh"
            ;;
        session_lifecycle.coh)
            echo "scripts/cohsh/session_lifecycle.coh"
            ;;
        busy_backpressure.coh)
            echo "scripts/cohsh/busy_backpressure.coh"
            ;;
        cas_roundtrip.coh)
            echo "scripts/cohsh/cas_roundtrip.coh"
            ;;
        cas_fixture_signature_rejected.coh)
            echo "scripts/cohsh/cas_fixture_signature_rejected.coh"
            ;;
        tcp_basic.coh)
            echo "scripts/cohsh/tcp_basic.coh"
            ;;
        session_pool.coh)
            echo "scripts/cohsh/session_pool.coh"
            ;;
        policy_gate.coh)
            echo "scripts/cohsh/policy_gate.coh"
            ;;
        model_cas_bind.coh)
            echo "scripts/cohsh/model_cas_bind.coh"
            ;;
        replay_journal.coh)
            echo "scripts/cohsh/replay_journal.coh"
            ;;
        sidecar_integration.coh)
            echo "scripts/cohsh/sidecar_integration.coh"
            ;;
        *)
            echo "Unknown script: $script" >&2
            return 2
            ;;
    esac
}

run_cohsh_file() {
    local script_path="$1"
    local bin="${COHSH_BIN:-./out/cohesix/host-tools/cohsh}"
    local tcp_host="${COHSH_RUN_TCP_HOST:-127.0.0.1}"
    local tcp_port="${COHSH_RUN_TCP_PORT:-31337}"
    local args=(
        "$bin"
        --transport tcp
        --tcp-host "$tcp_host"
        --tcp-port "$tcp_port"
        --auth-token "${COHSH_AUTH_TOKEN}"
    )
    if [[ -n "${COHSH_RUN_POLICY:-}" ]]; then
        args+=(--policy "$COHSH_RUN_POLICY")
    fi
    args+=(--script "$script_path")
    "${args[@]}"
}

run_cohsh() {
    local script="$1"
    local script_path
    if ! script_path="$(script_path_for "$script")"; then
        return 2
    fi
    run_cohsh_file "$script_path"
}

reset_scoped_directory() {
    local directory="$1"
    case "$directory" in
        "$ARCHIVE_ROOT"|"$QEMU_ARTIFACT_ROOT"|\
        "${PROJECT_ROOT}/"*|"${ARCHIVE_ROOT}/"*|"${QEMU_ARTIFACT_ROOT}/"*)
            ;;
        *)
            echo "Refusing to reset directory outside scoped roots: $directory" >&2
            return 1
            ;;
    esac
    if [[ -d "$directory" ]]; then
        find "$directory" -mindepth 1 -delete
    else
        mkdir -p "$directory"
    fi
}

prepare_qemu_artifact() {
    local variant="$1"
    local manifest="$2"
    local artifact_dir="${QEMU_ARTIFACT_ROOT}/${variant}"
    local artifact_manifest="${artifact_dir}/qemu-artifact.json"
    local build_log="${ARCHIVE_ROOT}/build/${variant}.log"
    local selected_profile="${COHESIX_SEL4_PROFILE:-qemu_smp_production}"
    local smp="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-4,cores=4,threads=1,sockets=1}}"
    local virtualization="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-off}}"
    local machine_extra="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
    if [[ -z "$machine_extra" && "$(uname -s)" == "Darwin" ]]; then
        machine_extra="kernel-irqchip=off"
    fi

    snapshot_generated_outputs
    reset_scoped_directory "$artifact_dir"
    mkdir -p "$artifact_dir" "$(dirname "$build_log")"
    echo "INFO: building QEMU artifact variant=${variant} manifest=${manifest}"
    COHESIX_SEL4_PROFILE="$selected_profile" \
        COH_RTC_MANIFEST="$manifest" \
        SEL4_BUILD_DIR="$SEL4_BUILD_DIR" \
        "$BUILD_RUN_BIN" \
        --sel4-build "$SEL4_BUILD_DIR" \
        --out-dir "$artifact_dir" \
        --profile release \
        --qemu "$QEMU_BIN" \
        --root-task-features cohesix-dev \
        --cargo-target aarch64-unknown-none \
        --transport tcp \
        --no-run \
        >"$build_log" 2>&1

    local record_args=(
        record
        --artifact-dir "$artifact_dir"
        --output "$artifact_manifest"
        --manifest "$manifest"
        --resolved-manifest "$GENERATED_CONFIG_DIR/root_task_resolved.json"
        --policy "$GENERATED_CONFIG_DIR/cohsh_policy.toml"
        --source-digest "$TEST_SOURCE_DIGEST"
        --sel4-build "$SEL4_BUILD_DIR"
        --sel4-profile "$selected_profile"
        --qemu "$QEMU_BIN"
        --root-task-features cohesix-dev
        --cargo-target aarch64-unknown-none
        --smp "$smp"
        --virtualization "$virtualization"
        --machine-extra "$machine_extra"
        --net-backend virtio
        --detect-gic-script "$PROJECT_ROOT/scripts/lib/detect_gic_version.py"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
    )
    local launch_accelerator
    launch_accelerator="$(python3 - "$artifact_dir/cohesix-qemu-launch-artifacts.json" <<'PY'
import json
from pathlib import Path
import sys

document = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(document["qemu"]["accelerator"])
PY
)"
    record_args+=(--accelerator "$launch_accelerator")
    if [[ -n "$TEST_ATTEMPT_MANIFEST" && -f "$TEST_ATTEMPT_MANIFEST" ]]; then
        record_args+=(--attempt-manifest "$TEST_ATTEMPT_MANIFEST")
    elif [[ -n "$TEST_ATTEMPT_MANIFEST" ]]; then
        echo "WARN: attempt manifest is not yet available; omitting from artifact evidence: ${TEST_ATTEMPT_MANIFEST}" \
            >>"$build_log"
    fi
    "$QEMU_ARTIFACT_HELPER" "${record_args[@]}" >>"$build_log"
    local artifact_claim_tier
    artifact_claim_tier="$(python3 - "$artifact_manifest" <<'PY'
import json
from pathlib import Path
import sys

document = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(document["qemu"]["claim"]["tier"])
PY
)"
    if [[ -z "$QEMU_ARTIFACT_CLAIM_TIER" ]]; then
        QEMU_ARTIFACT_CLAIM_TIER="$artifact_claim_tier"
        TEST_CLAIM_TIER="$artifact_claim_tier"
    elif [[ "$artifact_claim_tier" != "$QEMU_ARTIFACT_CLAIM_TIER" ]]; then
        echo "QEMU artifact variants have different claim tiers" >&2
        return 1
    fi
    echo "INFO: QEMU artifact ready variant=${variant} manifest=${artifact_manifest}"

    case "$variant" in
        base)
            BASE_QEMU_ARTIFACT_MANIFEST="$artifact_manifest"
            ;;
        gated)
            GATED_QEMU_ARTIFACT_MANIFEST="$artifact_manifest"
            ;;
        *)
            echo "Unknown QEMU artifact variant: $variant" >&2
            return 1
            ;;
    esac
}

run_qemu_response_matrix() {
    local label="$1"
    local log_root="$2"
    local archive_root="$3"
    local qemu_log="$4"
    local matrix_log="${log_root}/${label}.out.log"
    local arguments=(
        python3
        "$QEMU_RESPONSE_MATRIX_SCRIPT"
        --host "$QEMU_TCP_HOST"
        --port "$QEMU_TCP_PORT"
        --mode fixed
    )

    echo "=== Running base/${label} ==="
    if ! COHSH_AUTH_TOKEN="$COHSH_AUTH_TOKEN" "${arguments[@]}" >"$matrix_log" 2>&1; then
        echo "FAIL: QEMU TCP response matrix" >&2
        cp "$qemu_log" "${archive_root}/${label}.qemu.log" || true
        cp "$matrix_log" "${archive_root}/${label}.out.log" || true
        return 1
    fi
    # The matrix itself requires exact OK QUIT followed by target EOF on the
    # same socket. The isolated console child owns TCP close/relisten, so a
    # legacy root-stack UART close marker is neither required nor authoritative.
    cp "$qemu_log" "${archive_root}/${label}.qemu.log"
    cp "$matrix_log" "${archive_root}/${label}.out.log"
    echo "PASS: ${label}"
}

write_qemu_result() {
    local name="$1"
    local artifact_manifest="$2"
    local boot_id="$3"
    local qemu_log="$4"
    shift 4
    local scripts=("$@")
    local result_path="${TRANSPORT_RESULT_ROOT}/${name}.json"
    local arguments=(
        result
        --output "$result_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier "$TEST_CLAIM_TIER"
        --target qemu
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
        --artifact-manifest "$artifact_manifest"
        --artifact-action-id "$TEST_ACTION_ID"
        --artifact-catalog-action-digest "$TEST_ACTION_DIGEST"
        --boot-id "$boot_id"
        --group "$name"
        --status pass
        --log "$qemu_log"
    )
    local script
    for script in "${scripts[@]}"; do
        arguments+=(--script "$script")
        arguments+=(--log "${ARCHIVE_ROOT}/runtime/${name}/${script%.coh}.out.log")
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${result_path}.id"
}

run_batch() {
    local name="$1"
    local artifact_manifest="$2"
    shift 2
    local scripts=("$@")
    local evidence_scripts=("${scripts[@]}")

    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_TCP_PORT" tcp; then
        echo "QEMU console port already in use: ${QEMU_TCP_PORT}" >&2
        return 1
    fi
    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_UDP_PORT" udp; then
        echo "QEMU UDP echo port already in use: ${QEMU_UDP_PORT}" >&2
        return 1
    fi
    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_SMOKE_PORT" tcp; then
        echo "QEMU smoke port already in use: ${QEMU_SMOKE_PORT}" >&2
        return 1
    fi

    "$QEMU_ARTIFACT_HELPER" verify \
        --artifact-manifest "$artifact_manifest" \
        --source-digest "$TEST_SOURCE_DIGEST" \
        --action-id "$TEST_ACTION_ID" \
        --catalog-action-digest "$TEST_ACTION_DIGEST" >/dev/null

    local artifact_dir
    artifact_dir="$(dirname "$artifact_manifest")"
    local log_root="${ARCHIVE_ROOT}/runtime/${name}"
    local archive_root="${ARCHIVE_ROOT}/${name}"
    local qemu_log="${log_root}/regression_batch.qemu.log"
    local boot_id="${RUN_ID}-${name}-$$-${RANDOM}"

    mkdir -p "$log_root" "$archive_root"
    "$QEMU_ARTIFACT_HELPER" launch \
        --artifact-manifest "$artifact_manifest" \
        --qemu "$QEMU_BIN" \
        --source-digest "$TEST_SOURCE_DIGEST" \
        --action-id "$TEST_ACTION_ID" \
        --catalog-action-digest "$TEST_ACTION_DIGEST" \
        --console-port "$QEMU_TCP_PORT" \
        --udp-port "$QEMU_UDP_PORT" \
        --smoke-port "$QEMU_SMOKE_PORT" \
        >"$qemu_log" 2>&1 &
    qemu_pid=$!

    if ! wait_log_marker "$qemu_log" "$READY_MARKER" "$READY_TIMEOUT" "$qemu_pid"; then
        echo "FAIL: root console ready marker not observed" >&2
        tail -n 50 "$qemu_log" >&2 || true
        return 1
    fi
    echo "INFO: root console ready marker observed"

    COHSH_BIN="${artifact_dir}/host-tools/cohsh"
    COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
    COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"
    COHSH_RUN_POLICY="${artifact_dir}/evidence/cohsh_policy.toml"

    if [[ "$name" == "base" ]]; then
        run_qemu_response_matrix \
            "$QEMU_RESPONSE_MATRIX_FIXED_LABEL" \
            "$log_root" \
            "$archive_root" \
            "$qemu_log" || return 1
        evidence_scripts+=("$QEMU_RESPONSE_MATRIX_FIXED_LABEL")
    fi

    for script in "${scripts[@]}"; do
        local script_name="${script%.coh}"
        echo "=== Running ${name}/${script} ==="

        local coh_log="${log_root}/${script_name}.out.log"

        # A successful cohsh exit includes the TCP transport's exact OK QUIT
        # followed by peer EOF. Do not substitute a root UART audit marker for
        # that same-connection protocol proof.
        if ! run_cohsh "$script" > "$coh_log" 2>&1; then
            echo "FAIL: cohsh script ${script}" >&2
            cp "$qemu_log" "${archive_root}/${script_name}.qemu.log" || true
            cp "$coh_log" "${archive_root}/${script_name}.out.log" || true
            return 1
        fi

        cp "$qemu_log" "${archive_root}/${script_name}.qemu.log"
        cp "$coh_log" "${archive_root}/${script_name}.out.log"
        echo "PASS: ${script}"
    done

    if kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    qemu_pid=0
    write_qemu_result \
        "$name" \
        "$artifact_manifest" \
        "$boot_id" \
        "$qemu_log" \
        "${evidence_scripts[@]}"
    return 0
}

ensure_live_cohsh_bin() {
    if [[ -n "${COHSH_BIN:-}" ]]; then
        if [[ ! -x "$COHSH_BIN" ]]; then
            echo "Configured COHSH_BIN is not executable: $COHSH_BIN" >&2
            return 1
        fi
        return 0
    fi

    COHSH_BIN="${PROJECT_ROOT}/target/debug/cohsh"
    if [[ -x "$COHSH_BIN" ]]; then
        return 0
    fi

    cargo build -p cohsh
    if [[ ! -x "$COHSH_BIN" ]]; then
        echo "cohsh build completed but binary is missing: $COHSH_BIN" >&2
        return 1
    fi
}

write_lifecycle_resume_script() {
    local path="$1"
    cat > "$path" <<'EOF'
# Author: Lukas Bower
# Purpose: Resume a live Pi 4 regression target before continuing batch execution.
# Copyright 2026 Lukas Bower
attach queen
EXPECT OK
lifecycle resume
EXPECT OK
quit
EXPECT OK
EOF
}

write_lifecycle_touch_script() {
    local path="$1"
    cat > "$path" <<'EOF'
# Author: Lukas Bower
# Purpose: Refresh live Pi 4 root reachability after an idempotent lifecycle check.
# Copyright 2026 Lukas Bower
attach queen
EXPECT OK
quit
EXPECT OK
EOF
}

run_lifecycle_touch() {
    local label="$1"
    local log_path="${ARCHIVE_ROOT}/touch-${label}.out.log"
    if run_cohsh_file "$LIFECYCLE_TOUCH_SCRIPT" > "$log_path" 2>&1; then
        printf "TOUCH %s status=0\n" "$label" | tee -a "$SUMMARY_LOG"
        return 0
    fi
    local status=$?
    printf "TOUCH %s status=%s\n" "$label" "$status" | tee -a "$SUMMARY_LOG"
    sed 's/^/  /' "$log_path" | tail -n 20 | tee -a "$SUMMARY_LOG"
    return "$status"
}

run_lifecycle_resume() {
    local label="$1"
    local log_path="${ARCHIVE_ROOT}/resume-${label}.out.log"
    local status
    mkdir -p "$ARCHIVE_ROOT"
    set +e
    run_cohsh_file "$LIFECYCLE_RESUME_SCRIPT" > "$log_path" 2>&1
    status=$?
    set -e
    if [[ "$status" -eq 0 ]]; then
        printf "RESUME %s status=0\n" "$label" | tee -a "$SUMMARY_LOG"
        return 0
    fi
    if grep -q 'state=ONLINE reason=invalid-transition' "$log_path"; then
        printf "RESUME %s status=0 already-online\n" "$label" | tee -a "$SUMMARY_LOG"
        run_lifecycle_touch "$label" || true
        return 0
    fi
    printf "RESUME %s status=%s\n" "$label" "$status" | tee -a "$SUMMARY_LOG"
    sed 's/^/  /' "$log_path" | tail -n 20 | tee -a "$SUMMARY_LOG"
    return "$status"
}

run_live_group() {
    local name="$1"
    shift
    local scripts=("$@")
    local group_dir="${ARCHIVE_ROOT}/${name}"
    mkdir -p "$group_dir"

    run_lifecycle_resume "before-${name}" || true
    for script in "${scripts[@]}"; do
        local script_name="${script%.coh}"
        local coh_log="${group_dir}/${script_name}.out.log"

        pi_total=$((pi_total + 1))
        printf "=== Running %s/%s ===\n" "$name" "$script" | tee -a "$SUMMARY_LOG"
        if run_cohsh "$script" > "$coh_log" 2>&1; then
            pi_pass=$((pi_pass + 1))
            printf "PASS %s/%s\n" "$name" "$script" | tee -a "$SUMMARY_LOG"
        else
            local status=$?
            pi_fail=$((pi_fail + 1))
            printf "FAIL %s/%s status=%s\n" "$name" "$script" "$status" | tee -a "$SUMMARY_LOG"
            sed 's/^/  /' "$coh_log" | tail -n 40 | tee -a "$SUMMARY_LOG"
            if [[ "$BATCH_CONTINUE_ON_FAIL" != "1" ]]; then
                run_lifecycle_resume "after-${name}-${script_name}" || true
                return "$status"
            fi
        fi
        run_lifecycle_resume "after-${name}-${script_name}" || true
    done
}

write_pi4_result() {
    local name="$1"
    shift
    local scripts=("$@")
    if [[ -z "$TARGET_EVIDENCE_FILE" ]]; then
        if [[ "$REQUIRE_RESULT_EVIDENCE" == "1" ]]; then
            echo "Pi 4 transport evidence requires TEST_PLAN_TARGET_EVIDENCE_FILE" >&2
            return 1
        fi
        printf \
            "NO_CLAIM group=%s tier=pi4-transport reason=target-evidence-missing\n" \
            "$name" | tee -a "$SUMMARY_LOG"
        return 0
    fi

    local result_path="${TRANSPORT_RESULT_ROOT}/${name}.json"
    local arguments=(
        result
        --output "$result_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier pi4-transport
        --target pi4
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
        --target-evidence "$TARGET_EVIDENCE_FILE"
        --group "$name"
        --status pass
        --log "$SUMMARY_LOG"
    )
    local script
    for script in "${scripts[@]}"; do
        arguments+=(--script "$script")
        arguments+=(--log "${ARCHIVE_ROOT}/${name}/${script%.coh}.out.log")
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${result_path}.id"
}

write_transport_aggregate() {
    local target="$1"
    local claim_tier="$2"
    if [[ "$target" == "pi4" && -z "$TARGET_EVIDENCE_FILE" ]]; then
        return 0
    fi
    local aggregate_path="${TRANSPORT_RESULT_ROOT}/stage-03.json"
    local arguments=(
        aggregate
        --output "$aggregate_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier "$claim_tier"
        --target "$target"
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
    )
    local group
    for group in base base-telemetry base-shard gated; do
        if group_selected "$group"; then
            arguments+=(--result "${TRANSPORT_RESULT_ROOT}/${group}.json")
        fi
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${aggregate_path}.id"
}

run_pi4_batch() {
    reset_scoped_directory "$ARCHIVE_ROOT"
    SUMMARY_LOG="${ARCHIVE_ROOT}/summary.log"
    : > "$SUMMARY_LOG"
    LIFECYCLE_RESUME_SCRIPT="${ARCHIVE_ROOT}/lifecycle_resume.coh"
    LIFECYCLE_TOUCH_SCRIPT="${ARCHIVE_ROOT}/lifecycle_touch.coh"
    write_lifecycle_resume_script "$LIFECYCLE_RESUME_SCRIPT"
    write_lifecycle_touch_script "$LIFECYCLE_TOUCH_SCRIPT"

    COHSH_RUN_TCP_HOST="$TCP_HOST"
    COHSH_RUN_TCP_PORT="$TCP_PORT"
    ensure_live_cohsh_bin

    printf "INFO target=pi4 tcp=%s:%s log_root=%s\n" "$TCP_HOST" "$TCP_PORT" "$ARCHIVE_ROOT" | tee -a "$SUMMARY_LOG"
    if ! wait_port_ready "$TCP_HOST" "$TCP_PORT" "$PORT_TIMEOUT" 0; then
        printf "FAIL: TCP console not reachable on %s:%s within %ss\n" "$TCP_HOST" "$TCP_PORT" "$PORT_TIMEOUT" | tee -a "$SUMMARY_LOG" >&2
        return 1
    fi
    printf "INFO: TCP console reachable on %s:%s\n" "$TCP_HOST" "$TCP_PORT" | tee -a "$SUMMARY_LOG"

    if ! wait_auth_ready "$TCP_HOST" "$TCP_PORT" "$COHSH_AUTH_TOKEN" "$AUTH_READY_TIMEOUT" 0; then
        printf "FAIL: TCP console auth endpoint not ready on %s:%s within %ss\n" "$TCP_HOST" "$TCP_PORT" "$AUTH_READY_TIMEOUT" | tee -a "$SUMMARY_LOG" >&2
        return 1
    fi
    printf "INFO: TCP auth handshake is responsive\n" | tee -a "$SUMMARY_LOG"

    pi_pass=0
    pi_fail=0
    pi_total=0
    if group_selected "base"; then
        if ! run_live_group "base" "${BASE_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base"
    fi
    if group_selected "base-telemetry"; then
        if ! run_live_group "base-telemetry" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base-telemetry"
    fi
    if group_selected "base-shard"; then
        if ! run_live_group "base-shard" "${BASE_SHARD_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base-shard"
    fi
    if group_selected "gated"; then
        if ! run_live_group "gated" "${GATED_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "gated"
    fi

    # Fresh-boot proof after this point is intentionally UART/operator-driven;
    # TCP disappears during reset. Follow the top-of-file minicom notes.
    printf "RESULT pass=%s fail=%s total=%s log_root=%s\n" "$pi_pass" "$pi_fail" "$pi_total" "$ARCHIVE_ROOT" | tee -a "$SUMMARY_LOG"
    if (( pi_fail > 0 )); then
        return 1
    fi
    if group_selected "base"; then
        write_pi4_result "base" "${BASE_SCRIPTS[@]}"
    fi
    if group_selected "base-telemetry"; then
        write_pi4_result "base-telemetry" "${BASE_TELEMETRY_SCRIPTS[@]}"
    fi
    if group_selected "base-shard"; then
        write_pi4_result "base-shard" "${BASE_SHARD_SCRIPTS[@]}"
    fi
    if group_selected "gated"; then
        write_pi4_result "gated" "${GATED_SCRIPTS[@]}"
    fi
    write_transport_aggregate "pi4" "pi4-transport"
    return 0
}

qemu_pid=0
SUMMARY_LOG=""
LIFECYCLE_RESUME_SCRIPT=""
LIFECYCLE_TOUCH_SCRIPT=""
pi_pass=0
pi_fail=0
pi_total=0

cleanup() {
    local status=$?
    local cleanup_status=0
    # Cleanup owns generated-output restoration. Ignore repeated termination
    # signals before touching the snapshot so they cannot interrupt the EXIT
    # transaction and strand a live generated path or inherited lock.
    trap '' HUP INT TERM
    set +e
    if (( qemu_pid > 0 )); then
        if kill -0 "$qemu_pid" 2>/dev/null; then
            kill "$qemu_pid" || true
            wait "$qemu_pid" 2>/dev/null || true
        fi
    fi
    if ! restore_generated_outputs; then
        cleanup_status=1
    fi
    if ! release_generated_output_lock; then
        cleanup_status=1
    fi
    if (( cleanup_status != 0 )); then
        status=1
    fi
    trap - EXIT
    exit "$status"
}
terminate_batch() {
    local status="$1"
    # Ignore a second signal before entering EXIT cleanup. Resetting these
    # handlers to their defaults creates a window where cleanup can be killed.
    trap '' HUP INT TERM
    exit "$status"
}
trap cleanup EXIT
trap 'terminate_batch 129' HUP
trap 'terminate_batch 130' INT
trap 'terminate_batch 143' TERM

if [[ "$BATCH_TARGET" == "pi4" ]]; then
    if [[ -n "$TARGET_EVIDENCE_FILE" ]]; then
        "$QEMU_ARTIFACT_HELPER" verify-pi4-evidence \
            --target-evidence "$TARGET_EVIDENCE_FILE" \
            --source-digest "$TEST_SOURCE_DIGEST" >/dev/null
    elif [[ "$REQUIRE_RESULT_EVIDENCE" == "1" ]]; then
        echo "Pi 4 transport evidence requires TEST_PLAN_TARGET_EVIDENCE_FILE" >&2
        exit 1
    fi
    run_pi4_batch
    exit $?
fi

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    echo "Missing seL4 build directory: $SEL4_BUILD_DIR" >&2
    echo "Build the canonical profile or set SEL4_BUILD_DIR (or SEL4_BUILD) explicitly; default: ${PROJECT_ROOT}/out/sel4/profile-v2/qemu-smp-production" >&2
    exit 1
fi

resolve_qemu_host_ports
if [[ "${COHSH_BATCH_CLEAN_TARGET:-0}" == "1" ]]; then
    if [[ -d "${PROJECT_ROOT}/target" ]]; then
        find "${PROJECT_ROOT}/target" -mindepth 1 -delete
        rmdir "${PROJECT_ROOT}/target"
    fi
fi
reset_scoped_directory "$ARCHIVE_ROOT"
BASE_QEMU_ARTIFACT_MANIFEST=""
GATED_QEMU_ARTIFACT_MANIFEST=""
QEMU_ARTIFACT_CLAIM_TIER=""

if group_selected "base" \
    || group_selected "base-telemetry" \
    || group_selected "base-shard"; then
    prepare_qemu_artifact "base" "$BASE_MANIFEST"
fi
if group_selected "gated"; then
    prepare_qemu_artifact "gated" "$GATED_MANIFEST"
fi
# The built artifacts carry private copies of their resolved manifest and
# client policy, so tracked/default generated outputs can be restored before
# any long-running QEMU boot begins.
restore_generated_outputs

case "${COHSH_BATCH_PREPARE_ONLY:-0}" in
    0)
        ;;
    1)
        if [[ -n "$BASE_QEMU_ARTIFACT_MANIFEST" ]]; then
            printf 'BASE_QEMU_ARTIFACT_MANIFEST=%s\n' "$BASE_QEMU_ARTIFACT_MANIFEST"
        fi
        if [[ -n "$GATED_QEMU_ARTIFACT_MANIFEST" ]]; then
            printf 'GATED_QEMU_ARTIFACT_MANIFEST=%s\n' "$GATED_QEMU_ARTIFACT_MANIFEST"
        fi
        exit 0
        ;;
    *)
        echo "COHSH_BATCH_PREPARE_ONLY must be 0 or 1" >&2
        exit 2
        ;;
esac

if group_selected "base"; then
    if ! run_batch "base" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base"
fi

if group_selected "base-telemetry"; then
    if ! run_batch "base-telemetry" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base-telemetry"
fi

if group_selected "base-shard"; then
    if ! run_batch "base-shard" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_SHARD_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base-shard"
fi

if group_selected "gated"; then
    if ! run_batch \
        "gated" \
        "$GATED_QEMU_ARTIFACT_MANIFEST" \
        "${GATED_SCRIPTS[@]}" \
        "${QEMU_GATED_FIXTURE_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "gated"
fi

write_transport_aggregate "qemu" "qemu-integration"
echo "regression batch complete: $(selected_script_count) scripts passed"
