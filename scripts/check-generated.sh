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
cas_template="$work_dir/cas_manifest_template.json"
cli_script="$work_dir/boot_v0.coh"
doc_snippet="$work_dir/root_task_manifest.md"
gpu_breadcrumbs="$work_dir/gpu_breadcrumbs.md"
observability_interfaces="$work_dir/observability_interfaces.md"
observability_security="$work_dir/observability_security.md"
ticket_quotas="$work_dir/ticket_quotas.md"
trace_policy="$work_dir/trace_policy.md"
cas_interfaces="$work_dir/cas_interfaces.md"
cas_security="$work_dir/cas_security.md"
cohesix_py_defaults="$work_dir/cohesix_py_defaults.py"
cohesix_py_doc="$work_dir/cohesix_py_defaults.md"
coh_doctor_doc="$work_dir/coh_doctor_checks.md"
cohsh_policy="$work_dir/cohsh_policy.toml"
cohsh_policy_rust="$work_dir/cohsh_policy.rs"
cohsh_policy_doc="$work_dir/cohsh_policy.md"
cohsh_client_rust="$work_dir/cohsh_client.rs"
cohsh_client_doc="$work_dir/cohsh_client.md"
cohsh_grammar_doc="$work_dir/cohsh_grammar.md"
cohsh_ticket_policy_doc="$work_dir/cohsh_ticket_policy.md"
coh_policy="$work_dir/coh_policy.toml"
coh_policy_rust="$work_dir/coh_policy.rs"
coh_policy_doc="$work_dir/coh_policy.md"

cargo run -p coh-rtc -- \
  "$manifest_path" \
  --out "$generated_dir" \
  --manifest "$manifest_out" \
  --cas-manifest-template "$cas_template" \
  --cli-script "$cli_script" \
  --doc-snippet "$doc_snippet" \
  --gpu-breadcrumbs-snippet "$gpu_breadcrumbs" \
  --observability-interfaces-snippet "$observability_interfaces" \
  --observability-security-snippet "$observability_security" \
  --ticket-quotas-snippet "$ticket_quotas" \
  --trace-policy-snippet "$trace_policy" \
  --cas-interfaces-snippet "$cas_interfaces" \
  --cas-security-snippet "$cas_security" \
  --cohsh-policy "$cohsh_policy" \
  --cohsh-policy-rust "$cohsh_policy_rust" \
  --cohsh-policy-doc "$cohsh_policy_doc" \
  --cohsh-client-rust "$cohsh_client_rust" \
  --cohsh-client-doc "$cohsh_client_doc" \
  --cohsh-grammar-doc "$cohsh_grammar_doc" \
  --cohsh-ticket-policy-doc "$cohsh_ticket_policy_doc" \
  --coh-policy "$coh_policy" \
  --coh-policy-rust "$coh_policy_rust" \
  --coh-policy-doc "$coh_policy_doc" \
  --cohesix-py-defaults "$cohesix_py_defaults" \
  --cohesix-py-doc "$cohesix_py_doc" \
  --coh-doctor-doc "$coh_doctor_doc"

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
compare_file "$repo_root/out/cas_manifest_template.json" "$cas_template"
compare_file "$repo_root/out/cas_manifest_template.json.sha256" "${cas_template}.sha256"
compare_file "$repo_root/scripts/cohsh/boot_v0.coh" "$cli_script"
compare_file "$repo_root/docs/snippets/root_task_manifest.md" "$doc_snippet"
compare_file "$repo_root/docs/snippets/gpu_breadcrumbs.md" "$gpu_breadcrumbs"
compare_file "$repo_root/docs/snippets/observability_interfaces.md" "$observability_interfaces"
compare_file "$repo_root/docs/snippets/observability_security.md" "$observability_security"
compare_file "$repo_root/docs/snippets/ticket_quotas.md" "$ticket_quotas"
compare_file "$repo_root/docs/snippets/trace_policy.md" "$trace_policy"
compare_file "$repo_root/docs/snippets/cas_interfaces.md" "$cas_interfaces"
compare_file "$repo_root/docs/snippets/cas_security.md" "$cas_security"
compare_file "$repo_root/out/cohsh_policy.toml" "$cohsh_policy"
compare_file "$repo_root/out/cohsh_policy.toml.sha256" "${cohsh_policy}.sha256"
compare_file "$repo_root/apps/cohsh/src/generated/policy.rs" "$cohsh_policy_rust"
compare_file "$repo_root/docs/snippets/cohsh_policy.md" "$cohsh_policy_doc"
compare_file "$repo_root/apps/cohsh/src/generated/client.rs" "$cohsh_client_rust"
compare_file "$repo_root/docs/snippets/cohsh_client.md" "$cohsh_client_doc"
compare_file "$repo_root/docs/snippets/cohsh_grammar.md" "$cohsh_grammar_doc"
compare_file "$repo_root/docs/snippets/cohsh_ticket_policy.md" "$cohsh_ticket_policy_doc"
compare_file "$repo_root/out/coh_policy.toml" "$coh_policy"
compare_file "$repo_root/out/coh_policy.toml.sha256" "${coh_policy}.sha256"
compare_file "$repo_root/apps/coh/src/generated/policy.rs" "$coh_policy_rust"
compare_file "$repo_root/docs/snippets/coh_policy.md" "$coh_policy_doc"
compare_file "$repo_root/tools/cohesix-py/cohesix/generated.py" "$cohesix_py_defaults"
compare_file "$repo_root/docs/snippets/cohesix_py_defaults.md" "$cohesix_py_doc"
compare_file "$repo_root/docs/snippets/coh_doctor_checks.md" "$coh_doctor_doc"

"$repo_root/scripts/ci/check_test_plan.sh"
if [[ -f "$repo_root/scripts/ci/security_nist.sh" ]]; then
  "$repo_root/scripts/ci/security_nist.sh"
fi

printf "coh-rtc outputs match committed artefacts.\n"
