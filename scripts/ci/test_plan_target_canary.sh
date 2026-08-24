#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Collect one non-claiming QEMU or Pi target convergence observation.
# Copyright 2026 Lukas Bower

set -euo pipefail
umask 077

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
focus=${TEST_PLAN_CONVERGENCE_FOCUS:-}
run_id=${TEST_PLAN_CONVERGENCE_RUN_ID:-}
state_dir=${TEST_PLAN_CONVERGENCE_STATE_DIR:-}
observation=${TEST_PLAN_TARGET_OBSERVATION:-}
current_layer="target-entry"
serial_log=""
serial_source_log=""
image_path=""
image_identity_path=""
profile=""
operation_script=""
operation_log=""
qemu_pid=""

usage() {
  cat <<'EOF'
Usage: scripts/ci/test_plan_target_canary.sh --target qemu|pi4

This command is invoked by test_plan_converge.sh. It collects diagnostic
evidence only and never emits a Test Plan stage attestation.
EOF
}

die() {
  printf 'target canary: %s\n' "$*" >&2
  return 1
}

blocked() {
  printf 'target canary BLOCKED: %s\n' "$*" >&2
  return 3
}

write_observation() {
  local result=$1
  local failing_layer=$2
  local detail=$3
  [[ -n "${observation}" ]] || return 0
  python3 - \
    "${observation}" \
    "${result}" \
    "${failing_layer}" \
    "${detail}" \
    "${target}" \
    "${focus}" \
    "${run_id}" \
    "${profile}" \
    "${serial_log}" \
    "${serial_source_log}" \
    "${image_path}" \
    "${image_identity_path}" \
    "${operation_script}" \
    "${operation_log}" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import sys

(
    output_raw,
    result,
    failing_layer,
    detail,
    target,
    focus,
    run_id,
    profile,
    serial_raw,
    serial_source_raw,
    image_raw,
    identity_raw,
    operation_raw,
    operation_log_raw,
) = sys.argv[1:]


def file_record(raw: str):
    if not raw:
        return None
    path = Path(raw).resolve()
    if not path.is_file():
        return {"path": str(path), "present": False}
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return {
        "path": str(path),
        "present": True,
        "size_bytes": path.stat().st_size,
        "sha256": digest.hexdigest(),
    }


payload = {
    "schema": "cohesix-target-observation/v2",
    "banner": "NON-CLAIMING TARGET DIAGNOSTIC",
    "claiming": False,
    "result": result,
    "first_failing_proof_layer": failing_layer or None,
    "detail": detail,
    "target": target,
    "focus": focus,
    "run_id": run_id,
    "profile": profile,
    "serial_log": file_record(serial_raw),
    "serial_source_log": serial_source_raw or None,
    "built_image": file_record(image_raw),
    "image_identity": file_record(identity_raw),
    "operation_script": file_record(operation_raw),
    "operation_log": file_record(operation_log_raw),
}
output = Path(output_raw).resolve()
output.parent.mkdir(parents=True, exist_ok=True)
temporary = output.with_name(f".{output.name}.{os.getpid()}.tmp")
with temporary.open("x", encoding="utf-8") as handle:
    json.dump(payload, handle, indent=2, sort_keys=True)
    handle.write("\n")
    handle.flush()
    os.fsync(handle.fileno())
os.replace(temporary, output)
PY
}

stop_qemu() {
  if [[ -n "${qemu_pid}" ]] && kill -0 "${qemu_pid}" 2>/dev/null; then
    kill "${qemu_pid}" 2>/dev/null || true
    wait "${qemu_pid}" 2>/dev/null || true
  fi
  qemu_pid=""
}

cleanup() {
  local status=$?
  stop_qemu
  if [[ "${target:-}" == "pi4" && -n "${serial_log}" && \
        -f "${serial_log}" && "${serial_log}" != "${state_dir}/uart.snapshot.log" ]]; then
    cp "${serial_log}" "${state_dir}/uart.snapshot.log"
    serial_source_log=${serial_source_log:-${serial_log}}
    serial_log="${state_dir}/uart.snapshot.log"
  fi
  if [[ ! -f "${observation}" ]]; then
    if [[ "${status}" -eq 3 ]]; then
      write_observation "BLOCKED" "${current_layer}" "required target input or environment unavailable"
    elif [[ "${status}" -ne 0 ]]; then
      write_observation "FAIL" "${current_layer}" "target proof layer failed"
    fi
  fi
}

