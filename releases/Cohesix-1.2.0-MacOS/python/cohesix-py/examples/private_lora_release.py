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
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.hf_native import VERSIONS, encode, regular, sha, write


def selection_data(path: Path) -> tuple[dict, list[str], list[str]]:
    """Freeze reviewed train/evaluation inputs before creating any native state."""
    selected = tomllib.loads(regular(path, 262144).decode("utf-8"))
    expected = {"schema", "train_data", "eval_data", "settings", "canary", "evaluation_policy",
                "license_refs", "credential_refs", "gated_download", "upload", "resource_caps"}
    if not isinstance(selected, dict) or set(selected) != expected or selected["schema"] != "cohesix-peft-selection/v1":
        raise ValueError("invalid PEFT selection")
    if selected["gated_download"] is not False or selected["upload"] is not False:
        raise ValueError("offline reference cannot download gated models or upload private data")
    refs = selected["credential_refs"]
    if not isinstance(refs, list) or len(refs) > 4 or any(not isinstance(ref, str) or
            re.fullmatch(r"(?:env:[A-Z][A-Z0-9_]{0,63}|file:/[A-Za-z0-9_./-]{1,255})", ref) is None
            for ref in refs):
        raise ValueError("invalid credential reference")
    if selected["resource_caps"] != {"phase_memory_bytes": 3221225472,
                                     "serving_memory_bytes": 2147483648,
                                     "retained_cas_bytes": 2147483648}:
        raise ValueError("resource caps must match the enforced native owner")
    settings = selected["settings"]
    required_settings = {"seed", "max_steps", "learning_rate", "rank", "max_length",
                         "batch_size", "checkpoint_interval"}
    if (not isinstance(settings, dict) or set(settings) != required_settings
            or type(settings["seed"]) is not int or not 0 <= settings["seed"] < 2**32
            or type(settings["max_steps"]) is not int or not 2 <= settings["max_steps"] <= 64
            or type(settings["learning_rate"]) not in {int, float}
            or not 0 < settings["learning_rate"] <= 0.001
            or type(settings["rank"]) is not int or settings["rank"] not in {2, 4, 8}
            or type(settings["max_length"]) is not int or settings["max_length"] not in {64, 128}
            or settings["batch_size"] != 1
            or type(settings["checkpoint_interval"]) is not int
            or not 1 <= settings["checkpoint_interval"] <= 16
            or settings["checkpoint_interval"] >= settings["max_steps"]):
        raise ValueError("invalid selected training and checkpoint settings")
    canary = selected["canary"]
    if (not isinstance(canary, dict) or set(canary) != {"prompts", "max_tokens", "maximum_latency_ms"}
            or not isinstance(canary["prompts"], list) or len(canary["prompts"]) != 4
            or any(not isinstance(prompt, str) or not 1 <= len(prompt.encode()) <= 256
                   for prompt in canary["prompts"])
            or type(canary["max_tokens"]) is not int or not 1 <= canary["max_tokens"] <= 8
            or type(canary["maximum_latency_ms"]) is not int
            or not 1 <= canary["maximum_latency_ms"] <= 30000):
        raise ValueError("invalid fixed application canary")
    policy = selected["evaluation_policy"]
    if (not isinstance(policy, dict) or set(policy) != {"minimum_samples", "maximum_age_ms", "metrics"}
            or policy["minimum_samples"] != 16 or type(policy["maximum_age_ms"]) is not int
            or not 1 <= policy["maximum_age_ms"] <= 3600000
            or not isinstance(policy["metrics"], dict) or set(policy["metrics"]) != {"eval_loss"}):
        raise ValueError("invalid predeclared evaluation policy")
    bound = policy["metrics"]["eval_loss"]
    if (not isinstance(bound, dict) or set(bound) != {"direction", "absolute_bound", "maximum_regression"}
            or bound["direction"] != "lower" or type(bound["absolute_bound"]) not in {int, float}
            or not 0 < bound["absolute_bound"] <= 8.0 or bound["maximum_regression"] != 0):
        raise ValueError("invalid held-out loss bound")
    if (not isinstance(selected["license_refs"], list) or not selected["license_refs"] or
            len(selected["license_refs"]) > 8 or any(not isinstance(ref, str) or len(ref) > 256
            for ref in selected["license_refs"])):
        raise ValueError("explicit licence references required")
    rows = []
    for label in ("train_data", "eval_data"):
        data_path = Path(selected[label])
        value = json.loads(regular(data_path, 262144))
        if not isinstance(value, list) or not all(isinstance(row, str) and 16 <= len(row) <= 2048
                                                   for row in value):
            raise ValueError("invalid licensed text rows")
        rows.append(value)
    train_rows, eval_rows = rows
    if not (16 <= len(train_rows) <= 256 and len(eval_rows) == 16
            and len(set(train_rows)) == len(train_rows)
            and len(set(eval_rows)) == 16 and set(train_rows).isdisjoint(eval_rows)):
        raise ValueError("training and held-out rows must be distinct")
    return selected, train_rows, eval_rows


