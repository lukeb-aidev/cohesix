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
Usage: scripts/ci/test_plan_run.sh [--target qemu|pi4] [--state-dir <path>] [--stage <1..5>] [--resume|--force] [--iteration] [--reuse-common-from <state-dir>] [--list]

Runs the scripted test plan stages in order:
  1 common-hermetic (integrity plus host suites)
  2 provisioned-target
  3 qemu-tcp-regression
  4 rest-multiplexer
  5 due-diligence

Options:
  --target <name>     Target under test: qemu or pi4. Defaults to qemu for legacy invocations.
  --state-dir <path>  Shared state/log directory for stage markers and logs.
  --stage <n>         Run exactly one stage (requires previous stage markers for n>1).
  --iteration         Focused rerun mode for a single stage. Writes iteration markers only.
  --resume            Verify and reuse valid evidence; this is the default mode.
  --force             Rerun selected stages after preserving prior immutable evidence.
  --reuse-common-from <state-dir>
                      Import exact, verified common-stage evidence. Stage 01 only;
                      Stage 02 remains target-bound because it includes provisioned actions.
  --list              Print stage map and exit.
  --help              Show this help.

Environment pass-through:
  TEST_PLAN_TARGET
  TEST_PLAN_STATE_DIR
  COHESIX_GATEWAY_URL / HIVE_GATEWAY_URL / COHSH_REST_URL / COH_REST_URL
  COHSH_BATCH_TARGET / COHSH_TCP_HOST / COHSH_TCP_PORT
  TP_STAGE4_GATEWAY_BIND / TP_STAGE4_QEMU_TCP_PORT
  TP_HOST_JOBS / TP_UI_WORKERS / TP_ALLOW_OVERSUBSCRIBE
  TEST_PLAN_ITERATION
  TEST_PLAN_FORCE
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
  - Valid attestations resume by default. Stale evidence fails closed unless --force is used.
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

stage roles:
1  common-hermetic (integrity + host actions; cross-target reusable)
2  provisioned-target (never cross-target reused)

state-dir target metadata:
target.env
stage_01.qemu.done / stage_01.pi4.done
stage_01.inputs.sha256
stage_01.qemu.iteration / stage_01.pi4.iteration
stage_01.attestation.json / stage_01.<target>.attestation.json
evidence/attempts/ / evidence/iterations/ / evidence/imports/sha256/
STAGES
}

state_dir="${TEST_PLAN_STATE_DIR:-${repo_root}/out/test-plan/$(date -u +%Y%m%dT%H%M%SZ)-$$}"
single_stage=""
target="${TEST_PLAN_TARGET:-qemu}"
iteration="${TEST_PLAN_ITERATION:-0}"
force="${TEST_PLAN_FORCE:-0}"
resume_explicit=0
reuse_common_from=""

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
    --force)
      force="1"
      ;;
    --resume)
      resume_explicit="1"
      ;;
    --reuse-common-from)
      shift
      [[ $# -gt 0 ]] || {
        echo "--reuse-common-from requires a value" >&2
        exit 2
      }
      reuse_common_from="$1"
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

case "${force}" in
  0|1)
    ;;
  *)
    echo "invalid TEST_PLAN_FORCE value: ${force} (expected 0 or 1)" >&2
    exit 2
    ;;
esac

if [[ "${iteration}" == "1" && "${force}" == "1" ]]; then
  echo "--iteration and --force are mutually exclusive" >&2
  exit 2
fi

if [[ "${resume_explicit}" == "1" && "${force}" == "1" ]]; then
  echo "--resume and --force are mutually exclusive" >&2
  exit 2
fi

if [[ "${iteration}" == "1" && -n "${reuse_common_from}" ]]; then
  echo "--iteration and --reuse-common-from are mutually exclusive" >&2
  exit 2
fi

if [[ "${force}" == "1" && -n "${reuse_common_from}" ]]; then
  echo "--force and --reuse-common-from are mutually exclusive" >&2
  exit 2
fi

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

