#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and bind fresh target components for provisioned root-task checks.
# Copyright 2026 Lukas Bower

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/ci/test_plan_target_root_check.sh \
  --target qemu|pi4 --sel4-build <dir> --profile <name> \
  --features release-qemu|release-pi4 --timer-clock-hz <hz>

Builds target components in the current Stage 02 attempt, packages exact
Worker and driver-runtime identities, and checks root-task with those bindings.
EOF
}

fail() {
  printf 'target-root-check: %s\n' "$*" >&2
  exit 1
}

target=""
sel4_build=""
profile=""
features=""
timer_clock_hz=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="${2:-}"; shift 2 ;;
    --sel4-build) sel4_build="${2:-}"; shift 2 ;;
    --profile) profile="${2:-}"; shift 2 ;;
    --features) features="${2:-}"; shift 2 ;;
    --timer-clock-hz) timer_clock_hz="${2:-}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) fail "unknown argument: $1" ;;
  esac
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
case "${target}:${profile}:${features}:${timer_clock_hz}" in
  qemu:qemu_smp_production:release-qemu:24000000)
    expected_sel4="${repo_root}/out/sel4/profile-v2/qemu-smp-production"
    selected_manifest="${repo_root}/configs/root_task.toml"
    projection_profile="qemu_smp_production"
    ;;
  pi4:pi4_diagnostic:release-pi4:54000000)
    expected_sel4="${repo_root}/seL4/build_UBOOT"
    selected_manifest="${repo_root}/configs/root_task_pi4_uboot_aarch64.toml"
    projection_profile="pi4_production"
    ;;
  *)
    fail "target/profile/features/timer tuple is not canonical"
    ;;
esac

[[ -n "${sel4_build}" && -d "${sel4_build}" ]] || fail "selected seL4 build directory is missing"
sel4_build=$(cd "${sel4_build}" && pwd)
[[ "${sel4_build}" == "${expected_sel4}" ]] || fail "selected seL4 build directory does not match the canonical ${target} profile"
platform_header="${sel4_build}/kernel/gen_headers/plat/platform_gen.h"
[[ -f "${platform_header}" ]] || fail "selected seL4 timer header is missing"
grep -Eq "^#define TIMER_CLOCK_HZ ULL_CONST\\(${timer_clock_hz}\\)$" "${platform_header}" ||
  fail "selected seL4 timer frequency does not match ${timer_clock_hz} Hz"

[[ -n "${TEST_PLAN_STATE_DIR:-}" ]] || fail "TEST_PLAN_STATE_DIR is required"
[[ -n "${TEST_PLAN_ATTEMPT_ID:-}" ]] || fail "TEST_PLAN_ATTEMPT_ID is required"
evidence_kind="attempts"
[[ "${TEST_PLAN_ITERATION:-0}" == "1" ]] && evidence_kind="iterations"
attempt_dir="${TEST_PLAN_STATE_DIR}/evidence/${evidence_kind}/stage-02/${TEST_PLAN_ATTEMPT_ID}"
[[ -d "${attempt_dir}" ]] || fail "current Stage 02 attempt directory is missing"
output_dir="${attempt_dir}/target-root-binding/${target}"
[[ ! -e "${output_dir}" ]] || fail "target binding output already exists: ${output_dir}"
mkdir -p "${output_dir}"

cd "${repo_root}"
export CARGO_TARGET_DIR="${output_dir}/cargo-target"
export SEL4_BUILD_DIR="${sel4_build}"

selected_projection="${output_dir}/selected-python-profile.json"
cargo run --locked -p coh-rtc --bin coh-rtc-python-profile -- \
  "${selected_manifest}" \
  --sel4-profiles "${repo_root}/configs/sel4/profiles.toml" \
  --profile "${projection_profile}" \
  --out "${selected_projection}"

