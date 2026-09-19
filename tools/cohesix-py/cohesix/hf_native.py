# Author: Lukas Bower
# Purpose: Adapt pinned HF training, evaluation and native serving APIs to confined release phases; callbacks only record observations.
# Copyright 2026 Lukas Bower
"""Native CUDA provider helper. Admission and recipe position belong to the Rust executor.

The configured systemd user service runs the upstream Transformers server. This
module neither implements an inference API nor accepts arbitrary commands.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import re
import resource
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any
import urllib.request

MAX_JSON = 262144
MAX_ADAPTER = 33554432
REFERENCE_MODEL = "HuggingFaceTB/SmolLM2-135M"
REFERENCE_REVISION = "93efa2f097d58c2a74874c7e644dbc9b0cee75a2"
REFERENCE_WEIGHTS = "80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1"
VERSIONS = {
    "torch": "2.9.1+cu126", "transformers": "4.56.2", "peft": "0.17.1",
    "accelerate": "1.10.1", "safetensors": "0.6.2",
    "huggingface-hub": "0.34.4", "tokenizers": "0.22.0",
    "openai": "1.106.1", "fastapi": "0.116.1", "uvicorn": "0.35.0", "pydantic": "2.11.7",
    "cryptography": "45.0.7",
}


class Refused(ValueError):
    """A deterministic profile, artifact or native postcondition refusal."""


def require(condition: bool, reason: str) -> None:
    """Never turn absent native evidence into a default successful observation."""
    if not condition:
        raise Refused(reason)


def encode(value: Any) -> bytes:
    """Stable native artifact encoding; no non-finite metric values."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def regular(path: Path, maximum: int) -> bytes:
    """Confine reads to regular files, rejecting every symlink component."""
    require(path.is_absolute() and ".." not in path.parts, "invalid_path")
    for part in (path, *path.parents):
        require(not part.is_symlink(), "symlink_refused")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        meta = os.fstat(stream.fileno())
        require(stat.S_ISREG(meta.st_mode) and meta.st_size <=
                maximum, "artifact_size_or_kind")
        data = stream.read(maximum + 1)
    require(len(data) <= maximum, "artifact_size")
    return data


def read_json(path: Path) -> dict[str, Any]:
    value = json.loads(regular(path, MAX_JSON))
    require(isinstance(value, dict), "invalid_object")
    return value


def write(path: Path, data: bytes) -> None:
    """Commit on the same filesystem, syncing the file before rename and parent after."""
    require(path.parent.is_dir() and not path.is_symlink(), "invalid_atomic_target")
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".phase-", delete=False) as stream:
            temporary = stream.name
            os.chmod(temporary, 0o600)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        temporary = None
        fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary is not None:
            os.unlink(temporary)


def keys(value: dict[str, Any], required: set[str]) -> None:
    require(set(value) == required, "missing_or_unknown_fields")


