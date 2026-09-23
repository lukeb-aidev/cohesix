#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Prepare a small licensed text-completion reference and exact HF release inputs without submitting tickets or manufacturing execution evidence.
# Copyright 2026 Lukas Bower
"""Prepare a private native HF profile from a pinned local base and repository docs.

Run with the qualified Python environment on the CUDA host. Review the emitted
profile, source attestation, native configuration and systemd unit before admitting
the emitted release request through `coh peft release`. Source enrollment here is
an explicit local reference enrollment, not model-vendor or device attestation.
"""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import re
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.hf_native import VERSIONS, encode, regular, sha, write


def prepare(root: Path, base: Path, base_manifest: Path, documents: list[Path], port: int) -> None:
    """Materialize bounded reference inputs with content hashes and explicit local source custody."""
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    for name in ["objects", "bundles", "operations"]:
        (root/name).mkdir(mode=0o700)

    def put(payload: bytes) -> str:
        digest = sha(payload)
        destination = root/"objects"/digest
        if not destination.exists():
            write(destination, payload)
        return digest

    original = json.loads(regular(base_manifest, 262144))
    files = {}
    for name, digest in original["files"].items():
        if not re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", name) or name in {".", ".."}:
            raise ValueError("invalid base manifest file name")
        path = base/name
        payload = regular(path, 1073741824)
        if sha(payload) != digest:
            raise ValueError("base provenance or regular-file verification failed")
        files[name] = put(payload)
    # The declared reference is plain text continuation. Pin this preprocessing
    # instead of pretending a base model shipped a vendor chat template.
    config = json.loads(regular(base/"tokenizer_config.json", 262144))
    config["chat_template"] = "{% for message in messages %}{{ message['content'] }}{% endfor %}"
    config["pad_token"] = config["eos_token"]
    files["tokenizer_config.json"] = put(encode(config))
    bundle = {"files": files, "source": original["model"],
              "revision": original["revision"], "license_ref": original["license_ref"]}
    base_ref = put(encode(bundle))
    tokenizer_sha = sha(encode({name: value for name, value in files.items(
    ) if "token" in name or name in {"merges.txt", "vocab.json"}}))
    paragraphs = []
    document_refs = []
    for document in documents:
        payload = regular(document.absolute(), 262144)
        document_refs.append(
            {"name": document.name, "sha256": put(payload), "license": "Apache-2.0"})
        text = re.sub(r"```.*?```", "", payload.decode(), flags=re.S)
        for paragraph in text.split("\n\n"):
            paragraph = " ".join(paragraph.split())
            if 120 <= len(paragraph) <= 700 and not any(character in paragraph for character in "|<>#"):
                if paragraph not in paragraphs:
                    paragraphs.append(paragraph)
    if len(paragraphs) < 80:
        raise ValueError("provide at least 80 distinct licensed prose paragraphs")
    train = put(encode(paragraphs[:64]))
    evaluate = put(encode(paragraphs[64:80]))
    settings = {"seed": 41, "max_steps": 32, "learning_rate": 0.0005,
                "rank": 4, "max_length": 128, "batch_size": 1}
    runtime_sha = sha(encode(
        {"versions": VERSIONS, "device": "cuda:0", "dtype": "float32", "attention": "eager"}))
    context = {"dataset_sha256": evaluate, "split": "heldout-16", "preprocessing_sha256": sha(encode({"max_length": 128, "collator": "DataCollatorForLanguageModeling/mlm=false"})),
               "base_sha256": files["model.safetensors"], "tokenizer_sha256": tokenizer_sha,
               "evaluator": "transformers.Trainer.evaluate", "evaluator_version": VERSIONS["transformers"],
               "parameters_sha256": sha(encode({"batch_size": 1, "max_length": 128, "seed": 41})), "seed_policy": "fixed:41",
               "runtime_sha256": runtime_sha, "resource_sha256": sha(encode({"device": "cuda:0", "dtype": "float32", "threads": 2, "memory_fraction": 0.4}))}
    source = put(encode({"base": original, "documents": document_refs,
                        "permitted_use": "training-and-private-inference", "tokenizer_transform": config["chat_template"]}))
    key = Ed25519PrivateKey.generate()
    write(root/"source.private.key",
          key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()
    licenses = [original["license_ref"],
                "https://github.com/lukasbower/cohesix/blob/main/LICENSE.txt"]
    attested = {"schema": "cohesix-peft-source-attestation/v1", "source_sha256": source,
                "base_sha256": context["base_sha256"], "tokenizer_sha256": tokenizer_sha, "dataset_sha256": train,
                "adapter_bundle_sha256": None, "permitted_use": "training-and-private-inference", "license_refs": licenses}
    attestation = put(encode({"payload": attested, "key_id": "local-reference-source",
                             "signature": key.sign(encode(attested)).hex()}))
    profile = {"schema": "cohesix-hf-profile/v1", "versions": VERSIONS, "base": base_ref,
               "tokenizer_sha256": tokenizer_sha, "context": context, "train_data": train, "eval_data": evaluate,
               "settings": settings, "canary": {"prompts": ["Cohesix keeps model weights", "A host ticket", "The accepted deployment", "A failed adapter"],
                                                "max_tokens": 8, "maximum_latency_ms": 30000}, "license_refs": licenses, "source_sha256": source,
               "attestations": [attestation], "attestation_keys": {"local-reference-source": public}}
    profile_sha = put(encode(profile))
    input_ref = put(encode({"profile_sha256": profile_sha, "source_sha256": source, "attestations": [attestation],
                           "license_refs": licenses, "checkpoint": None}))
    baseline = {"generation": 0, "adapter_sha256": None, "served_artifact_sha256": context["base_sha256"],
                "runtime_sha256": runtime_sha, "healthy": True, "rollback_verified": True}
    # This is a requested baseline. Native evaluation must verify its serving
    # identity and behavior before it becomes admissible comparison evidence.
    write(root/"accepted.json", encode(baseline))
    write(root/"runtime.json",
          encode({"bundle_sha256": base_ref, "adapter_sha256": None}))
    service = "cohesix-lora-reference.service"
    native = {"schema": "cohesix-hf-native/v1",
              "root": str(root), "profile_sha256": profile_sha, "service": service, "port": port}
    write(root/"native.json", encode(native))
    helper = Path(__file__).resolve().parents[1]/"cohesix/hf_native.py"
    agent = {"python": sys.executable, "helper": str(helper), "helper_sha256": sha(
        helper.read_bytes()), "native_config": str(root/"native.json")}
    write(root/"agent.json", encode(agent))
    # Rust's request serialization order is intentional; the host refuses other encodings.
    request = {"schema": "cohesix-peft-release/v1", "operation_id": "m27d-train-01", "model_id": "private-lora-reference",
               "entry": "train", "profile_sha256": profile_sha, "input_sha256": input_ref,
               "evaluation_policy": {"minimum_samples": 16, "maximum_age_ms": 3600000,
                                     "metrics": {"eval_loss": {"direction": "lower", "absolute_bound": 8.0, "maximum_regression": 0.0}}},
               "baseline": baseline}
    raw = json.dumps(request, separators=(",", ":"), allow_nan=False).encode()
    request_ref = put(raw)
    write(root/"request.json", raw)
    for value in [sys.executable, str(helper), str(root)]:
        if re.fullmatch(r"/[A-Za-z0-9_./-]+", value) is None:
            raise ValueError(
                "reference service paths must contain simple absolute path characters")
    unit = f'''# Author: Lukas Bower
# Purpose: Run the pinned upstream Transformers server with one private immutable model.
# Copyright 2026 Lukas Bower
[Unit]
Description=Cohesix private LoRA reference serving
[Service]
Type=exec
ExecStart={sys.executable} -I {helper} --config {root}/native.json --phase serve
MemoryMax=2147483648
TasksMax=128
CPUQuota=200%
NoNewPrivileges=yes
Restart=no
'''
    write(root/service, unit.encode())
    print(json.dumps({"mode": "prepared-only", "request_sha256": request_ref, "profile_sha256": profile_sha,
                      "source_public_key": public, "serving_unit": str(root/service), "native_config": str(root/"native.json")}, indent=2))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--base-manifest", type=Path, required=True)
    parser.add_argument("--document", type=Path, action="append", required=True)
    parser.add_argument("--port", type=int, default=38527)
    args = parser.parse_args()
    os.umask(0o077)
    prepare(args.root.resolve(), args.base.resolve(),
            args.base_manifest.resolve(), args.document, args.port)


if __name__ == "__main__":
    main()