target_stage_iteration_attestation_ref() {
  local stage="$1"
  printf "%s/stage_%02d.%s.iteration.attestation.json" \
    "${state_dir}" \
    "${stage}" \
    "${target}"
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

verified_target_stages=()

mark_target_stage_verified() {
  verified_target_stages["$1"]=1
}

target_stage_was_verified() {
  [[ "${verified_target_stages[$1]:-0}" == "1" ]]
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
    if target_stage_was_verified "${previous}"; then
      continue
    fi
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    TEST_PLAN_TARGET="${target}"
    if ! tp_verify_stage_attestation "${state_dir}" "${previous}" "${target}"; then
      echo "missing or stale target-qualified attestation for stage $(printf "%02d" "${previous}")" >&2
      echo "run stage $(printf "%02d" "${previous}") first with --target ${target} --state-dir ${state_dir}" >&2
      return 1
    fi
    mark_target_stage_verified "${previous}"
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
    local existing_root
    existing_root="$(sed -n 's/^TEST_PLAN_REPO_ROOT=//p' "${metadata}" | tail -n 1)"
    if [[ -n "${existing_root}" && "${existing_root}" != "${repo_root}" ]]; then
      echo "state dir repository mismatch: ${metadata} records ${existing_root}, current ${repo_root}" >&2
      exit 1
    fi
    return 0
  fi
  {
    printf "TEST_PLAN_TARGET=%s\n" "${target}"
    printf "TEST_PLAN_TARGET_MATRIX_VERSION=2\n"
    printf "TEST_PLAN_STATE_DIR=%s\n" "${state_dir}"
    printf "TEST_PLAN_REPO_ROOT=%s\n" "${repo_root}"
    printf "TEST_PLAN_STARTED_AT_UTC=%s\n" "${started_at}"
  } >"${metadata}"
}

assert_required_artifacts() {
  local stage="$1"
  case "${stage}" in
    1)
      [[ -s "${state_dir}/target.env" ]] || {
        echo "missing target metadata: ${state_dir}/target.env" >&2
        return 1
      }
      ;;
    2)
      ;;
    3)
      local stage3_root
      stage3_root="$(stage_artifact_root 3)"
      [[ -s "${stage3_root}/transport-results/stage-03.json" ]] || {
        echo "missing Stage 03 aggregate result: ${stage3_root}/transport-results/stage-03.json" >&2
        return 1
      }
      if [[ ! -d "${stage3_root}/batch" ]] \
        || ! find "${stage3_root}/batch" -type f -size +0 -print -quit | grep -q .
      then
        echo "missing immutable Stage 03 batch logs: ${stage3_root}/batch" >&2
        return 1
      fi
      if [[ "${target}" == "pi4" ]]; then
        [[ -s "${stage3_root}/target-evidence.json" ]] || {
          echo "missing Pi 4 target evidence: ${stage3_root}/target-evidence.json" >&2
          return 1
        }
        [[ -s "${stage3_root}/batch/summary.log" ]] || {
          echo "missing Pi 4 regression summary: ${stage3_root}/batch/summary.log" >&2
          return 1
        }
      else
        [[ -s "${stage3_root}/qemu-artifacts/base/qemu-artifact.json" ]] || {
          echo "missing Stage 03 base QEMU artifact manifest" >&2
          return 1
        }
        [[ -s "${stage3_root}/qemu-artifacts/gated/qemu-artifact.json" ]] || {
          echo "missing Stage 03 gated QEMU artifact manifest" >&2
          return 1
        }
      fi
      ;;
    4)
      local stage4_root
      stage4_root="$(stage_artifact_root 4)"
      [[ -s "${stage4_root}/results/stage-04.json" ]] || {
        echo "missing Stage 04 aggregate result: ${stage4_root}/results/stage-04.json" >&2
        return 1
      }
      [[ -s "${stage4_root}/results/summary.log" ]] || {
        echo "missing Stage 04 summary: ${stage4_root}/results/summary.log" >&2
        return 1
      }
      [[ -d "${stage4_root}/regression-logs" ]] || {
        echo "missing Stage 04 immutable REST logs: ${stage4_root}/regression-logs" >&2
        return 1
      }
      local rest_log_count
      rest_log_count="$(
        find "${stage4_root}/regression-logs" \
          -type f \
          -size +0 \
          \( -name '*.log' -o -name '*.out' \) \
          2>/dev/null |
          wc -l |
          tr -d ' '
      )"
      if [[ "${rest_log_count}" -lt 4 ]]; then
        echo "Stage 04 requires four immutable REST script logs; found ${rest_log_count}" >&2
        return 1
      fi
      if [[ "${target}" == "pi4" || -n "$(existing_gateway_url)" ]]; then
        [[ -s "${stage4_root}/target-evidence.json" ]] || {
          echo "missing external target evidence: ${stage4_root}/target-evidence.json" >&2
          return 1
        }
      fi
      ;;
    5)
      local stage5_root
      stage5_root="$(stage_artifact_root 5)"
      local audit_root="${stage5_root}/audit"
      local audit_log
      for audit_log in \
        cargo-audit-version.log \
        cargo-audit.log \
        cargo-deny-version.log \
        cargo-deny-advisories.log
      do
        [[ -s "${audit_root}/${audit_log}" ]] || {
          echo "missing immutable Stage 05 audit log: ${audit_root}/${audit_log}" >&2
          return 1
        }
      done
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

