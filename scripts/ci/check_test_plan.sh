#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate docs/TEST_PLAN.md hashes against on-disk fixtures.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
doc_path="${repo_root}/docs/TEST_PLAN.md"

python3 - "$repo_root" "$doc_path" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
doc = pathlib.Path(sys.argv[2])
text = doc.read_text()
pattern = re.compile(r'^- `([^`]+)` — `sha256:([0-9a-f]{64})`$', re.M)
entries = pattern.findall(text)
if not entries:
    print("no hash entries found in docs/TEST_PLAN.md", file=sys.stderr)
    sys.exit(1)

errors = 0
for rel_path, expected in entries:
    path = root / rel_path
    if not path.is_file():
        print(f"missing file: {rel_path}", file=sys.stderr)
        errors += 1
        continue
    data = path.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        print(f"hash mismatch: {rel_path}", file=sys.stderr)
        print(f"  expected: {expected}", file=sys.stderr)
        print(f"  actual:   {actual}", file=sys.stderr)
        errors += 1

if errors:
    sys.exit(1)
print("test plan hashes ok")
PY