projection_identity=$(
  python3 - \
    "${selected_projection}" \
    "${repo_root}/apps/root-task/src/generated/mod.rs" \
    "${target}" \
    "${projection_profile}" <<'PY'
import json
from pathlib import Path
import re
import sys

projection_path = Path(sys.argv[1])
generated_path = Path(sys.argv[2])
expected_target = sys.argv[3]
expected_profile = sys.argv[4]

projection = json.loads(projection_path.read_text(encoding="utf-8"))
if projection.get("schema") != "cohesix-python-profile/v1":
    raise SystemExit("selected target projection has an unexpected schema")
if projection.get("target") != expected_target:
    raise SystemExit("selected target projection has an unexpected target")
if projection.get("target_profile") != expected_profile:
    raise SystemExit("selected target projection has an unexpected profile")

selected_sha = projection.get("manifest_sha256")
if not isinstance(selected_sha, str) or re.fullmatch(r"[0-9a-f]{64}", selected_sha) is None:
    raise SystemExit("selected target projection has an invalid manifest identity")

generated = generated_path.read_text(encoding="utf-8")
match = re.search(
    r'pub const MANIFEST_SHA256:\s*&str\s*=\s*"([0-9a-f]{64})";',
    generated,
)
if match is None:
    raise SystemExit("compiled root-task generated manifest identity is missing")

print(f"{selected_sha} {match.group(1)}")
PY
) || fail "selected target manifest identity could not be verified"
read -r selected_manifest_sha compiled_manifest_sha <<<"${projection_identity}"
[[ "${selected_manifest_sha}" == "${compiled_manifest_sha}" ]] ||
  fail "generated root-task projection does not match selected ${target} manifest (selected=${selected_manifest_sha} compiled=${compiled_manifest_sha})"

target_triple="aarch64-unknown-none"
cargo build --locked --release --target "${target_triple}" \
  -p nine-door-runtime \
  -p console-network-runtime \
  -p worker-heart \
  -p worker-gpu \
  -p worker-lora \
  -p pi4-driver-runtime

artifact_dir="${CARGO_TARGET_DIR}/${target_triple}/release"
worker_dir="${output_dir}/worker-images"
worker_archive="${worker_dir}/cohesix-worker-images.cpio"
worker_manifest="${worker_dir}/cohesix-worker-image-manifest.json"
python3 "${repo_root}/scripts/worker_image_manifest.py" build \
  --image-dir "${artifact_dir}" \
  --output-dir "${worker_dir}/canonical" \
  --archive "${worker_archive}" \
  --manifest "${worker_manifest}" \
  --target "${target_triple}" \
  --profile release

driver_dir="${output_dir}/driver-runtimes"
driver_archive="${driver_dir}/cohesix-driver-runtimes.cpio"
driver_manifest="${driver_dir}/cohesix-driver-runtime-manifest.json"
python3 "${repo_root}/scripts/driver_runtime_manifest.py" build \
  --image-dir "${artifact_dir}" \
  --archive "${driver_archive}" \
  --manifest "${driver_manifest}" \
  --target "${target_triple}" \
  --profile release \
  --classic-comparator-record "${repo_root}/configs/driver_runtime_classic_comparator.toml"

root_env=(
  "SEL4_BUILD_DIR=${sel4_build}"
  "SEL4_LD=${repo_root}/apps/root-task/sel4.ld"
  "COHESIX_WORKER_IMAGE_ARCHIVE=${worker_archive}"
  "COHESIX_WORKER_IMAGE_MANIFEST=${worker_manifest}"
  "COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE=${artifact_dir}/console-network-runtime"
  "COHESIX_NINEDOOR_RUNTIME_IMAGE=${artifact_dir}/nine-door-runtime"
  "COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD=${driver_archive}"
)
if [[ "${target}" == "pi4" ]]; then
  root_env+=("COHESIX_PI4_WIFI_FIRMWARE_DIR=${repo_root}/third_party/raspberry-pi-firmware/v1.50/firmware/cyw43455-linux-capture")
fi
env "${root_env[@]}" cargo check --locked -p root-task \
  --target "${target_triple}" \
  --no-default-features \
  --features "${features}"

printf 'target-root-check: PASS target=%s profile=%s timer_clock_hz=%s manifest_sha256=%s output=%s\n' \
  "${target}" "${profile}" "${timer_clock_hz}" "${selected_manifest_sha}" "${output_dir}"