wait_for_marker() {
  local path=$1
  local marker=$2
  local timeout=$3
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if [[ -n "${qemu_pid}" ]] && ! kill -0 "${qemu_pid}" 2>/dev/null; then
      return 1
    fi
    if [[ -f "${path}" ]] && grep -Fq -- "${marker}" "${path}"; then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

marker_count() {
  local path=$1
  local marker=$2
  if [[ ! -f "${path}" ]]; then
    printf '0\n'
    return 0
  fi
  grep -F -c -- "${marker}" "${path}" || true
}

wait_for_marker_count() {
  local path=$1
  local marker=$2
  local minimum=$3
  local timeout=$4
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if [[ -n "${qemu_pid}" ]] && ! kill -0 "${qemu_pid}" 2>/dev/null; then
      return 1
    fi
    if (( $(marker_count "${path}" "${marker}") >= minimum )); then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

wait_for_port() {
  local host=$1
  local port=$2
  local timeout=$3
  python3 - "${host}" "${port}" "${timeout}" <<'PY'
import socket
import sys
import time

host, port_raw, timeout_raw = sys.argv[1:]
deadline = time.monotonic() + int(timeout_raw)
while time.monotonic() < deadline:
    try:
        with socket.create_connection((host, int(port_raw)), timeout=0.5):
            raise SystemExit(0)
    except OSError:
        time.sleep(0.2)
raise SystemExit(1)
PY
}

resolve_auth_token() {
  local manifest=$1
  python3 - "${manifest}" <<'PY'
from pathlib import Path
import sys
import tomllib

data = tomllib.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for ticket in data.get("tickets", []):
    if ticket.get("role") == "queen" and ticket.get("secret"):
        print(ticket["secret"])
        raise SystemExit(0)
raise SystemExit("selected manifest has no Queen ticket")
PY
}

cohsh_binary() {
  local preferred=$1
  if [[ -x "${preferred}" ]]; then
    printf '%s\n' "${preferred}"
    return 0
  fi
  if [[ ! -x "${repo_root}/target/debug/cohsh" ]]; then
    cargo build -p cohsh --no-default-features --features tcp >&2
  fi
  printf '%s\n' "${repo_root}/target/debug/cohsh"
}

run_live_operation() {
  local binary=$1
  local host=$2
  local port=$3
  local token=$4
  local log=$5
  "${binary}" \
    --transport tcp \
    --tcp-host "${host}" \
    --tcp-port "${port}" \
    --auth-token "${token}" \
    --script "${operation_script}" >"${log}" 2>&1
}

unexpected_faults() {
  local path=$1
  grep -Eiq \
    'unhandled (seL4 )?fault|kernel fault|capability fault|panicked at|runtime panic|MCS (allocation|bootstrap|scheduler).*(failed|error)|BOOT ABORT' \
    "${path}"
}

qemu_canary() {
  profile="qemu_smp_production / configs/root_task.toml"
  local sel4_build="${repo_root}/out/sel4/profile-v2/qemu-smp-production"
  local qemu_bin="${TEST_PLAN_CONVERGENCE_QEMU_BIN:-${QEMU_BIN:-qemu-system-aarch64}}"
  local launch_existing=${TEST_PLAN_CONVERGENCE_LAUNCH_EXISTING:-0}
  local qemu_out
  if [[ "${launch_existing}" == "1" ]]; then
    qemu_out="${TEST_PLAN_CONVERGENCE_QEMU_OUT_DIR:-${repo_root}/out/cohesix}"
  else
    qemu_out="${state_dir}/qemu-artifacts"
  fi
  local port
  port=$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)
  local build=(
    "${repo_root}/scripts/cohesix-build-run.sh"
    --sel4-build "${sel4_build}"
    --out-dir "${qemu_out}"
    --profile release
    --root-task-features release-qemu,bootstrap-trace
    --cargo-target aarch64-unknown-none
    --qemu "${qemu_bin}"
    --transport tcp
    --tcp-port "${port}"
  )

  current_layer="exact-target-build"
  if [[ "${launch_existing}" == "1" ]]; then
    [[ -f "${qemu_out}/cohesix-qemu-launch-artifacts.json" ]] || \
      blocked "--launch-existing has no immutable record in ${qemu_out}"
  else
    "${build[@]}" --no-run
  fi
  current_layer="image-validity"
  "${build[@]}" --launch-existing --no-run
  image_path="${qemu_out}/cohesix-system.cpio"
  image_identity_path="${qemu_out}/cohesix-qemu-launch-artifacts.json"
  [[ -f "${image_path}" && -f "${image_identity_path}" ]] || \
    die "canonical QEMU image or immutable launch record is missing"

  current_layer="target-boot"
  serial_log="${state_dir}/uart.log"
  serial_source_log="${serial_log}"
  "${build[@]}" --launch-existing --raw-qemu >"${serial_log}" 2>&1 &
  qemu_pid=$!
  wait_for_marker "${serial_log}" "Cohesix console ready" 240 || \
    die "QEMU did not reach root steady state"

  current_layer="changed-service-ready"
  local ready_marker=${TEST_PLAN_CONVERGENCE_READY_MARKER:-}
  case "${focus}" in
    ninedoor)
      ready_marker=${ready_marker:-"[ninedoor-service] passive child active bootstrap-sc=unbound recovery-reply=installed"}
      ;;
    console-network|live-transport)
      ready_marker=${ready_marker:-"[console-network] isolated child active fault_receiver=active"}
      ;;
    worker)
      ;;
    root-mcs)
      ready_marker=${ready_marker:-"Cohesix console ready"}
      ;;
  esac
  if [[ -n "${ready_marker}" ]]; then
    wait_for_marker "${serial_log}" "${ready_marker}" 120 || \
      die "changed service did not reach its READY marker"
  fi
  wait_for_port 127.0.0.1 "${port}" 60 || \
    die "QEMU TCP console did not become reachable"

  current_layer="real-target-operation"
  case "${focus}" in
    ninedoor)
      operation_script=${TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT:-"${repo_root}/scripts/cohsh/9p_batch.coh"}
      ;;
    worker)
      operation_script=${TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT:-"${repo_root}/scripts/cohsh/converge_worker.coh"}
      ;;
    root-mcs)
      operation_script=${TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT:-"${repo_root}/scripts/cohsh/converge_target_activity.coh"}
      ;;
    *)
      operation_script=${TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT:-"${repo_root}/scripts/cohsh/tcp_basic.coh"}
      ;;
  esac
  local token
  token=${COHSH_AUTH_TOKEN:-$(resolve_auth_token "${repo_root}/configs/root_task.toml")}
  local binary
  binary=$(cohsh_binary "${qemu_out}/host-tools/cohsh")
  local worker_ready_before=0
  if [[ "${focus}" == "worker" ]]; then
    worker_ready_before=$(marker_count \
      "${serial_log}" "WORKER_TASK_READY role=worker-heartbeat")
  fi
  operation_log="${state_dir}/target-operation.log"
  run_live_operation \
    "${binary}" 127.0.0.1 "${port}" "${token}" \
    "${operation_log}" || \
    die "live QEMU operation failed"
  if [[ "${focus}" == "worker" ]]; then
    current_layer="changed-service-ready"
    wait_for_marker_count \
      "${serial_log}" \
      "WORKER_TASK_READY role=worker-heartbeat" \
      "$((worker_ready_before + 2))" \
      120 || \
      die "Worker startup/teardown/restart did not produce two real READY generations"
  fi

  current_layer="unexpected-target-fault"
  if unexpected_faults "${serial_log}"; then
    die "unexpected target fault or panic appeared in QEMU UART"
  fi
  stop_qemu
  write_observation "PASS" "" "root/service readiness and one live operation proved"
}

