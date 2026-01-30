#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Assert documentation invariants for NIST evidence mapping.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

if ! command -v rg >/dev/null 2>&1; then
  echo "rg is required for this check" >&2
  exit 1
fi

fail=0

check() {
  local label="$1"
  local pattern="$2"
  local file="$3"
  if ! rg -q "$pattern" "$file"; then
    echo "missing ${label} in ${file} (pattern: ${pattern})" >&2
    fail=1
  fi
}

check_ci() {
  local label="$1"
  local pattern="$2"
  local file="$3"
  if ! rg -qi "$pattern" "$file"; then
    echo "missing ${label} in ${file} (pattern: ${pattern})" >&2
    fail=1
  fi
}

secure9p_doc="$repo_root/docs/SECURE9P.md"
roles_doc="$repo_root/docs/ROLES_AND_SCHEDULING.md"
userland_doc="$repo_root/docs/USERLAND_AND_CLI.md"
security_doc="$repo_root/docs/SECURITY.md"
interfaces_doc="$repo_root/docs/INTERFACES.md"
agents_doc="$repo_root/AGENTS.md"

# Secure9P bounds and invariants
check "Secure9P msize bound" "msize.*8192|8192.*msize" "$secure9p_doc"
check "Secure9P walk depth bound" "walk depth.*8|depth.*8" "$secure9p_doc"
check "Secure9P no .. traversal" "Disallow .*\\.\\.|no .*\\.\\." "$secure9p_doc"
check "Secure9P fid reuse after clunk" "fid reuse.*clunk|clunk.*fid reuse" "$secure9p_doc"

# ACK/ERR before side effects (console response line precedes payload)
check "ACK/ERR before payload" "before any payload" "$userland_doc"

# Role isolation and mount scoping
check "Role namespaces documented" "/shard/<label>/worker/<id>/telemetry" "$roles_doc"
check "Role-to-namespace rules" "Role-to-namespace" "$secure9p_doc"

# Rate limiting on auth failures
check "Auth rate limiting" "leaky-bucket rate limiter" "$security_doc"

# Audit line expectations
check_ci "Audit line guidance" "audit lines" "$security_doc"
check "Reason tagging" "reason=<busy\\|quota\\|cut\\|policy>" "$userland_doc"

# Attach grammar and ticket requirement
check "Attach grammar" "attach <role> \\[ticket\\]" "$userland_doc"

# Console-only TCP listener guidance
check "Console-only TCP listener" "only permitted in-VM TCP listener" "$agents_doc"

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "nist evidence smoke checks ok"