@contextmanager
def phase_lock(operation: Path, rollback: bool):
    """Serialize native effects and reject delayed forward work after compensation."""
    descriptor = os.open(operation / "native.lock", os.O_RDWR |
                         os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        require(stat.S_ISREG(os.fstat(descriptor).st_mode), "invalid_native_lock")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        require(rollback or not (operation / "forward-cancelled.json").exists(),
                "forward_execution_cancelled")
        yield
    finally:
        os.close(descriptor)


class Provider:
    """One preconfigured native host profile, with no caller-selected executable or URL."""

    def __init__(self, config_path: Path, request_path: Path | None = None):
        self.config_path = config_path
        self.config = read_json(config_path)
        keys(self.config, {"schema", "root", "profile_sha256", "service", "port"})
        require(self.config["schema"] == "cohesix-hf-native/v1", "native_schema")
        self.root = Path(self.config["root"])
        require(self.root.is_absolute() and self.root.is_dir()
                and self.root.resolve() == self.root and self.root.stat().st_mode & 0o077 == 0,
                "private_root_required")
        require(re.fullmatch(r"cohesix-lora-[a-z0-9-]{1,48}\.service", self.config["service"]) is not None,
                "service_not_allowlisted")
        require(type(self.config["port"]) is int and 1024 <=
                self.config["port"] <= 65535, "port_bounds")
        self.profile = json.loads(self.blob(self.config["profile_sha256"], MAX_JSON))
        keys(self.profile, {"schema", "versions", "base", "tokenizer_sha256", "context",
                            "train_data", "eval_data", "settings", "canary", "license_refs",
                            "source_sha256", "attestations", "attestation_keys"})
        require(self.profile["schema"] == "cohesix-hf-profile/v1"
                and self.profile["versions"] == VERSIONS, "pinned_profile_required")
        require(self.profile["license_refs"]
                and self.profile["attestations"], "provenance_required")
        self.blob(self.profile["source_sha256"], MAX_JSON)
        self.attest(self.profile["attestations"], self.profile["source_sha256"], None)
        self.request: dict[str, Any] = {}
        self.request_sha = ""
        if request_path is not None:
            raw = regular(request_path, MAX_JSON)
            self.request = json.loads(raw)
            self.request_sha = sha(raw)
            require(self.request["profile_sha256"] ==
                    self.config["profile_sha256"], "profile_binding")
            require(re.fullmatch(r"[A-Za-z0-9_-]{1,64}", self.request["operation_id"]) is not None,
                    "operation_id")
            self.operation = self.root / "operations" / self.request["operation_id"]
            require(self.operation.is_dir()
                    and not self.operation.is_symlink(), "operation_missing")
            self.input = json.loads(self.blob(self.request["input_sha256"], MAX_JSON))

    def blob(self, reference: str, maximum: int) -> bytes:
        require(isinstance(reference, str) and re.fullmatch(r"[0-9a-f]{64}", reference) is not None,
                "invalid_cas_ref")
        payload = regular(self.root / "objects" / reference, maximum)
        require(sha(payload) == reference, "cas_digest_mismatch")
        return payload

    def current_authority(self) -> None:
        """The agent supplies the absolute admitted deadline; native commit cannot extend it."""
        require(type(self.authority_expiry) is int and int(time.time()*1000) < self.authority_expiry,
                "native_authority_expired")
        require(self.phase == "rollback" or not (self.operation / "forward-cancelled.json").exists(),
                "forward_execution_cancelled")

    def store(self, data: bytes) -> str:
        require(len(data) <= MAX_ADAPTER, "retained_artifact_bound")
        reference = sha(data)
        path = self.root / "objects" / reference
        if path.exists():
            require(self.blob(reference, MAX_ADAPTER) == data, "cas_collision")
        else:
            require(sum(1 for _ in path.parent.iterdir()) < 4096, "cas_capacity")
            require(sum(p.lstat().st_size for p in path.parent.iterdir()) + len(data) <= 2147483648,
                    "cas_byte_capacity")
            write(path, data)
        return reference

    def attest(self, references: list[str], source: str, adapter: str | None) -> None:
        """Require a source attestation signed by an independently configured profile key."""
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
        require(isinstance(references, list) and 1 <= len(references) <= 4
                and 1 <= len(self.profile["attestation_keys"]) <= 4, "source_attestation_required")
        expected = {"schema": "cohesix-peft-source-attestation/v1", "source_sha256": source,
                    "base_sha256": self.profile["context"]["base_sha256"],
                    "tokenizer_sha256": self.profile["tokenizer_sha256"],
                    "dataset_sha256": self.profile["train_data"], "adapter_bundle_sha256": adapter,
                    "permitted_use": "training-and-private-inference", "license_refs": self.profile["license_refs"]}
        for reference in references:
            record = json.loads(self.blob(reference, MAX_JSON))
            keys(record, {"payload", "key_id", "signature"})
            require(record["payload"] == expected and record["key_id"] in self.profile["attestation_keys"],
                    "source_attestation_binding")
            public_key = self.profile["attestation_keys"][record["key_id"]]
            Ed25519PublicKey.from_public_bytes(bytes.fromhex(public_key)).verify(
                bytes.fromhex(record["signature"]), encode(record["payload"]))

    def bundle(self, reference: str) -> tuple[Path, dict[str, Any]]:
        manifest = json.loads(self.blob(reference, MAX_JSON))
        keys(manifest, {"files", "source", "revision", "license_ref"})
        require(manifest["source"] and manifest["revision"]
                and manifest["license_ref"], "bundle_provenance")
        require(1 <= len(manifest["files"]) <= 16, "bundle_file_count")
        destination = self.root / "bundles" / reference
        destination.mkdir(mode=0o700, exist_ok=True)
        require(not destination.is_symlink(), "bundle_symlink")
        for name, object_ref in manifest["files"].items():
            require(re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", name) is not None
                    and name not in {".", ".."} and name.endswith((".json", ".safetensors", ".txt", ".md")),
                    "unsafe_bundle_file")
            limit = 1073741824 if name == "model.safetensors" else MAX_ADAPTER
            payload = self.blob(object_ref, limit)
            target = destination / name
            if target.exists():
                require(regular(target, limit) == payload,
                        "materialized_bundle_changed")
            else:
                write(target, payload)
        require(set(p.name for p in destination.iterdir()) ==
                set(manifest["files"]), "unexpected_bundle_file")
        return destination, manifest

    def stack(self) -> tuple[Any, Any]:
        for name, version in VERSIONS.items():
            require(importlib.metadata.version(name) ==
                    version, "runtime_version_mismatch")
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        os.environ["CUBLAS_WORKSPACE_CONFIG"] = ":4096:8"
        os.environ["TOKENIZERS_PARALLELISM"] = "false"
        import torch
        import transformers
        require(torch.cuda.is_available()
                and torch.version.cuda == "12.6", "cuda_required")
        torch.set_num_threads(2)
        torch.manual_seed(self.profile["settings"]["seed"])
        torch.use_deterministic_algorithms(True)
        torch.cuda.set_per_process_memory_fraction(0.40)
        return torch, transformers

    def candidate(self) -> Path:
        marker = read_json(self.operation / "candidate.json")
        path, _ = self.bundle(marker["bundle_sha256"])
        require(self.scan_adapter(path)["adapter_sha256"] == marker["adapter_sha256"],
                "candidate_artifact_binding")
        return path

    def validate(self) -> dict[str, Any]:
        self.stack()
        base, manifest = self.bundle(self.profile["base"])
        require(manifest["source"] == REFERENCE_MODEL and manifest["revision"] == REFERENCE_REVISION
                and manifest["files"].get("model.safetensors") == REFERENCE_WEIGHTS,
                "unqualified_base_model")
        require("model.safetensors" in manifest["files"]
                and "config.json" in manifest["files"], "safe_base_required")
        settings = self.profile["settings"]
        keys(settings, {"seed", "max_steps", "learning_rate",
             "rank", "max_length", "batch_size"})
        require(type(settings["seed"]) is int and 0 <= settings["seed"] < 2**32
                and type(settings["max_steps"]) is int and 1 <= settings["max_steps"] <= 64
                and math.isfinite(settings["learning_rate"]) and 0 < settings["learning_rate"] <= 0.001
                and settings["rank"] in {2, 4, 8} and settings["max_length"] in {64, 128}
                and settings["batch_size"] == 1, "training_bounds")
        canary = self.profile["canary"]
        keys(canary, {"prompts", "max_tokens", "maximum_latency_ms"})
        require(isinstance(canary["prompts"], list) and len(canary["prompts"]) == 4
                and all(isinstance(p, str) and 1 <= len(p.encode()) <= 256 for p in canary["prompts"])
                and type(canary["max_tokens"]) is int and 1 <= canary["max_tokens"] <= 8
                and type(canary["maximum_latency_ms"]) is int
                and 1 <= canary["maximum_latency_ms"] <= 30000, "canary_bounds")
        tokenizer = {name: value for name, value in manifest["files"].items()
                     if "token" in name or name in {"merges.txt", "vocab.json"}}
        context = self.profile["context"]
        expected = {"dataset_sha256": self.profile["eval_data"], "split": "heldout-16",
                    "preprocessing_sha256": sha(encode({"max_length": settings["max_length"],
                                                        "collator": "DataCollatorForLanguageModeling/mlm=false"})),
                    "base_sha256": manifest["files"]["model.safetensors"], "tokenizer_sha256": sha(encode(tokenizer)),
                    "evaluator": "transformers.Trainer.evaluate", "evaluator_version": VERSIONS["transformers"],
                    "parameters_sha256": sha(encode({"batch_size": 1, "max_length": settings["max_length"],
                                                     "seed": settings["seed"]})), "seed_policy": f'fixed:{settings["seed"]}',
                    "runtime_sha256": sha(encode({"versions": VERSIONS, "device": "cuda:0", "dtype": "float32", "attention": "eager"})),
                    "resource_sha256": sha(encode({"device": "cuda:0", "dtype": "float32", "threads": 2, "memory_fraction": 0.4}))}
        require(context == expected and self.profile["tokenizer_sha256"] == expected["tokenizer_sha256"],
                "evaluation_configuration_binding")
        train_rows = json.loads(self.blob(self.profile["train_data"], MAX_JSON))
        eval_rows = json.loads(self.blob(self.profile["eval_data"], MAX_JSON))
        require(len(eval_rows) == 16 and len(set(eval_rows)) == 16 and not set(train_rows).intersection(eval_rows),
                "independent_heldout_split_required")
        expected_input = {"profile_sha256", "source_sha256",
                          "attestations", "license_refs", "checkpoint"}
        if self.request["entry"] == "import":
            expected_input.update({"adapter_bundle_sha256", "base_sha256", "tokenizer_sha256", "versions",
                                   "dataset_sha256", "training_settings"})
        keys(self.input, expected_input)
        require(self.input["profile_sha256"] == self.config["profile_sha256"]
                and self.input["license_refs"] == self.profile["license_refs"] and self.input["source_sha256"]
                and self.input["attestations"], "input_provenance")
        self.blob(self.input["source_sha256"], MAX_JSON)
        self.attest(self.input["attestations"], self.input["source_sha256"],
                    self.input.get("adapter_bundle_sha256") if self.request["entry"] == "import" else None)
        require(self.input.get("checkpoint") is None,
                "native_resume_unqualified_use_new_authorized_attempt")
        if self.request["entry"] == "import":
            path, _ = self.bundle(self.input["adapter_bundle_sha256"])
            scanned = self.scan_adapter(path)
            require(self.input["base_sha256"] == self.profile["context"]["base_sha256"]
                    and self.input["tokenizer_sha256"] == self.profile["tokenizer_sha256"]
                    and self.input["versions"] == VERSIONS
                    and self.input["dataset_sha256"] == self.profile["train_data"]
                    and self.input["training_settings"] == settings, "import_compatibility")
            write(self.operation / "candidate.json", encode({"bundle_sha256": self.input["adapter_bundle_sha256"],
                  "adapter_sha256": scanned["adapter_sha256"], "entry": "import"}))
        else:
            require(self.request["entry"] == "train", "entry_not_supported")
        return {"base": str(base), "provenance": "verified", "native_checkpoint_resume": "unqualified"}

    def scan_adapter(self, path: Path) -> dict[str, Any]:
        from safetensors import safe_open
        payload = regular(path / "adapter_model.safetensors", MAX_ADAPTER)
        config_bytes = regular(path / "adapter_config.json", MAX_JSON)
        config = json.loads(config_bytes)
        base, _ = self.bundle(self.profile["base"])
        require(config.get("peft_type") == "LORA" and config.get("task_type") == "CAUSAL_LM"
                and config.get("base_model_name_or_path") == str(base)
                and config.get("r") == self.profile["settings"]["rank"]
                and set(config.get("target_modules", [])) == {"q_proj", "v_proj"}
                and config.get("lora_alpha") == 2*self.profile["settings"]["rank"]
                and config.get("lora_dropout") == 0 and config.get("bias") == "none"
                and config.get("modules_to_save") is None and config.get("auto_mapping") is None,
                "adapter_config_compatibility")
        import torch
        nonzero = False
        with safe_open(str(path / "adapter_model.safetensors"), framework="pt", device="cpu") as tensors:
            names = list(tensors.keys())
            require(0 < len(names) <= 512, "adapter_tensor_count")
            for name in names:
                require(".lora_A." in name or ".lora_B." in name,
                        "unexpected_adapter_tensor")
                value = tensors.get_tensor(name)
                require(value.ndim == 2 and value.dtype in {torch.float32, torch.float16, torch.bfloat16}
                        and bool(torch.isfinite(value).all()), "invalid_adapter_tensor")
                if ".lora_B." in name:
                    nonzero = nonzero or bool(torch.count_nonzero(value))
        require(nonzero, "untrained_or_synthetic_adapter")
        files = {"adapter_config.json": sha(
            config_bytes), "adapter_model.safetensors": sha(payload)}
        return {"adapter_sha256": sha(encode(files)), "files": files,
                "tensor_count": len(names), "safe_format": "safetensors"}

    def model(self, adapter: Path | None = None) -> tuple[Any, Any, Any]:
        torch, hf = self.stack()
        base, _ = self.bundle(self.profile["base"])
        tokenizer = hf.AutoTokenizer.from_pretrained(
            base, local_files_only=True, trust_remote_code=False)
        model = hf.AutoModelForCausalLM.from_pretrained(base, local_files_only=True, trust_remote_code=False,
                                                        use_safetensors=True, dtype=torch.float32, attn_implementation="eager").to("cuda")
        if adapter is not None:
            model.load_adapter(str(adapter), adapter_name="release", is_trainable=False)
            model.set_adapter("release")
        return model, tokenizer, hf

    def data(self, reference: str, tokenizer: Any) -> list[dict[str, Any]]:
        rows = json.loads(self.blob(reference, MAX_JSON))
        require(isinstance(rows, list) and 16 <= len(
            rows) <= 256, "dataset_sample_bounds")
        require(all(isinstance(row, str) and 16 <= len(row) <=
                2048 for row in rows), "dataset_text_bounds")
        result = []
        for row in rows:
            value = tokenizer(row, truncation=True,
                              max_length=self.profile["settings"]["max_length"])
            require(len(value["input_ids"]) >= 8, "insufficient_sample_tokens")
            value["labels"] = value["input_ids"].copy()
            result.append(value)
        return result

    def train(self) -> dict[str, Any]:
        import peft
        model, tokenizer, hf = self.model()
        settings = self.profile["settings"]
        model = peft.get_peft_model(model, peft.LoraConfig(r=settings["rank"], lora_alpha=2*settings["rank"],
                                                           target_modules=["q_proj", "v_proj"], lora_dropout=0.0, bias="none", task_type="CAUSAL_LM"))
        output = self.operation / "training"
        require(not output.exists(), "ambiguous_training_already_started")
        output.mkdir(mode=0o700)
        observations: list[dict[str, Any]] = []

        class Observer(hf.TrainerCallback):
            def on_log(self, args: Any, state: Any, control: Any, logs: Any = None, **kwargs: Any) -> None:
                observations.append({"step": state.global_step, "logs": logs})
                require(len(observations) <= 128, "callback_observation_bound")
                write(output / "observations.json", encode({"events": observations}))

            def on_train_end(self, args: Any, state: Any, control: Any, **kwargs: Any) -> None:
                observations.append({"event": "train_end", "step": state.global_step,
                                     "native_checkpoint": None, "resume_qualified": False})
                write(output / "observations.json", encode({"events": observations}))

        trainer = hf.Trainer(model=model, args=hf.TrainingArguments(
            output_dir=str(output), max_steps=settings["max_steps"], per_device_train_batch_size=1,
            learning_rate=settings["learning_rate"], seed=settings["seed"], data_seed=settings["seed"],
            save_strategy="no", logging_steps=1, report_to=[], optim="adamw_torch",
            dataloader_num_workers=0, dataloader_pin_memory=False, disable_tqdm=True),
            train_dataset=self.data(self.profile["train_data"], tokenizer),
            data_collator=hf.DataCollatorForLanguageModeling(tokenizer=tokenizer, mlm=False), callbacks=[Observer()])
        result = trainer.train()
        adapter = output / "adapter"
        model.save_pretrained(adapter, safe_serialization=True)
        scanned = self.scan_adapter(adapter)
        manifest = {"source": self.profile["source_sha256"], "revision": self.request["operation_id"],
                    "license_ref": self.profile["license_refs"][0], "files": {}}
        for name in ["adapter_model.safetensors", "adapter_config.json"]:
            manifest["files"][name] = self.store(regular(adapter / name, MAX_ADAPTER))
        bundle_sha = self.store(encode(manifest))
        write(self.operation / "candidate.json", encode({"bundle_sha256": bundle_sha,
              "adapter_sha256": scanned["adapter_sha256"], "entry": "train"}))
        # The CAS retains the deployable bytes and read-only observations. No
        # scratch weights or fabricated resumable checkpoint remain in training/.
        for name, reference in manifest["files"].items():
            require(self.blob(reference, MAX_ADAPTER) == regular(
                adapter/name, MAX_ADAPTER), "cleanup_cas_verification")
        shutil.rmtree(adapter)
        return {"native_job_id": "systemd:"+os.environ["INVOCATION_ID"], "training_metrics": result.metrics,
                "cleanup": {"scratch_adapter_removed": not adapter.exists(), "cas_artifacts_retained": True},
                "bundle_sha256": bundle_sha, "checkpoint": {"kind": "deployable_adapter",
                                                            "native_resume": False, "reason": "optimizer_scheduler_rng_checkpoint_not_qualified"}}

    def behavior(self, model: Any, tokenizer: Any, torch: Any) -> dict[str, Any]:
        """Read native greedy continuations for the exact configured model and prompts."""
        model.eval()
        behaviors = []
        for prompt in self.profile["canary"]["prompts"]:
            inputs = tokenizer.apply_chat_template([{"role": "user", "content": prompt}],
                                                   add_generation_prompt=True, return_tensors="pt").to("cuda")
            with torch.inference_mode():
                output = model.generate(inputs, attention_mask=torch.ones_like(inputs), do_sample=False,
                                        max_new_tokens=self.profile["canary"]["max_tokens"], use_cache=True)
            behaviors.append(tokenizer.decode(
                output[0, inputs.shape[1]:], skip_special_tokens=True))
        return {"texts": behaviors}

    def evaluate(self) -> dict[str, Any]:
        import gc
        torch, _ = self.stack()
        current = read_json(self.root / "accepted.json")
        require(current == self.request["baseline"], "baseline_generation_changed")
        result = {}
        baseline_bundle = read_json(self.root / "runtime.json")["bundle_sha256"]
        if current["adapter_sha256"] is None:
            require(baseline_bundle == self.profile["base"], "baseline_runtime_binding")
        else:
            require(self.scan_adapter(self.bundle(baseline_bundle)[0])["adapter_sha256"]
                    == current["served_artifact_sha256"], "baseline_runtime_binding")
        for label in ("baseline", "candidate"):
            adapter = self.candidate() if label == "candidate" else (
                self.bundle(baseline_bundle)[0] if current["adapter_sha256"] is not None else None)
            model, tokenizer, hf = self.model(adapter)
            rows = self.data(self.profile["eval_data"], tokenizer)
            trainer = hf.Trainer(model=model, args=hf.TrainingArguments(
                output_dir=str(self.operation / (label+"-evaluation")), per_device_eval_batch_size=1,
                report_to=[], seed=self.profile["settings"]["seed"], dataloader_num_workers=0,
                dataloader_pin_memory=False, disable_tqdm=True), eval_dataset=rows,
                data_collator=hf.DataCollatorForLanguageModeling(tokenizer=tokenizer, mlm=False))
            metrics = trainer.evaluate()
            native_report = self.store(encode(metrics))
            artifact = read_json(self.operation / "candidate.json")[
                "adapter_sha256"] if label == "candidate" else current["served_artifact_sha256"]
            result[label] = {"schema": "cohesix-native-evaluation/v1", "artifact_sha256": artifact,
                             "context": self.profile["context"], "samples": len(rows), "completed_unix_ms": int(time.time()*1000),
                             "metrics": {"eval_loss": metrics["eval_loss"]}, "native_report_sha256": native_report}
            write(self.operation / (label+"-behavior.json"),
                  encode(self.behavior(model, tokenizer, torch)))
            del trainer, model, tokenizer
            gc.collect()
            torch.cuda.empty_cache()
        self.canary(rollback=True)
        return result

    def scan(self) -> dict[str, Any]:
        self.stack()
        return self.scan_adapter(self.candidate())

    def stage(self) -> dict[str, Any]:
        candidate = read_json(self.operation / "candidate.json")
        adapter, manifest = self.bundle(candidate["bundle_sha256"])
        require(self.scan_adapter(adapter)["adapter_sha256"] == candidate["adapter_sha256"],
                "staged_artifact_binding")
        # The upstream server discovers architecture and tokenizer from this local model directory.
        # Base/config/tokenizer files remain exact profile bytes; no base weights are copied.
        _, base_manifest = self.bundle(self.profile["base"])
        serving = {**manifest, "files": dict(manifest["files"])}
        for name, reference in base_manifest["files"].items():
            if name.endswith(".json") or name == "chat_template.jinja":
                serving["files"][name] = reference
        serving_sha = self.store(encode(serving))
        path, _ = self.bundle(serving_sha)
        write(self.operation / "staged.json", encode({"bundle_sha256": serving_sha,
              "adapter_sha256": candidate["adapter_sha256"]}))
        return {"serving_bundle_sha256": serving_sha, "adapter_sha256": candidate["adapter_sha256"], "path": str(path)}

    def service(self, action: str) -> dict[str, str]:
        require(action in {"restart", "show"}, "service_action")
        command = ["systemctl", "--user", action, self.config["service"]]
        if action == "show":
            command += ["--property=ActiveState,SubState,MainPID,InvocationID,MemoryMax,MemoryCurrent,CPUUsageNSec,TasksMax"]
        value = subprocess.run(command, check=True,
                               capture_output=True, text=True, timeout=30)
        require(len(value.stdout) <= 8192 and len(value.stderr)
                <= 8192, "native_service_output_bound")
        return dict(line.split("=", 1) for line in value.stdout.splitlines() if "=" in line)

    def load(self) -> dict[str, Any]:
        old = read_json(self.root / "runtime.json")
        require(read_json(self.root / "accepted.json") ==
                self.request["baseline"], "baseline_generation_changed")
        write(self.operation / "rollback-runtime.json", encode(old))
        staged = read_json(self.operation / "staged.json")
        self.current_authority()
        write(self.root / "runtime.json", encode(staged))
        self.service("restart")
        self.ready()
        return {"runtime": staged, "service": self.service("show")}

    def ready(self) -> None:
        """Native startup has a finite 30 second readiness budget, separate from canary latency."""
        deadline = time.monotonic() + 30
        while True:
            require(self.service("show").get("ActiveState") in {
                    "active", "activating"}, "runtime_start_failed")
            try:
                with socket.create_connection(("127.0.0.1", self.config["port"]), timeout=1):
                    return
            except OSError:
                require(time.monotonic() < deadline, "runtime_start_deadline")
                time.sleep(0.1)

    def canary(self, rollback: bool = False) -> dict[str, Any]:
        runtime = read_json(self.root / "runtime.json")
        model, _ = self.bundle(runtime["bundle_sha256"])
        expected_artifact = self.request["baseline"]["served_artifact_sha256"] if rollback else read_json(
            self.operation / "candidate.json")["adapter_sha256"]
        if runtime["adapter_sha256"] is None:
            require(runtime["bundle_sha256"] == self.profile["base"]
                    and expected_artifact == self.profile["context"]["base_sha256"], "served_base_identity")
        else:
            require(self.scan_adapter(model)["adapter_sha256"] == expected_artifact
                    and runtime["adapter_sha256"] == expected_artifact, "served_adapter_identity")
        reference = read_json(
            self.operation / ("baseline-behavior.json" if rollback else "candidate-behavior.json"))
        before = self.service("show")
        require(before.get("ActiveState") == "active" and before.get("MainPID") not in {None, "0"}
                and before.get("InvocationID"), "runtime_not_active")
        texts = []
        latencies = []
        for prompt, expected in zip(self.profile["canary"]["prompts"], reference["texts"], strict=True):
            start = time.monotonic()
            request = urllib.request.Request(f'http://127.0.0.1:{self.config["port"]}/v1/chat/completions',
                                             data=encode({"model": str(model), "messages": [{"role": "user", "content": prompt}],
                                                          "stream": True, "temperature": 0, "max_tokens": self.profile["canary"]["max_tokens"]}),
                                             headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=30) as response:
                payload = response.read(65537)
            require(len(payload) <= 65536, "canary_output_bound")
            pieces = []
            terminal = False
            for line in payload.decode().splitlines():
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                record = json.loads(line[6:])
                require(record.get("model") == str(model) +
                        "@main", "served_model_identity")
                choice = record["choices"][0]
                pieces.append(choice["delta"].get("content") or "")
                terminal = terminal or choice.get("finish_reason") is not None
            actual = "".join(pieces)
            elapsed = int((time.monotonic()-start)*1000)
            require(terminal and actual == expected and elapsed <= self.profile["canary"]["maximum_latency_ms"],
                    "canary_behavior_or_latency")
            texts.append(sha(actual.encode()))
            latencies.append(elapsed)
        after = self.service("show")
        require(before.get("InvocationID") == after.get(
            "InvocationID"), "runtime_changed_during_canary")
        return {"runtime": runtime, "service": after, "behavior_sha256": texts,
                "latency_ms": latencies, "healthy": True}

    def promote(self) -> dict[str, Any]:
        current = read_json(self.root / "accepted.json")
        require(current == self.request["baseline"], "deployment_generation_conflict")
        staged = read_json(self.operation / "staged.json")
        require(read_json(self.root / "runtime.json") ==
                staged, "runtime_changed_before_promote")
        verified = self.canary()
        self.current_authority()
        require(read_json(self.root / "accepted.json") ==
                current, "deployment_generation_conflict")
        evaluations = read_json(self.operation / "evaluate.json")["detail"]
        now = int(time.time()*1000)
        maximum_age = self.request["evaluation_policy"]["maximum_age_ms"]
        require(all(0 <= now-evaluations[label]["completed_unix_ms"] < maximum_age
                    for label in ["candidate", "baseline"]), "evaluation_evidence_expired")
        accepted = {**current, "generation": current["generation"]+1,
                    "adapter_sha256": staged["adapter_sha256"],
                    "served_artifact_sha256": staged["adapter_sha256"], "healthy": True, "rollback_verified": True}
        write(self.root / "accepted.json", encode(accepted))
        return {"accepted": accepted, "canary": verified}

    def rollback(self) -> dict[str, Any]:
        target = self.operation / "rollback-runtime.json"
        # A durable Load intent may precede any native load call. In that case,
        # only the unchanged, frozen baseline may be verified and restored.
        previous = read_json(target if target.exists() else self.root / "runtime.json")
        if not target.exists():
            require(read_json(self.root / "accepted.json") == self.request["baseline"]
                    and previous["adapter_sha256"] == self.request["baseline"]["adapter_sha256"],
                    "unestablished_recovery_target")
        reference = self.operation / "baseline-behavior.json"
        if not reference.exists():
            # Cancellation before evaluation still verifies the frozen baseline
            # with the same native generation API; it emits no candidate score.
            expected = self.request["baseline"]["served_artifact_sha256"]
            base_adapter = None
            if previous["adapter_sha256"] is None:
                require(previous["bundle_sha256"] == self.profile["base"]
                        and expected == self.profile["context"]["base_sha256"], "recovery_base_identity")
            else:
                base_adapter = self.bundle(previous["bundle_sha256"])[0]
                require(self.scan_adapter(base_adapter)["adapter_sha256"] == expected,
                        "recovery_adapter_identity")
            model, tokenizer, _ = self.model(base_adapter)
            import torch
            write(reference, encode(self.behavior(model, tokenizer, torch)))
            del model, tokenizer
            import gc
            gc.collect()
            torch.cuda.empty_cache()
        self.current_authority()
        write(self.root / "runtime.json", encode(previous))
        self.service("restart")
        self.ready()
        verified = self.canary(rollback=True)
        self.current_authority()
        # Restore the frozen accepted generation only after observing native identity and behavior.
        write(self.root / "accepted.json", encode(self.request["baseline"]))
        return {"accepted": self.request["baseline"], "canary": verified, "candidate_release": "failed"}

    def serve(self) -> None:
        """Exec upstream serving with one immutable local model and a loopback endpoint."""
        self.stack()
        runtime = read_json(self.root / "runtime.json")
        model, _ = self.bundle(runtime["bundle_sha256"])
        executable = str(Path(sys.executable).parent / "transformers")
        os.execv(executable, [executable, "serve", "--host", "127.0.0.1", "--port", str(self.config["port"]),
                              "--device", "cuda:0", "--dtype", "float32", "--attn-implementation", "eager",
                              "--force-model", str(model), "--model-timeout", "3600"])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--request", type=Path)
    parser.add_argument("--recovery-id")
    parser.add_argument("--authority-expires-unix-ms", type=int)
    parser.add_argument("--phase", choices=["validate", "train", "evaluate", "scan",
                        "stage", "load", "canary", "promote", "rollback", "serve"], required=True)
    args = parser.parse_args()
    provider = Provider(args.config, args.request)
    if args.phase == "serve":
        provider.serve()
        return 0
    require(args.request is not None, "request_required")
    provider.authority_expiry = args.authority_expires_unix_ms
    provider.phase = args.phase
    target = provider.operation / (args.phase+".json")
    if args.recovery_id is not None:
        require(args.phase == "rollback" and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", args.recovery_id) is not None,
                "recovery_scope")
        target = provider.operation / ("recovery-"+args.recovery_id) / "rollback.json"
    with phase_lock(provider.operation, args.phase == "rollback"):
        require(not target.exists(), "duplicate_native_phase_refused")
        succeeded = True
        try:
            provider.current_authority()
            detail = getattr(provider, args.phase)()
            provider.current_authority()
        except Exception as error:
            succeeded = False
            detail = {"error": type(error).__name__, "reason": str(error)[:512]}
        usage = resource.getrusage(resource.RUSAGE_SELF)
        cpu_us = int((usage.ru_utime + usage.ru_stime) * 1000000)
        prior_cpu_us = 0
        observed_phases = 0
        prior_paths = [provider.operation / (phase+".json") for phase in
                       ["validate", "train", "evaluate", "scan", "stage", "load", "canary", "promote", "rollback"]]
        prior_paths.extend(sorted(provider.operation.glob("recovery-*/rollback.json")))
        require(len(prior_paths) <= 13, "recovery_accounting_bound")
        for previous in prior_paths:
            if previous.exists():
                prior = read_json(previous)["detail"].get("resources")
                if prior is not None:
                    prior_cpu_us += prior["process_cpu_us"]
                    observed_phases += 1
        detail["resources"] = {"process_cpu_us": cpu_us,
                               "process_peak_host_rss_bytes": usage.ru_maxrss * 1024,
                               "cumulative_observed_cpu_us": prior_cpu_us + cpu_us,
                               "completed_phase_observations": observed_phases + 1,
                               "cuda_allocation_is_hard_partition": False,
                               "serving_allocation_released": False}
        invocation = os.environ.get("INVOCATION_ID", "")
        require(re.fullmatch(r"[0-9a-f]{32}", invocation)
                is not None, "native_manager_identity_required")
        observation = {"operation_id": provider.request["operation_id"], "request_sha256": provider.request_sha,
                       "phase": args.phase, "native_identity": "systemd:"+invocation,
                       "completed_unix_ms": int(time.time()*1000), "succeeded": succeeded, "detail": detail}
        write(target, encode(observation))
        sys.stdout.buffer.write(encode(observation)+b"\n")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
