"""PEFT export/import/activate/rollback example."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
EXAMPLES_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT_DIR))
sys.path.insert(0, str(EXAMPLES_DIR))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.client import CohesixClient  # noqa: E402
from cohesix.defaults import DEFAULTS  # noqa: E402

from common import (  # noqa: E402
    add_backend_args,
    build_backend,
    resolve_output_root,
    write_audit,
)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    add_backend_args(parser)
    parser.add_argument("--job-id", default=None, help="export job id")
    parser.add_argument("--model-id", default=None, help="model id for import")
    parser.add_argument(
        "--previous-model-id",
        default=None,
        help="previous model id to seed rollback",
    )
    args = parser.parse_args()

    backend = build_backend(args)
    client = CohesixClient(backend)
    audit = CohesixAudit()

    defaults = DEFAULTS.get("examples", {})
    job_id = args.job_id or defaults.get("job_id", "job_8932")
    model_id = args.model_id or defaults.get("model_id", "llama3-edge-v7")
    previous_model_id = args.previous_model_id or "llama3-edge-v6"

    output_root = resolve_output_root(args.out)
    out_dir = output_root / "peft_roundtrip"
    export_root = out_dir / "export"
    registry_root = out_dir / "registry"
    adapter_dir = out_dir / "adapter"
    export_root.mkdir(parents=True, exist_ok=True)
    registry_root.mkdir(parents=True, exist_ok=True)
    adapter_dir.mkdir(parents=True, exist_ok=True)

    (adapter_dir / "adapter.safetensors").write_bytes(b"adapter-bytes")
    (adapter_dir / "lora.json").write_bytes(b"{\"rank\":8}")
    (adapter_dir / "metrics.json").write_bytes(b"{\"loss\":0.02}")

    client.peft_export(job_id, export_root, audit)
    client.peft_import(
        model_id=model_id,
        adapter_dir=adapter_dir,
        export_root=export_root,
        job_id=job_id,
        registry_root=registry_root,
        audit=audit,
    )
    client.peft_import(
        model_id=previous_model_id,
        adapter_dir=adapter_dir,
        export_root=export_root,
        job_id=job_id,
        registry_root=registry_root,
        audit=audit,
    )
    client.peft_activate(previous_model_id, registry_root, audit)
    client.peft_activate(model_id, registry_root, audit)
    client.peft_rollback(registry_root, audit)

    write_audit(out_dir, audit)


if __name__ == "__main__":
    main()
