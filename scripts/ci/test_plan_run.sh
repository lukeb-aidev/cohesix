#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run target-qualified Cohesix test-plan scripts with deterministic stage progression.
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/../.." && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

usage() {
  cat <<'USAGE'
Usage: scripts/ci/test_plan_run.sh [--target qemu|pi4] [--state-dir <path>] [--stage <1..5>] [--iteration] [--list]

Runs the scripted test plan stages in order:
  1 integrity
  2 host-fast
  3 qemu-tcp-regression
  4 rest-multiplexer
  5 due-diligence

Options:
  --target <name>     Target under test: qemu or pi4. Defaults to qemu for legacy invocations.
  --state-dir <path>  Shared state/log directory for stage markers and logs.
  --stage <n>         Run exactly one stage (requires previous stage markers for n>1).
  --iteration         Focused rerun mode for a single stage. Writes iteration markers only.
  --list              Print stage map and exit.
  --help              Show this help.

Environment pass-through:
  TEST_PLAN_TARGET
  TEST_PLAN_STATE_DIR
  COHESIX_GATEWAY_URL / HIVE_GATEWAY_URL / COHSH_REST_URL / COH_REST_URL
  COHSH_BATCH_TARGET / COHSH_TCP_HOST / COHSH_TCP_PORT
  TP_STAGE4_GATEWAY_BIND / TP_STAGE4_QEMU_TCP_PORT
  TEST_PLAN_ITERATION
  TP_SKIP_GENERATED_CHECK, TP_SKIP_PYTHON, TP_SKIP_FUSE, TP_WRITE_TRACE_FIXTURES

Target contract:
  - qemu supports stages 1-5, including self-contained QEMU Stage 03/04 evidence.
  - pi4 supports stages 1-5, but Stage 03 requires COHSH_TCP_HOST or COHSH_HOST
    for a live Pi 4 TCP console, and Stage 04 requires COHESIX_GATEWAY_URL or an
    equivalent existing REST gateway URL so the stage cannot start local QEMU.
  - The state dir records target.env and stage_XX.<target>.done markers.
  - A target-qualified PASS requires stage_01.<target>.done through
    stage_05.<target>.done, generic stage_01.done through stage_05.done, and no
    stage_*.incomplete marker or incomplete/ record.

Notes:
  - TP_SKIP_* options record an INCOMPLETE marker and the stage fails (they are for local iteration only).
  - --iteration is for focused debugging only; it never writes stage_XX.done or stage_XX.<target>.done.
USAGE
}

list_stages() {
  cat <<'STAGES'
1  scripts/ci/test_plan_stage_01_integrity.sh
2  scripts/ci/test_plan_stage_02_host_fast.sh
3  scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh
4  scripts/ci/test_plan_stage_04_rest_multiplexer.sh
5  scripts/ci/test_plan_stage_05_due_diligence.sh

targets:
qemu  stages 1 2 3 4 5
pi4   stages 1 2 3 4 5  (stage 3 requires COHSH_TCP_HOST/COHSH_HOST; stage 4 requires COHESIX_GATEWAY_URL/HIVE_GATEWAY_URL/COHSH_REST_URL/COH_REST_URL)

state-dir target metadata:
target.env
stage_01.qemu.done / stage_01.pi4.done
stage_01.inputs.sha256
stage_01.qemu.iteration / stage_01.pi4.iteration
STAGES
}

state_dir="${TEST_PLAN_STATE_DIR:-${repo_root}/out/test-plan/$(date -u +%Y%m%dT%H%M%SZ)}"
single_stage=""
target="${TEST_PLAN_TARGET:-qemu}"
iteration="${TEST_PLAN_ITERATION:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      shift
      [[ $# -gt 0 ]] || {
        echo "--target requires a value" >&2
        exit 2
      }
      target="$1"
      ;;
    --state-dir)
      shift
      [[ $# -gt 0 ]] || {
        echo "--state-dir requires a value" >&2
        exit 2
      }
      state_dir="$1"
      ;;
    --stage)
      shift
      [[ $# -gt 0 ]] || {
        echo "--stage requires a value" >&2
        exit 2
      }
      single_stage="$1"
      ;;
    --iteration)
      iteration="1"
      ;;
    --list)
      list_stages
      exit 0
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

case "${target}" in
  qemu|pi4)
    ;;
  *)
    echo "invalid --target value: ${target} (expected qemu or pi4)" >&2
    exit 2
    ;;
esac

case "${iteration}" in
  0|1)
    ;;
  *)
    echo "invalid TEST_PLAN_ITERATION value: ${iteration} (expected 0 or 1)" >&2
    exit 2
    ;;
esac

stage_script_path() {
  local stage="$1"
  case "${stage}" in
    1) printf "%s/scripts/ci/test_plan_stage_01_integrity.sh" "${repo_root}" ;;
    2) printf "%s/scripts/ci/test_plan_stage_02_host_fast.sh" "${repo_root}" ;;
    3) printf "%s/scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh" "${repo_root}" ;;
    4) printf "%s/scripts/ci/test_plan_stage_04_rest_multiplexer.sh" "${repo_root}" ;;
    5) printf "%s/scripts/ci/test_plan_stage_05_due_diligence.sh" "${repo_root}" ;;
    *) return 1 ;;
  esac
}

