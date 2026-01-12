#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Refresh docs/SECURITY.md latency metrics from test output JSON.
set -euo pipefail

input_path=${1:-out/metrics/telemetry_ring_latency.json}
output_snippet=${2:-docs/snippets/latency_metrics.md}
security_doc=${3:-docs/SECURITY.md}

python3 - "$input_path" "$output_snippet" "$security_doc" <<'PY'
import json
from pathlib import Path
import re
import sys

input_path = Path(sys.argv[1])
output_snippet = Path(sys.argv[2])
security_doc = Path(sys.argv[3])

payload = json.loads(input_path.read_text())

snippet = """### Telemetry Ring Latency (generated)
- Suite: `{suite}`
- Samples: `{samples}`
- P50: `{p50_ms:.3f} ms`
- P95: `{p95_ms:.3f} ms`
- Unit: `{unit}`
""".format(
    suite=payload.get("suite", "unknown"),
    samples=payload.get("samples", 0),
    p50_ms=float(payload.get("p50_ms", 0.0)),
    p95_ms=float(payload.get("p95_ms", 0.0)),
    unit=payload.get("unit", "ms"),
).strip()
snippet_with_source = "\n".join([snippet, f"_Generated from `{input_path}`._"])

output_snippet.parent.mkdir(parents=True, exist_ok=True)
output_snippet.write_text(
    "\n".join([
        "<!-- Author: Lukas Bower -->",
        "<!-- Purpose: Generated latency metrics for docs/SECURITY.md. -->",
        "",
        snippet_with_source,
    ])
)

contents = security_doc.read_text()
start = "<!-- metrics:latency:start -->"
end = "<!-- metrics:latency:end -->"
pattern = re.compile(re.escape(start) + r".*?" + re.escape(end), re.S)
replacement = "\n".join([start, snippet_with_source, end])
if not pattern.search(contents):
    raise SystemExit("metrics markers missing in docs/SECURITY.md")
security_doc.write_text(pattern.sub(replacement, contents))
PY