pi4_canary() {
  profile="pi4_diagnostic / configs/root_task_pi4_uboot_aarch64.toml"
  local target_evidence=${TEST_PLAN_PI4_TARGET_EVIDENCE:-}
  local readback_image=${TEST_PLAN_PI4_READBACK_IMAGE:-}
  local identity_metadata=${TEST_PLAN_PI4_IDENTITY_METADATA:-}
  serial_log=${TEST_PLAN_PI4_SERIAL_LOG:-}
  serial_source_log=${serial_log}
  local host=${TEST_PLAN_PI4_HOST:-}
  local source_digest=${TEST_PLAN_SOURCE_DIGEST:-}
  for required in \
    "${target_evidence}" \
    "${readback_image}" \
    "${identity_metadata}" \
    "${serial_log}" \
    "${host}" \
    "${source_digest}"
  do
    [[ -n "${required}" ]] || \
      blocked "Pi convergence requires target evidence, readback image/metadata, serial log, host, and source identity"
  done
  [[ -f "${target_evidence}" && -f "${readback_image}" && \
     -f "${identity_metadata}" && -f "${serial_log}" ]] || \
    blocked "one or more Pi convergence evidence files are unavailable"

  current_layer="image-source-identity"
  "${repo_root}/scripts/ci/qemu_artifact.py" verify-pi4-evidence \
    --target-evidence "${target_evidence}" \
    --source-digest "${source_digest}" >/dev/null
  python3 "${repo_root}/scripts/pi4_image_identity.py" verify \
    --image "${readback_image}" \
    --metadata "${identity_metadata}" \
    --git-commit "$(git -C "${repo_root}" rev-parse HEAD)" >/dev/null
  image_path=${readback_image}
  image_identity_path=${identity_metadata}

  current_layer="target-boot"
  grep -Fq "Cohesix console ready" "${serial_log}" || \
    die "Pi UART does not contain root steady-state readiness"

  current_layer="changed-service-ready"
  local ready_marker=${TEST_PLAN_CONVERGENCE_READY_MARKER:-}
  if [[ -z "${ready_marker}" && "${focus}" == "pi4-driver" ]]; then
    ready_marker="DRIVER_TASK_SUBSTRATE active=true"
  fi
  if [[ -n "${ready_marker}" ]]; then
    grep -Fq "${ready_marker}" "${serial_log}" || \
      die "Pi UART does not contain the touched service/device marker"
  fi
  if [[ "${focus}" == "pi4-driver" ]]; then
    grep -Fq "failed_count=0" "${serial_log}" || \
      die "Pi driver-task substrate reports a failed child"
  fi
  wait_for_port "${host}" "${COHSH_TCP_PORT:-31337}" 30 || \
    die "Pi TCP console is not reachable"

  current_layer="real-target-operation"
  operation_script=${TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT:-"${repo_root}/scripts/cohsh/converge_target_activity.coh"}
  local before_size
  before_size=$(wc -c <"${serial_log}" | tr -d ' ')
  local binary
  binary=$(cohsh_binary "${repo_root}/out/cohesix/host-tools/cohsh")
  local token
  token=${COHSH_AUTH_TOKEN:-$(resolve_auth_token "${repo_root}/configs/root_task_pi4_uboot_aarch64.toml")}
  run_live_operation \
    "${binary}" "${host}" "${COHSH_TCP_PORT:-31337}" "${token}" \
    "${state_dir}/target-operation.log" || \
    die "live Pi operation failed"

  current_layer="uart-liveness"
  local deadline=$((SECONDS + 15))
  local after_size=${before_size}
  while (( SECONDS < deadline )); do
    after_size=$(wc -c <"${serial_log}" | tr -d ' ')
    if (( after_size > before_size )); then
      break
    fi
    sleep 0.2
  done
  (( after_size > before_size )) || \
    die "Pi UART did not remain live across the target operation"

  current_layer="unexpected-target-fault"
  if unexpected_faults "${serial_log}"; then
    die "unexpected target fault or panic appeared in Pi UART"
  fi
  local serial_snapshot="${state_dir}/uart.snapshot.log"
  cp "${serial_log}" "${serial_snapshot}"
  serial_log=${serial_snapshot}
  write_observation "PASS" "" "exact readback, one boot, device/service liveness, operation, and UART liveness proved"
}

if [[ $# -ne 2 || "$1" != "--target" ]]; then
  usage >&2
  exit 2
fi
target=$2
case "${target}" in
  qemu|pi4)
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
if [[ "${TEST_PLAN_CONVERGENCE:-0}" != "1" ]]; then
  printf 'target canary may run only through the non-claiming convergence runner\n' >&2
  exit 2
fi
if [[ -z "${focus}" || -z "${run_id}" || -z "${state_dir}" || -z "${observation}" ]]; then
  printf 'target canary is missing convergence provenance selectors\n' >&2
  exit 2
fi
trap cleanup EXIT

printf 'NON-CLAIMING TARGET DIAGNOSTIC target=%s focus=%s run=%s\n' \
  "${target}" "${focus}" "${run_id}"
cd "${repo_root}"
if [[ "${target}" == "qemu" ]]; then
  qemu_canary
else
  pi4_canary
fi