target_stage_marker() {
  local stage="$1"
  printf "%s/stage_%02d.%s.done" "${state_dir}" "${stage}" "${target}"
}

target_stage_iteration_marker() {
  local stage="$1"
  printf "%s/stage_%02d.%s.iteration" "${state_dir}" "${stage}" "${target}"
}

existing_gateway_url() {
  printf "%s" "${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
}

pi4_tcp_host() {
  printf "%s" "${COHSH_TCP_HOST:-${COHSH_HOST:-}}"
}

is_loopback_host() {
  case "$1" in
    127.0.0.1|localhost|::1)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

validate_target_stage() {
  local stage="$1"
  if [[ "${target}" != "pi4" ]]; then
    return 0
  fi

  case "${stage}" in
    3)
      local host
      host="$(pi4_tcp_host)"
      if [[ -z "${host}" ]]; then
        echo "pi4 stage 03 requires COHSH_TCP_HOST or COHSH_HOST for the live Pi 4 TCP console" >&2
        return 1
      fi
      if is_loopback_host "${host}" && [[ "${TP_PI4_ALLOW_LOOPBACK:-0}" != "1" ]]; then
        echo "pi4 stage 03 refuses loopback host ${host}; set TP_PI4_ALLOW_LOOPBACK=1 only for an intentional local tunnel" >&2
        return 1
      fi
      ;;
    4)
      if [[ -z "$(existing_gateway_url)" ]]; then
        echo "pi4 stage 04 requires COHESIX_GATEWAY_URL, HIVE_GATEWAY_URL, COHSH_REST_URL, or COH_REST_URL" >&2
        echo "without an existing gateway, stage 04 would start local QEMU and create misleading Pi 4 evidence" >&2
        return 1
      fi
      ;;
  esac
}

require_previous_target_markers() {
  local stage="$1"
  local previous
  for ((previous = 1; previous < stage; previous += 1)); do
    local marker
    marker="$(target_stage_marker "${previous}")"
    if [[ ! -f "${marker}" ]]; then
      echo "missing target-qualified stage marker: ${marker}" >&2
      echo "run stage $(printf "%02d" "${previous}") first with --target ${target} --state-dir ${state_dir}" >&2
      return 1
    fi
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    if ! tp_assert_stage_fingerprint_fresh "${previous}"; then
      return 1
    fi
  done
}

write_target_metadata() {
  local started_at
  started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  local metadata="${state_dir}/target.env"
  local existing_target=""
  if [[ -f "${metadata}" ]]; then
    existing_target="$(sed -n 's/^TEST_PLAN_TARGET=//p' "${metadata}" | tail -n 1)"
    if [[ -n "${existing_target}" && "${existing_target}" != "${target}" ]]; then
      echo "state dir target mismatch: ${metadata} records ${existing_target}, requested ${target}" >&2
      exit 1
    fi
  fi
  {
    printf "TEST_PLAN_TARGET=%s\n" "${target}"
    printf "TEST_PLAN_TARGET_MATRIX_VERSION=1\n"
    printf "TEST_PLAN_STATE_DIR=%s\n" "${state_dir}"
    printf "TEST_PLAN_REPO_ROOT=%s\n" "${repo_root}"
    printf "TEST_PLAN_STARTED_AT_UTC=%s\n" "${started_at}"
  } >"${metadata}"
}

assert_required_artifacts() {
  local stage="$1"
  local log_path="${state_dir}/logs/$(printf "stage-%02d" "${stage}")"
  case "${stage}" in
    1)
      [[ -s "${state_dir}/target.env" ]] || {
        echo "missing target metadata: ${state_dir}/target.env" >&2
        return 1
      }
      compgen -G "${log_path}-integrity.log" >/dev/null || {
        echo "missing stage 01 log under ${state_dir}/logs" >&2
        return 1
      }
      ;;
    2)
      compgen -G "${log_path}-host-fast.log" >/dev/null || {
        echo "missing stage 02 log under ${state_dir}/logs" >&2
        return 1
      }
      ;;
    3)
      if [[ "${target}" == "pi4" ]]; then
        local summary="${state_dir}/qemu-regression-logs/summary.log"
        [[ -s "${summary}" ]] || {
          echo "missing Pi 4 regression summary: ${summary}" >&2
          return 1
        }
        grep -F "INFO target=pi4" "${summary}" >/dev/null || {
          echo "Pi 4 regression summary does not prove target=pi4: ${summary}" >&2
          return 1
        }
      else
        [[ -d "${state_dir}/qemu-regression-logs" ]] || {
          echo "missing QEMU regression log directory: ${state_dir}/qemu-regression-logs" >&2
          return 1
        }
      fi
      ;;
    4)
      [[ -d "${state_dir}/rest-regression-logs" ]] || {
        echo "missing REST regression log directory: ${state_dir}/rest-regression-logs" >&2
        return 1
      }
      ;;
    5)
      compgen -G "${log_path}-due-diligence.log" >/dev/null || {
        echo "missing stage 05 log under ${state_dir}/logs" >&2
        return 1
      }
      if [[ "${target}" == "pi4" ]]; then
        local proof="${PI4_RUNTIME_DMA_PROOF_FILE:-${state_dir}/pi4-runtime-dma-proof.env}"
        [[ -s "${proof}" ]] || {
          echo "missing Pi 4 runtime/DMA proof artifact: ${proof}" >&2
          return 1
        }
        grep -Fx "PI4_RUNTIME_DMA_PROOF=fresh-pi" "${proof}" >/dev/null || {
          echo "Pi 4 runtime/DMA proof is not fresh-pi: ${proof}" >&2
          return 1
        }
        grep -Fx "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified" "${proof}" >/dev/null || {
          echo "Pi 4 runtime/DMA proof is not counter-qualified: ${proof}" >&2
          return 1
        }
      fi
      ;;
  esac
}

