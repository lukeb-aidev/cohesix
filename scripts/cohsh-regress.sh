#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/.." && pwd)
cd "${REPO_ROOT}"

usage() {
    cat <<'USAGE'
Usage: scripts/cohsh-regress.sh [--target mock|qemu] [--script <file>] [-- <extra cargo args>]

Execute the Cohsh CLI against the specified transport target using a newline
separated command script. The default script tails the queen log and quits,
mirroring the operator flows documented in docs/USERLAND_AND_CLI.md.

Targets:
  mock  - Run using the in-process NineDoor transport (default).
  qemu  - Run using the future QEMU transport. Additional CLI arguments can be
          supplied via the COHSH_QEMU_ARGS environment variable.
USAGE
}

TARGET="mock"
SCRIPT_PATH=""
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            TARGET="$2"
            shift 2
            ;;
        --script)
            SCRIPT_PATH="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            EXTRA_ARGS=("$@")
            break
            ;;
        *)
            EXTRA_ARGS+=("$1")
            shift
            ;;
    esac
done

DEFAULT_SCRIPT=0
if [[ -z "$SCRIPT_PATH" ]]; then
    SCRIPT_PATH=$(mktemp)
    cat >"$SCRIPT_PATH" <<'COMMANDS'
attach queen
tail /log/queen.log
quit
COMMANDS
    DEFAULT_SCRIPT=1
fi

cleanup() {
    if [[ "$DEFAULT_SCRIPT" -eq 1 && -f "$SCRIPT_PATH" ]]; then
        rm -f "$SCRIPT_PATH"
    fi
}
trap cleanup EXIT

run_cli() {
    local transport="$1"
    shift
    local output
    if ! output=$(cargo run --quiet --bin cohsh -- "--transport" "$transport" --script "$SCRIPT_PATH" "$@" 2>&1); then
        echo "$output" >&2
        return 1
    fi
    printf '%s\n' "$output"
    if ! grep -q "Cohesix boot" <<<"$output"; then
        echo "cohsh regression expected boot banner in output" >&2
        return 1
    fi
    if ! grep -q "closing session" <<<"$output"; then
        echo "cohsh regression expected graceful shutdown" >&2
        return 1
    fi
}

run_mock() {
    if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
        run_cli mock "${EXTRA_ARGS[@]}"
    else
        run_cli mock
    fi
}

run_qemu() {
    local invocation=()
    if [[ -n "${COHSH_QEMU_ARGS:-}" ]]; then
        local raw_args=()
        # shellcheck disable=SC2206
        raw_args=(${COHSH_QEMU_ARGS})
        local token
        for token in "${raw_args[@]}"; do
            invocation+=(--qemu-arg "$token")
        done
    fi
    if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
        invocation+=("${EXTRA_ARGS[@]}")
    fi
    if [[ ${#invocation[@]} -gt 0 ]]; then
        run_cli qemu "${invocation[@]}"
    else
        run_cli qemu
    fi
}

case "$TARGET" in
    mock)
        run_mock
        ;;
    qemu)
        run_qemu
        ;;
    *)
        echo "Unknown target: $TARGET" >&2
        usage
        exit 1
        ;;
esac