stage_artifact_root() {
  local stage="$1"
  local pointer="${state_dir}/stage_$(printf "%02d" "${stage}")_artifact_root.path"
  if [[ ! -s "${pointer}" ]]; then
    echo "missing immutable Stage $(printf "%02d" "${stage}") artifact pointer: ${pointer}" >&2
    return 1
  fi
  TEST_PLAN_ROOT="${repo_root}"
  TEST_PLAN_STATE_DIR="${state_dir}"
  tp_resolve_state_pointer "${state_dir}" "${pointer}"
}

required_artifact_paths() {
  local stage="$1"
  case "${stage}" in
    1)
      printf "%s\n" "${state_dir}/target.env"
      ;;
    3)
      stage_artifact_root 3
      ;;
    4)
      stage_artifact_root 4
      ;;
    5)
      stage_artifact_root 5
      if [[ "${target}" == "pi4" ]]; then
        printf "%s\n" "${PI4_RUNTIME_DMA_PROOF_FILE:-${state_dir}/pi4-runtime-dma-proof.env}"
      fi
      ;;
  esac
}

write_target_stage_iteration_marker() {
  local stage="$1"
  local generic_ref
  generic_ref="$(tp_stage_iteration_attestation_ref "${state_dir}" "${stage}")"
  local manifest
  manifest="$(tp_ref_manifest_path "${state_dir}" "${generic_ref}")"
  tp_atomic_ref_write \
    "${state_dir}" \
    "$(target_stage_iteration_attestation_ref "${stage}")" \
    "${manifest}"
  tp_atomic_marker_write \
    "$(target_stage_iteration_marker "${stage}")" \
    "${manifest}"
}

qualify_target_stage() {
  local stage="$1"
  local -a artifacts=()
  local artifact
  while IFS= read -r artifact; do
    [[ -n "${artifact}" ]] || continue
    artifacts+=("${artifact}")
  done < <(required_artifact_paths "${stage}")
  TEST_PLAN_ROOT="${repo_root}"
  TEST_PLAN_STATE_DIR="${state_dir}"
  TEST_PLAN_TARGET="${target}"
  if [[ "${#artifacts[@]}" -gt 0 ]]; then
    tp_qualify_stage_attestation \
      "${state_dir}" \
      "${stage}" \
      "${target}" \
      "${artifacts[@]}"
  else
    tp_qualify_stage_attestation \
      "${state_dir}" \
      "${stage}" \
      "${target}"
  fi
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
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    TEST_PLAN_TARGET="${target}"
    tp_verify_stage_attestation "${state_dir}" "${stage}" "${target}"
  done
}

stage_has_active_evidence() {
  local stage="$1"
  local candidate
  for candidate in \
    "$(tp_stage_attestation_ref "${state_dir}" "${stage}")" \
    "$(tp_stage_target_attestation_ref "${state_dir}" "${stage}" "${target}")" \
    "${state_dir}/stage_$(printf "%02d" "${stage}").done" \
    "$(target_stage_marker "${stage}")" \
    "${state_dir}/stage_$(printf "%02d" "${stage}").inputs.sha256" \
    "${state_dir}/stage_$(printf "%02d" "${stage}")_artifact_root.path" \
    "$(tp_stage_pending_ref "${state_dir}" "${stage}" full)"
  do
    [[ -e "${candidate}" ]] && return 0
  done
  return 1
}

