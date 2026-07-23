#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 03 (QEMU TCP regression batch).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 2
tp_stage_begin 3 "qemu-tcp-regression"

target="${TEST_PLAN_TARGET:-qemu}"
case "${target}" in
  qemu)
    action_id="qemu.tcp-regression"
    claim_tier="qemu-integration"
    ;;
  pi4)
    action_id="pi4.tcp-regression"
    claim_tier="pi4-transport"
    ;;
  *)
    tp_log "FAIL  Stage 03 target must be qemu or pi4, got ${target}"
    exit 2
    ;;
esac

selected_groups="${COHSH_BATCH_GROUPS:-all}"
if [[ "${TEST_PLAN_ITERATION:-0}" != "1" && -n "${selected_groups}" && "${selected_groups}" != "all" ]]; then
  tp_mark_incomplete \
    "qemu-regression-subset" \
    "COHSH_BATCH_GROUPS=${selected_groups}" \
    "Only part of the TCP regression matrix was selected; this is useful for focused iteration but cannot produce Stage 03 PASS evidence."
  tp_stage_complete 3
fi

ready_timeout="${TP_STAGE3_READY_TIMEOUT:-900}"
port_timeout="${TP_STAGE3_PORT_TIMEOUT:-60}"
quit_close_timeout="${TP_STAGE3_QUIT_CLOSE_TIMEOUT:-60}"
auth_ready_timeout="${TP_STAGE3_AUTH_READY_TIMEOUT:-120}"

artifact_helper="${TEST_PLAN_ROOT}/scripts/ci/qemu_artifact.py"
stage3_root="${TP_ATTEMPT_DIR}/transport"
transport_root="${stage3_root}/batch"
artifact_root="${stage3_root}/qemu-artifacts"
result_root="${stage3_root}/transport-results"
mkdir -p "${stage3_root}"
target_evidence_input="${TEST_PLAN_TARGET_EVIDENCE_FILE:-${TP_PI4_TARGET_EVIDENCE_FILE:-}}"
target_evidence=""
if [[ "${target}" == "pi4" && -z "${target_evidence_input}" ]]; then
  tp_log "FAIL  Pi 4 Stage 03 requires machine-generated target evidence"
  tp_log "FAIL  set TEST_PLAN_TARGET_EVIDENCE_FILE to a boot/image/source-bound JSON artifact"
  exit 1
fi
if [[ "${target}" == "pi4" ]]; then
  target_evidence="${stage3_root}/target-evidence.json"
  tp_run_cmd \
    "copy-pi4-stage3-target-evidence" \
    "${artifact_helper}" \
    copy-evidence \
    --source "${target_evidence_input}" \
    --output "${target_evidence}"
  tp_run_cmd \
    "verify-pi4-stage3-target-evidence" \
    "${artifact_helper}" \
    verify-pi4-evidence \
    --target-evidence "${target_evidence}" \
    --source-digest "sha256:${TEST_PLAN_SOURCE_DIGEST#sha256:}"
fi

export \
  READY_TIMEOUT="${ready_timeout}" \
  PORT_TIMEOUT="${port_timeout}" \
  QUIT_CLOSE_TIMEOUT="${quit_close_timeout}" \
  AUTH_READY_TIMEOUT="${auth_ready_timeout}" \
  COHSH_LOG_ROOT="${transport_root}" \
  COHSH_QEMU_ARTIFACT_ROOT="${artifact_root}" \
  COHSH_TRANSPORT_RESULT_ROOT="${result_root}" \
  COHSH_REQUIRE_RESULT_EVIDENCE=1 \
  TEST_PLAN_ACTION_ID="${action_id}"
if [[ -n "${target_evidence}" ]]; then
  export TEST_PLAN_TARGET_EVIDENCE_FILE="${target_evidence}"
fi

tp_run_catalog_action "${action_id}"

catalog_digest="sha256:$(
  python3 "${TEST_PLAN_ROOT}/scripts/ci/test_plan_catalog.py" \
    action \
    --id "${action_id}" \
    --field digest
)"
declare -a expected_groups=()
group_selected() {
  local group="$1"
  if [[ -z "${selected_groups}" || "${selected_groups}" == "all" ]]; then
    return 0
  fi
  local normalized="${selected_groups//,/ }"
  local selected
  for selected in ${normalized}; do
    if [[ "${selected}" == "${group}" ]]; then
      return 0
    fi
  done
  return 1
}
for group in base base-telemetry base-shard gated; do
  if group_selected "${group}"; then
    expected_groups+=(--expected-group "${group}")
  fi
done

tp_run_cmd \
  "verify-${action_id}-aggregate" \
  "${artifact_helper}" \
  verify-aggregate \
  --aggregate "${result_root}/stage-03.json" \
  --result-root "${result_root}" \
  --action-id "${action_id}" \
  --catalog-action-digest "${catalog_digest}" \
  --claim-tier "${claim_tier}" \
  --target "${target}" \
  --source-digest "sha256:${TEST_PLAN_SOURCE_DIGEST#sha256:}" \
  --evidence-root "${stage3_root}" \
  "${expected_groups[@]}"

if [[ "${target}" == "qemu" ]]; then
  if group_selected base \
    || group_selected base-telemetry \
    || group_selected base-shard; then
    tp_run_cmd \
      "verify-qemu-base-artifact" \
      "${artifact_helper}" \
      verify \
      --artifact-manifest "${artifact_root}/base/qemu-artifact.json" \
      --source-digest "sha256:${TEST_PLAN_SOURCE_DIGEST#sha256:}" \
      --action-id "${action_id}" \
      --catalog-action-digest "${catalog_digest}"
  fi
  if group_selected gated; then
    tp_run_cmd \
      "verify-qemu-gated-artifact" \
      "${artifact_helper}" \
      verify \
      --artifact-manifest "${artifact_root}/gated/qemu-artifact.json" \
      --source-digest "sha256:${TEST_PLAN_SOURCE_DIGEST#sha256:}" \
      --action-id "${action_id}" \
      --catalog-action-digest "${catalog_digest}"
  fi
fi

if [[ "${TEST_PLAN_ITERATION:-0}" != "1" ]]; then
  "${artifact_helper}" \
    publish-root \
    --state-dir "${TEST_PLAN_STATE_DIR}" \
    --root "${stage3_root}" \
    --pointer "${TEST_PLAN_STATE_DIR}/stage_03_artifact_root.path"
fi
tp_stage_complete 3
