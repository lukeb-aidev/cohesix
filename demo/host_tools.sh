#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run explicit 1.1.0-beta host workflows with retained logs and original exit status.
# Copyright 2026 Lukas Bower
set -euo pipefail
umask 077

demo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$demo_root"

usage() {
  cat <<'USAGE'
Usage:
  bash demo/host_tools.sh catalog OUT
  bash demo/host_tools.sh inspect|authority|telemetry|control|evidence OUT
  bash demo/host_tools.sh package PACKAGE TRUST OUT
  bash demo/host_tools.sh cuda|lora plan|apply|watch|verify|recover DEPLOYMENT OUT
  bash demo/host_tools.sh journey validate|doctor|run|submit CONFIG OUT

COH_BIN is the matching host-tool directory (default: target/debug).
COH_POLICY and COHSH_POLICY select exact generated policies when needed.
Live .coh scripts require DEMO_TRANSPORT=rest|tcp and the documented connection
variables. CUDA/LoRA apply and CUDA recover use the existing gateway plus
COH_TICKET_REF; secrets are resolved by coh, never expanded into command logs.
OUT must be a new private directory under an existing parent.
Plan may create a durable journal. Apply/submit executes approved work.
Recovery never changes operation identity or blindly retries submission.
USAGE
}

fail() { printf '%s\n' "$*" >&2; exit 2; }
[[ $# -gt 0 ]] || { usage; exit 2; }
case "$1" in --help|-h) usage; exit 0 ;; esac
kind=$1
shift
case "$kind" in
  catalog|inspect|authority|telemetry|control|evidence)
    [[ $# -eq 1 ]] || { usage; exit 2; }
    output=$1 ;;
  package)
    [[ $# -eq 3 ]] || { usage; exit 2; }
    package=$1; trust=$2; output=$3
    [[ -d "$package" && -f "$trust" ]] || fail "package directory and trust file required" ;;
  cuda|lora)
    [[ $# -eq 3 ]] || { usage; exit 2; }
    phase=$1; deployment=$2; output=$3
    case "$phase" in plan|apply|watch|verify|recover) ;; *) fail "unknown workflow phase" ;; esac
    [[ "$deployment" = /* && -f "$deployment" ]] || fail "deployment must be an existing absolute file" ;;
  journey)
    [[ $# -eq 3 ]] || { usage; exit 2; }
    phase=$1; config=$2; output=$3
    case "$phase" in validate|doctor|run|submit) ;; *) fail "unknown journey phase" ;; esac
    [[ "$config" = /* && -f "$config" ]] || fail "config must be an existing absolute file" ;;
  *) usage; exit 2 ;;
esac

coh_bin=${COH_BIN:-"$demo_root/target/debug"}
coh=("$coh_bin/coh")
cohsh=("$coh_bin/cohsh")
[[ -z ${COH_POLICY:-} ]] || coh+=(--policy "$COH_POLICY")
[[ -z ${COHSH_POLICY:-} ]] || cohsh+=(--policy "$COHSH_POLICY")
mode=local
connection=()
case "$kind" in
  inspect|authority|telemetry|control|evidence)
    mode=live
    case "${DEMO_TRANSPORT:-}" in
      rest)
        : "${COH_REST_URL:?select the existing gateway}"
        connection=(--transport rest --rest-url "$COH_REST_URL") ;;
      tcp)
        : "${COH_TARGET_HOST:?select the verified target host}"
        : "${COH_TARGET_PORT:?select the verified target port}"
        # cohsh otherwise gives these inherited variables priority over CLI flags.
        unset COHSH_TCP_HOST COHSH_TCP_PORT
        connection=(--transport tcp --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT") ;;
      *) fail "set DEMO_TRANSPORT=rest or tcp explicitly" ;;
    esac ;;
  cuda|lora)
    if [[ "$phase" == apply || ( "$kind" == cuda && "$phase" == recover ) ]]; then
      mode=live
      : "${COH_REST_URL:?select the existing gateway}"
      : "${COH_REST_AUTH_TOKEN:?configure the gateway authentication reference}"
      : "${COH_TICKET_REF:?select the scoped operator ticket reference}"
      case "$COH_TICKET_REF" in file:?*|env:?*) ;; *) fail "COH_TICKET_REF must be file: or env:" ;; esac
      coh+=(--ticket-ref "$COH_TICKET_REF")
      connection=(--rest-url "$COH_REST_URL")
    fi ;;
  journey) mode=deployment ;;
esac

# Refuse existing logs, including symlinks; retain partial output on command failure.
mkdir -m 700 -- "$output"
output=$(cd "$output" && pwd)
{
  printf 'release=1.1.0-beta\nmode=%s\nproof=none\nworkflow=%s\n' "$mode" "$kind"
  printf 'source_commit=%s\n' "$(git rev-parse HEAD)"
  if [[ -z "$(git status --porcelain --untracked-files=all)" ]]; then
    printf 'source_tree=clean\n'
  else
    printf 'source_tree=modified\n'
  fi
  printf 'coh_bin=%s\n' "$coh_bin"
  printf 'provider_graph_sha256='
  shasum -a 256 configs/generated/provider_registry.json | cut -d ' ' -f 1
  printf 'graph_source=checkout (compare with installed catalog)\n'
} > "$output/run.txt"

run() {
  local label=$1
  shift
  # Do not echo argv: secret references and deployment paths may be private.
  printf 'Running %s; logs: %s\n' "$label" "$output"
  "$@" 2>&1 | tee "$output/$label.log"
}

case "$kind" in
  catalog)
    run coh-providers "${coh[@]}" providers
    run cohsh-providers "${cohsh[@]}" --provider-registry
    run cuda-contract "${coh[@]}" plan cuda-reference --recipe
    for tool in cohsh coh-status hive-gateway gpu-bridge-host host-ticket-agent host-sidecar-bridge sidecar-bus cas-tool; do
      if [[ -x "$coh_bin/$tool" ]]; then
        run "$tool-help" "$coh_bin/$tool" --help
      else
        printf 'tool=%s availability=not-installed\n' "$tool" >> "$output/run.txt"
      fi
    done ;;
  inspect|authority|telemetry|control|evidence)
    script="demo/$kind.coh"
    [[ "$kind" != inspect ]] || script=demo/demo_runbook.coh
    [[ "$kind" != control ]] || script=demo/control_plane.coh
    run check "${cohsh[@]}" --check "$script"
    run "$kind" "${cohsh[@]}" "${connection[@]}" --script "$script" ;;
  package)
    run package-verify "${coh[@]}" package verify --input "$package" --trust "$trust" ;;
  cuda)
    # Bash 3.2 treats an empty array as unset under nounset.
    run "cuda-$phase" "${coh[@]}" "$phase" cuda-reference --recipe --deployment "$deployment" ${connection[@]+"${connection[@]}"} ;;
  lora)
    run "lora-$phase" "${coh[@]}" peft release "$phase" --deployment "$deployment" ${connection[@]+"${connection[@]}"} ;;
  journey)
    journey=${COH_JOURNEY_BIN:-cohesix-journey}
    if [[ "$phase" == submit ]]; then
      run journey-submit "$journey" run --config "$config" --submit
    else
      run "journey-$phase" "$journey" "$phase" --config "$config"
    fi ;;
esac
printf 'commands_completed=yes\nproof=none (interpret the retained verifier result)\n' >> "$output/run.txt"