def prepare(root: Path, base: Path, base_manifest: Path, documents: list[Path], port: int,
            selection: Path | None = None, capabilities: Path | None = None,
            service: str = "cohesix-lora-reference.service") -> None:
    """Materialize bounded reference inputs with content hashes and explicit local source custody."""
    if re.fullmatch(r"cohesix-lora-[a-z0-9-]{1,48}\.service", service) is None:
        raise ValueError("invalid isolated serving unit name")
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
    selected = None
    if selection is not None:
        if capabilities is None:
            raise ValueError("selected PEFT profile requires compiled capabilities")
        selected, train_rows, eval_rows = selection_data(selection)
        generated = json.loads(regular(capabilities, 262144))
        if generated.get("schema") != "cohesix-cuda-recipe-contract/v2":
            raise ValueError("compiled CUDA recipe contract required")
        compiled = generated.get("peft")
        capability_fields = {"schema", "base_model", "base_revision", "training", "import_formats",
                             "checkpoint", "checkpoint_files", "checkpoint_interval_max", "serving", "qlora"}
        if (not isinstance(compiled, dict) or set(compiled) != capability_fields
                or compiled["base_model"] != original["model"] or
                compiled["base_revision"] != original["revision"] or compiled["qlora"] is not False):
            raise ValueError("selected base is not in compiled PEFT capabilities")
        capabilities_ref = put(encode(compiled))
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
    if selected is None:
        if len(paragraphs) < 80:
            raise ValueError("provide at least 80 distinct licensed prose paragraphs")
        train_rows, eval_rows = paragraphs[:64], paragraphs[64:80]
    train = put(encode(train_rows))
    evaluate = put(encode(eval_rows))
    settings = selected["settings"] if selected is not None else {
        "seed": 41, "max_steps": 32, "learning_rate": 0.0005,
        "rank": 4, "max_length": 128, "batch_size": 1}
    runtime_sha = sha(encode(
        {"versions": VERSIONS, "device": "cuda:0", "dtype": "float32", "attention": "eager"}))
    context = {"dataset_sha256": evaluate, "split": "heldout-16", "preprocessing_sha256": sha(encode({"max_length": settings["max_length"], "collator": "DataCollatorForLanguageModeling/mlm=false"})),
               "base_sha256": files["model.safetensors"], "tokenizer_sha256": tokenizer_sha,
               "evaluator": "transformers.Trainer.evaluate", "evaluator_version": VERSIONS["transformers"],
               "parameters_sha256": sha(encode({"batch_size": 1, "max_length": settings["max_length"], "seed": settings["seed"]})), "seed_policy": f'fixed:{settings["seed"]}',
               "runtime_sha256": runtime_sha, "resource_sha256": sha(encode({"device": "cuda:0", "dtype": "float32", "threads": 2, "memory_fraction": 0.4}))}
    source = put(encode({"base": original, "documents": document_refs,
                        "train_data_sha256": train, "eval_data_sha256": evaluate,
                        "selection_sha256": sha(encode(selected)) if selected is not None else None,
                        "credential_refs": selected["credential_refs"] if selected is not None else [],
                        "permitted_use": "training-and-private-inference", "tokenizer_transform": config["chat_template"]}))
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption
    key = Ed25519PrivateKey.generate()
    write(root/"source.private.key",
          key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()
    licenses = selected["license_refs"] if selected is not None else [original["license_ref"],
                "https://github.com/lukasbower/cohesix/blob/main/LICENSE.txt"]
    if original["license_ref"] not in licenses:
        raise ValueError("selected licences omit the base model licence")
    attested = {"schema": "cohesix-peft-source-attestation/v1", "source_sha256": source,
                "base_sha256": context["base_sha256"], "tokenizer_sha256": tokenizer_sha, "dataset_sha256": train,
                "adapter_bundle_sha256": None, "permitted_use": "training-and-private-inference", "license_refs": licenses}
    attestation = put(encode({"payload": attested, "key_id": "local-reference-source",
                             "signature": key.sign(encode(attested)).hex()}))
    profile = {"schema": "cohesix-hf-profile/v2" if selected is not None else "cohesix-hf-profile/v1",
               "versions": VERSIONS, "base": base_ref,
               "tokenizer_sha256": tokenizer_sha, "context": context, "train_data": train, "eval_data": evaluate,
               "settings": settings, "canary": selected["canary"] if selected is not None else
               {"prompts": ["Cohesix keeps model weights", "A host ticket", "The accepted deployment", "A failed adapter"],
                "max_tokens": 8, "maximum_latency_ms": 30000}, "license_refs": licenses, "source_sha256": source,
               "attestations": [attestation], "attestation_keys": {"local-reference-source": public}}
    if selected is not None:
        profile["capabilities_sha256"] = capabilities_ref
        profile["evaluation_policy"] = selected["evaluation_policy"]
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
               "evaluation_policy": selected["evaluation_policy"] if selected is not None else {"minimum_samples": 16, "maximum_age_ms": 3600000,
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
    parser.add_argument("--document", type=Path, action="append", default=[])
    parser.add_argument("--selection", type=Path)
    parser.add_argument("--capabilities", type=Path)
    parser.add_argument("--port", type=int, default=38527)
    parser.add_argument("--service", default="cohesix-lora-reference.service")
    args = parser.parse_args()
    if args.selection is None and not args.document:
        parser.error("provide --selection or at least one --document")
    os.umask(0o077)
    prepare(args.root.resolve(), args.base.resolve(),
            args.base_manifest.resolve(), args.document, args.port,
            args.selection.resolve() if args.selection else None,
            args.capabilities.resolve() if args.capabilities else None, args.service)


if __name__ == "__main__":
    main()
