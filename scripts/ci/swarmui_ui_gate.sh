#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run deterministic SwarmUI Playwright coverage with lockfile-bound browsers and retained evidence.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=scripts/ci/test_plan_resources.sh
source "${repo_root}/scripts/ci/test_plan_resources.sh"
tp_configure_resource_limits
ui_root="${repo_root}/tools/swarmui-ui-tests"
lockfile="${ui_root}/package-lock.json"
evidence_dir="${SWARMUI_UI_EVIDENCE_DIR:-${repo_root}/out/test-plan/swarmui-ui}"
lock_digest=$(shasum -a 256 "${lockfile}" | awk '{print $1}')
browser_root="${PLAYWRIGHT_BROWSERS_PATH:-${repo_root}/out/toolchain/playwright-browsers/${lock_digest}}"
if [[ "${evidence_dir}" != /* ]]; then
  evidence_dir="${repo_root}/${evidence_dir}"
fi
if [[ "${browser_root}" != /* ]]; then
  browser_root="${repo_root}/${browser_root}"
fi
junit_path="${evidence_dir}/junit.xml"

usage() {
  cat <<'EOF'
Usage: scripts/ci/swarmui_ui_gate.sh [--run|--list|--verify-junit PATH]

--run is the default. It performs npm ci, installs the lockfile-selected
WebKit and Chromium builds, runs the deterministic Playwright matrix, and
requires a non-zero passing JUnit inventory.
EOF
}

playwright_version() {
  node -e \
    'const lock=require(process.argv[1]); const entry=lock.packages["node_modules/@playwright/test"]; if (!entry || !entry.version) process.exit(1); process.stdout.write(entry.version)' \
    "${lockfile}"
}

verify_junit() {
  local path="$1"
  python3 - "${path}" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET

path = pathlib.Path(sys.argv[1])
if not path.is_file() or path.stat().st_size == 0:
    print(f"missing Playwright JUnit evidence: {path}", file=sys.stderr)
    raise SystemExit(1)

try:
    root = ET.parse(path).getroot()
except ET.ParseError as error:
    print(f"invalid Playwright JUnit evidence: {error}", file=sys.stderr)
    raise SystemExit(1)

suites = [root] if root.tag == "testsuite" else list(root.iter("testsuite"))
if not suites:
    print("Playwright JUnit evidence contains no test suites", file=sys.stderr)
    raise SystemExit(1)

tests = sum(int(suite.attrib.get("tests", "0")) for suite in suites)
failures = sum(int(suite.attrib.get("failures", "0")) for suite in suites)
errors = sum(int(suite.attrib.get("errors", "0")) for suite in suites)
skipped = sum(int(suite.attrib.get("skipped", "0")) for suite in suites)
if tests <= 0:
    print("Playwright JUnit evidence contains zero tests", file=sys.stderr)
    raise SystemExit(1)
if failures or errors:
    print(
        f"Playwright JUnit evidence is not clean: tests={tests} "
        f"failures={failures} errors={errors}",
        file=sys.stderr,
    )
    raise SystemExit(1)
passed = tests - failures - errors - skipped
if passed <= 0:
    print(
        f"Playwright JUnit evidence contains zero passing tests: "
        f"tests={tests} skipped={skipped}",
        file=sys.stderr,
    )
    raise SystemExit(1)
print(f"SWARMUI_UI_TEST_COUNT={tests}")
print(f"SWARMUI_UI_PASS_COUNT={passed}")
PY
}

list_contract() {
  printf "playwright_version=%s\n" "$(playwright_version)"
  printf "lock_sha256=%s\n" "${lock_digest}"
  printf "browsers=webkit,chromium\n"
  printf "chromium_mode=new-headless-no-shell\n"
  printf "projects=webkit-desktop,webkit-narrow,chromium-tablet\n"
  printf "workers=%s\n" "${TP_UI_WORKERS}"
  printf "source_root=apps/swarmui/frontend\n"
}

run_gate() {
  mkdir -p "${evidence_dir}" "${browser_root}"
  rm -f "${junit_path}"
  {
    printf "PLAYWRIGHT_VERSION=%s\n" "$(playwright_version)"
    printf "PLAYWRIGHT_LOCK_SHA256=%s\n" "${lock_digest}"
    printf "PLAYWRIGHT_BROWSERS_PATH=%s\n" "${browser_root}"
    printf "SWARMUI_UI_SOURCE=apps/swarmui/frontend\n"
  } >"${evidence_dir}/gate.env"

  export PLAYWRIGHT_BROWSERS_PATH="${browser_root}"
  export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
  export PLAYWRIGHT_HTML_OUTPUT_DIR="${evidence_dir}/html-report"
  export PLAYWRIGHT_JUNIT_OUTPUT_NAME="${junit_path}"

  cd "${ui_root}"
  npm ci --ignore-scripts --no-audit --no-fund
  npx --no-install playwright install --no-shell webkit chromium
  npx --no-install playwright test \
    --workers="${TP_UI_WORKERS}" \
    --reporter=list,junit,html \
    --output="${evidence_dir}/test-results"
  verify_junit "${junit_path}"
}

if [[ $# -eq 0 || ( $# -eq 1 && "$1" == "--run" ) ]]; then
  run_gate
elif [[ $# -eq 1 && "$1" == "--list" ]]; then
  list_contract
elif [[ $# -eq 2 && "$1" == "--verify-junit" ]]; then
  verify_junit "$2"
elif [[ $# -eq 1 && ( "$1" == "-h" || "$1" == "--help" ) ]]; then
  usage
else
  usage >&2
  exit 2
fi