invalidate_downstream_active_evidence() {
  local stage="$1"
  local downstream
  local pending
  for ((downstream = stage + 1; downstream <= 5; downstream += 1)); do
    pending="$(tp_stage_pending_ref "${state_dir}" "${downstream}" full)"
    if [[ -e "${pending}" ]]; then
      echo "cannot invalidate stage ${downstream}; active attempt: ${pending}" >&2
      return 1
    fi
  done
  for ((downstream = stage + 1; downstream <= 5; downstream += 1)); do
    rm -f \
      "$(tp_stage_attestation_ref "${state_dir}" "${downstream}")" \
      "$(tp_stage_target_attestation_ref \
        "${state_dir}" \
        "${downstream}" \
        "${target}")" \
      "${state_dir}/stage_$(printf "%02d" "${downstream}").done" \
      "$(target_stage_marker "${downstream}")" \
      "${state_dir}/stage_$(printf "%02d" "${downstream}").inputs.sha256" \
      "${state_dir}/stage_$(printf "%02d" "${downstream}")_artifact_root.path"
  done
}

import_and_qualify_common_stage() {
  local stage="$1"
  if [[ -z "${reuse_common_from}" ]]; then
    return 1
  fi
  if [[ "$(tp_stage_scope "${stage}")" != "common" ]]; then
    echo "stage $(printf "%02d" "${stage}") is target-bound and cannot be imported from ${reuse_common_from}" >&2
    return 2
  fi
  invalidate_downstream_active_evidence "${stage}"
  echo "[test-plan] importing common stage ${stage} from ${reuse_common_from}"
  TEST_PLAN_ROOT="${repo_root}"
  TEST_PLAN_STATE_DIR="${state_dir}"
  TEST_PLAN_TARGET="${target}"
  tp_import_common_stage_attestation \
    "${reuse_common_from}" \
    "${state_dir}" \
    "${stage}" \
    "${target}"
  assert_required_artifacts "${stage}"
  qualify_target_stage "${stage}"
  mark_target_stage_verified "${stage}"
}

