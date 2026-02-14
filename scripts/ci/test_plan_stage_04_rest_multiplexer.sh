#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 04 (REST multiplexer regression batch).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 3
tp_stage_begin 4 "rest-multiplexer"

gateway_url="${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
gateway_auth_token="${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}"
if [[ -z "${gateway_url}" ]]; then
  tp_log "FAIL  missing gateway URL"
  tp_log "set COHESIX_GATEWAY_URL (or HIVE_GATEWAY_URL/COHSH_REST_URL/COH_REST_URL) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-url=${gateway_url}"
if [[ -z "${gateway_auth_token}" ]]; then
  tp_log "FAIL  missing gateway request auth token"
  tp_log "set HIVE_GATEWAY_REQUEST_AUTH_TOKEN (or COHSH_REST_AUTH_TOKEN/COH_REST_AUTH_TOKEN) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-auth-token=present"

tp_run_cmd "cohsh-rest-regression-batch" "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

if [[ "${TP_SKIP_PYTHON:-0}" == "1" ]]; then
  tp_log "SKIP  python-rest-smoke (TP_SKIP_PYTHON=1)"
else
  python_bin="${TP_PYTHON_BIN:-python3}"
  tp_run_shell "python-rest-smoke" \
    "COHESIX_GATEWAY_URL=\"${gateway_url}\" HIVE_GATEWAY_REQUEST_AUTH_TOKEN=\"${gateway_auth_token}\" \"${python_bin}\" - <<'PY'
import os
import sys
from pathlib import Path

repo_root = Path.cwd()
sys.path.insert(0, str(repo_root / \"tools\" / \"cohesix-py\"))

from cohesix.backends import RestBackend

gateway_url = os.environ[\"COHESIX_GATEWAY_URL\"]
backend = RestBackend(gateway_url)

root_entries = backend.list_dir(\"/\")
if not root_entries:
    raise SystemExit(\"REST smoke failed: root listing is empty\")
log_payload = backend.read_file(\"/log/queen.log\", 4096)
print(f\"python-rest-smoke root_entries={len(root_entries)} log_bytes={len(log_payload)}\")
PY"
fi

tp_stage_complete 4
