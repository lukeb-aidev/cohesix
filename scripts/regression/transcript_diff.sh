#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Compare cohsh transcripts across serial/TCP/core transports.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
transcript_root="${target_dir}/cohsh-transcripts"

cargo test -p cohsh-core --test transcripts

diff -u "${transcript_root}/boot_v0/serial.txt" "${transcript_root}/boot_v0/core.txt"
diff -u "${transcript_root}/boot_v0/serial.txt" "${transcript_root}/boot_v0/tcp.txt"
diff -u "${transcript_root}/abuse/serial.txt" "${transcript_root}/abuse/core.txt"
diff -u "${transcript_root}/abuse/serial.txt" "${transcript_root}/abuse/tcp.txt"

printf "transcript diff ok: zero-byte delta\n"