ensure_common_prerequisites() {
  local stage="$1"
  [[ -n "${reuse_common_from}" ]] || return 0
  local previous
  for ((previous = 1; previous < stage; previous += 1)); do
    [[ "$(tp_stage_scope "${previous}")" == "common" ]] || continue
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    TEST_PLAN_TARGET="${target}"
    if tp_verify_stage_attestation "${state_dir}" "${previous}" "${target}" 2>/dev/null; then
      mark_target_stage_verified "${previous}"
      continue
    fi
    if stage_has_active_evidence "${previous}"; then
      echo "destination has invalid active evidence for common stage $(printf "%02d" "${previous}")" >&2
      echo "use --force to rerun it or choose a fresh state directory" >&2
      return 1
    fi
    import_and_qualify_common_stage "${previous}"
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

if [[ -n "${reuse_common_from}" ]]; then
  if [[ ! -d "${reuse_common_from}" ]]; then
    echo "common evidence state directory does not exist: ${reuse_common_from}" >&2
    exit 2
  fi
  reuse_common_from="$(cd "${reuse_common_from}" && pwd)"
fi

mkdir -p "${state_dir}"
state_dir="$(cd "${state_dir}" && pwd)"
if [[ -n "${reuse_common_from}" && "${reuse_common_from}" == "${state_dir}" ]]; then
  echo "--reuse-common-from must name a different state directory" >&2
  exit 2
fi
TEST_PLAN_ROOT="${repo_root}"
TEST_PLAN_STATE_DIR="${state_dir}"
TEST_PLAN_TARGET="${target}"
export CARGO_INCREMENTAL=0
export COHSH_REQUIRE_RESULT_EVIDENCE=1
export TEST_PLAN_STAGED_RUN=1
export TP_PYTHON_PLAYBOOK_OUT="${state_dir}/python-playbooks"
tp_init
export TEST_PLAN_RUNNER_LOCK_HELD=1 TEST_PLAN_LOCK_OWNER_ID
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
  ensure_common_prerequisites "${stage}"
  if [[ "${stage}" -gt 1 ]]; then
    require_previous_target_markers "${stage}"
  fi

  if [[ "${iteration}" != "1" && "${force}" != "1" ]]; then
    TEST_PLAN_ROOT="${repo_root}"
    TEST_PLAN_STATE_DIR="${state_dir}"
    TEST_PLAN_TARGET="${target}"
    target_ref="$(tp_stage_target_attestation_ref "${state_dir}" "${stage}" "${target}")"
    existing_evidence_kind=""
    if [[ -e "${target_ref}" ]]; then
      if tp_verify_stage_attestation "${state_dir}" "${stage}" "${target}" 2>/dev/null; then
        existing_evidence_kind="target"
      else
        echo "stage $(printf "%02d" "${stage}") has stale or corrupt target-qualified evidence" >&2
        echo "rerun with --force or choose a fresh state directory" >&2
        exit 1
      fi
    else
      generic_ref="$(tp_stage_attestation_ref "${state_dir}" "${stage}")"
    fi
    if [[ -z "${existing_evidence_kind}" && -s "${generic_ref:-}" ]]; then
      if ! tp_verify_stage_attestation "${state_dir}" "${stage}"; then
        echo "stage $(printf "%02d" "${stage}") has stale or corrupt generic evidence" >&2
        echo "rerun with --force or choose a fresh state directory" >&2
        exit 1
      fi
      existing_evidence_kind="generic"
    fi

    if [[ -n "${existing_evidence_kind}" && "${stage}" == "5" ]]; then
      echo "[test-plan] refresh stage 5: advisory and governance evidence is time-sensitive"
    elif [[ "${existing_evidence_kind}" == "target" ]]; then
      mark_target_stage_verified "${stage}"
      echo "[test-plan] resume stage ${stage}: verified attestation"
      continue
    elif [[ "${existing_evidence_kind}" == "generic" ]]; then
      assert_required_artifacts "${stage}"
      qualify_target_stage "${stage}"
      mark_target_stage_verified "${stage}"
      echo "[test-plan] resume stage ${stage}: qualified existing generic attestation"
      continue
    elif stage_has_active_evidence "${stage}"; then
      echo "stage $(printf "%02d" "${stage}") has incomplete, stale, or legacy active evidence" >&2
      echo "rerun with --force or choose a fresh state directory" >&2
      exit 1
    elif [[ "$(tp_stage_scope "${stage}")" == "common" && -n "${reuse_common_from}" ]]; then
      import_and_qualify_common_stage "${stage}"
      echo "[test-plan] resume stage ${stage}: imported verified common evidence"
      continue
    fi
  fi

  script_path="$(stage_script_path "${stage}")"
  if [[ ! -x "${script_path}" ]]; then
    echo "stage script is missing or not executable: ${script_path}" >&2
    exit 1
  fi
  if [[ "${iteration}" != "1" ]]; then
    invalidate_downstream_active_evidence "${stage}"
  fi
  echo "[test-plan] running stage ${stage}: ${script_path}"
  stage_exit_code=0
  if TEST_PLAN_STATE_DIR="${state_dir}" \
    TEST_PLAN_TARGET="${target}" \
    TEST_PLAN_ITERATION="${iteration}" \
    TEST_PLAN_FORCE="${force}" \
    TEST_PLAN_STAGED_RUN=1 \
    COHSH_BATCH_TARGET="${target}" \
    "${script_path}"; then
    stage_exit_code=0
  else
    stage_exit_code=$?
  fi
  if [[ "${stage_exit_code}" -ne 0 ]]; then
    mode="full"
    [[ "${iteration}" == "1" ]] && mode="iteration"
    tp_finalize_pending_attempt \
      "${state_dir}" \
      "${stage}" \
      "${mode}" \
      failed || true
    echo "[test-plan] stage ${stage} failed with exit ${stage_exit_code}" >&2
    exit "${stage_exit_code}"
  fi
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
  TEST_PLAN_ROOT="${repo_root}"
  TEST_PLAN_STATE_DIR="${state_dir}"
  TEST_PLAN_TARGET="${target}"
  if ! tp_verify_stage_attestation "${state_dir}" "${stage}"; then
    echo "stage ${stage} completed without a valid generic attestation" >&2
    exit 1
  fi
  assert_required_artifacts "${stage}"
  qualify_target_stage "${stage}"
  mark_target_stage_verified "${stage}"
done

if [[ -z "${single_stage}" ]]; then
  assert_full_target_pass
fi

echo "[test-plan] completed stages: ${stages[*]}"
echo "[test-plan] logs: ${state_dir}/logs"
