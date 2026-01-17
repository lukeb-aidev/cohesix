#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Compare CohClient 9P replay transcripts against console semantics.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
transcript_root="${target_dir}/cohsh-client-transcripts"

cargo test -p cohsh --test client_lib

diff -u "${transcript_root}/console.txt" "${transcript_root}/client.txt"

printf "client vs console ok: zero-byte delta\n"
