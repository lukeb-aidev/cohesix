#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Guard against drift in coh-rtc generated artefacts.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
manifest_path="$repo_root/configs/root_task.toml"

if [[ ! -f "$manifest_path" ]]; then
  echo "configs/root_task.toml missing; run coh-rtc" >&2
  exit 1
fi

work_dir=$(mktemp -d)
trap 'rm -rf "${work_dir}"' EXIT

generated_dir="$work_dir/generated"
manifest_out="$work_dir/root_task_resolved.json"
cli_script="$work_dir/boot_v0.coh"
doc_snippet="$work_dir/root_task_manifest.md"

cargo run -p coh-rtc -- \
  "$manifest_path" \
  --out "$generated_dir" \
  --manifest "$manifest_out" \
  --cli-script "$cli_script" \
  --doc-snippet "$doc_snippet"

compare_file() {
  local expected="$1"
  local actual="$2"
  if ! diff -u "$expected" "$actual"; then
    echo "drift detected for $expected" >&2
    exit 1
  fi
}

compare_file "$repo_root/apps/root-task/src/generated/mod.rs" "$generated_dir/mod.rs"
compare_file "$repo_root/apps/root-task/src/generated/bootstrap.rs" "$generated_dir/bootstrap.rs"
compare_file "$repo_root/out/manifests/root_task_resolved.json" "$manifest_out"
compare_file "$repo_root/out/manifests/root_task_resolved.json.sha256" "${manifest_out}.sha256"
compare_file "$repo_root/scripts/cohsh/boot_v0.coh" "$cli_script"
compare_file "$repo_root/docs/snippets/root_task_manifest.md" "$doc_snippet"

printf "coh-rtc outputs match committed artefacts.\n"