write_target_stage_marker() {
  local stage="$1"
  local marker
  marker="$(target_stage_marker "${stage}")"
  date -u +"%Y-%m-%dT%H:%M:%SZ" >"${marker}"
}

write_target_stage_iteration_marker() {
  local stage="$1"
  local marker
  marker="$(target_stage_iteration_marker "${stage}")"
  date -u +"%Y-%m-%dT%H:%M:%SZ" >"${marker}"
}

assert_no_incomplete_markers() {
  if compgen -G "${state_dir}/stage_*.incomplete" >/dev/null; then
    echo "target-qualified PASS blocked by stage incomplete marker(s): ${state_dir}/stage_*.incomplete" >&2
    return 1
  fi
  if [[ -d "${state_dir}/incomplete" ]] && find "${state_dir}/incomplete" -type f -print -quit | grep -q .; then
    echo "target-qualified PASS blocked by incomplete records under ${state_dir}/incomplete" >&2
    return 1
  fi
}

assert_full_target_pass() {
  local stage
  assert_no_incomplete_markers
  for stage in 1 2 3 4 5; do
    [[ -f "${state_dir}/stage_$(printf "%02d" "${stage}").done" ]] || {
      echo "missing generic PASS marker for stage ${stage}: ${state_dir}/stage_$(printf "%02d" "${stage}").done" >&2
      return 1
    }
    [[ -f "$(target_stage_marker "${stage}")" ]] || {
      echo "missing target-qualified PASS marker for stage ${stage}: $(target_stage_marker "${stage}")" >&2
      return 1
    }
  done
}

declare -a stages
if [[ -n "${single_stage}" ]]; then
  if ! stage_script_path "${single_stage}" >/dev/null; then
    echo "invalid --stage value: ${single_stage}" >&2
    exit 2
  fi
  stages=("${single_stage}")
else
  stages=(1 2 3 4 5)
fi

if [[ "${iteration}" == "1" && -z "${single_stage}" ]]; then
  echo "--iteration requires --stage <1..5>" >&2
  exit 2
fi

mkdir -p "${state_dir}"
write_target_metadata
echo "[test-plan] root: ${repo_root}"
echo "[test-plan] state-dir: ${state_dir}"
echo "[test-plan] target: ${target}"
if [[ "${iteration}" == "1" ]]; then
  echo "[test-plan] iteration: yes"
fi

for stage in "${stages[@]}"; do
  if ! validate_target_stage "${stage}"; then
    exit 2
  fi
  if [[ "${stage}" -gt 1 ]]; then
    require_previous_target_markers "${stage}"
  fi
  script_path="$(stage_script_path "${stage}")"
  if [[ ! -x "${script_path}" ]]; then
    echo "stage script is missing or not executable: ${script_path}" >&2
    exit 1
  fi
  echo "[test-plan] running stage ${stage}: ${script_path}"
  TEST_PLAN_STATE_DIR="${state_dir}" \
    TEST_PLAN_TARGET="${target}" \
    TEST_PLAN_ITERATION="${iteration}" \
    COHSH_BATCH_TARGET="${target}" \
    "${script_path}"
  if [[ "${iteration}" == "1" ]]; then
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    iteration_marker="$(tp_stage_iteration_marker "${stage}")"
    if [[ ! -f "${iteration_marker}" ]]; then
      echo "missing iteration marker for stage ${stage}: ${iteration_marker}" >&2
      exit 1
    fi
    write_target_stage_iteration_marker "${stage}"
    continue
  fi
  assert_required_artifacts "${stage}"
  write_target_stage_marker "${stage}"
done

if [[ -z "${single_stage}" ]]; then
  assert_full_target_pass
fi

echo "[test-plan] completed stages: ${stages[*]}"
echo "[test-plan] logs: ${state_dir}/logs"
