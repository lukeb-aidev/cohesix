#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Enroll an independently supplied safe PEFT adapter with explicit unknown training provenance.
# Copyright 2026 Lukas Bower
"""Prepare a locally signed import input; this does not train, admit or deploy it."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.hf_native import MAX_ADAPTER, MAX_JSON, Provider, VERSIONS, encode, regular, sha, write


def prepare(config: Path, adapter: Path, origin: str, license_ref: str, output: Path) -> dict:
    """Record only observed bytes and source custody, never an inferred training job."""
    if not isinstance(origin, str) or re.fullmatch(r"[A-Za-z0-9_./:@-]{1,256}", origin) is None:
        raise ValueError("invalid non-secret source reference")
    provider = Provider(config)
    if provider.profile_schema != "cohesix-hf-profile/v2":
        raise ValueError("independent import requires the selected M28b profile")
    if license_ref not in provider.profile["license_refs"]:
        raise ValueError("adapter licence must be explicitly accepted by the profile")
    if not adapter.is_dir() or adapter.is_symlink() or output.exists():
        raise ValueError("adapter directory or output path is unsafe")
    if set(child.name for child in adapter.iterdir()) != {"adapter_config.json", "adapter_model.safetensors"}:
        raise ValueError("independent adapter must contain only native PEFT safe files")
    scanned = provider.scan_adapter(adapter)
    files = {name: provider.store(regular(adapter / name, MAX_JSON if name.endswith(".json") else MAX_ADAPTER))
             for name in ("adapter_config.json", "adapter_model.safetensors")}
    source = provider.store(encode({"schema": "cohesix-peft-import-source/v1", "origin": origin,
                                    "files": files, "training_provenance": "unknown"}))
    bundle = provider.store(encode({"source": source, "revision": sha(encode(files)),
                                    "license_ref": license_ref, "files": files}))
    private = regular(provider.root / "source.private.key", 32)
    if len(private) != 32:
        raise ValueError("invalid local source-custodian key")
    key = Ed25519PrivateKey.from_private_bytes(private)
    payload = {"schema": "cohesix-peft-source-attestation/v2", "source_sha256": source,
               "base_sha256": provider.profile["context"]["base_sha256"],
               "tokenizer_sha256": provider.profile["tokenizer_sha256"],
               "dataset_sha256": None, "adapter_bundle_sha256": bundle,
               "permitted_use": "training-and-private-inference",
               "license_refs": provider.profile["license_refs"], "training_provenance": "unknown"}
    attestation = provider.store(encode({"payload": payload, "key_id": "local-reference-source",
                                         "signature": key.sign(encode(payload)).hex()}))
    input_record = {"profile_sha256": provider.config["profile_sha256"], "source_sha256": source,
                    "attestations": [attestation], "license_refs": provider.profile["license_refs"],
                    "checkpoint": None, "adapter_bundle_sha256": bundle,
                    "base_sha256": provider.profile["context"]["base_sha256"],
                    "tokenizer_sha256": provider.profile["tokenizer_sha256"], "versions": VERSIONS,
                    "training_provenance": "unknown"}
    provider.store(encode(input_record))
    write(output, encode(input_record))
    return {"mode": "prepared-only", "adapter_sha256": scanned["adapter_sha256"],
            "bundle_sha256": bundle, "source_sha256": source,
            "training_provenance": "unknown", "input": str(output)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-config", type=Path, required=True)
    parser.add_argument("--adapter", type=Path, required=True)
    parser.add_argument("--origin", required=True)
    parser.add_argument("--license-ref", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(args.native_config, args.adapter, args.origin,
                             args.license_ref, args.out), sort_keys=True))


if __name__ == "__main__":
    main()
