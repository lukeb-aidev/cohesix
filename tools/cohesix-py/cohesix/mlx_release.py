# Author: Lukas Bower
# Purpose: Record admitted Mac MLX release phases under launchd custody with content, generation, Metal, and rollback checks.
# Copyright 2026 Lukas Bower
"""Native MLX phase adapter for the durable Cohesix PEFT release journal.

Only the host-ticket agent may admit a request. This module reads that frozen
request, performs one phase, and writes an observation under its original ID.
It has no ticket creation or evidence-signing authority.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import resource
import subprocess
import sys
import time
from typing import Any
import urllib.request

from .hf_native import encode, keys, phase_lock, read_json, regular, require, sha, write
from .mlx_native import (
    MAX_ADAPTER_BYTES, MAX_DATA_FILE_BYTES, MAX_MODEL_FILE_BYTES,
    MlxSelection, PINNED_VERSIONS, _adapter, evaluate_heldout, infer,
    observed_metal, train_lora, tree_digest,
)


MAX_JSON = 262_144
PHASES = ("validate", "train", "evaluate", "scan", "stage", "load",
          "canary", "promote", "rollback")
IDENTIFIER = re.compile(r"[A-Za-z0-9_-]{1,64}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
LABEL = re.compile(r"cohesix-mlx-serve-[a-z0-9-]{1,48}\Z")


def _now() -> int:
    return int(time.time() * 1000)


def _module_hash(module: Any) -> str:
    return sha(regular(Path(module.__file__).resolve(), MAX_JSON))


def _service_state(label: str) -> dict[str, str] | None:
    result = subprocess.run(["/bin/launchctl", "list", label],
                            capture_output=True, check=False, timeout=10)
    require(len(result.stdout) <= 8192 and len(result.stderr) <= 8192,
            "launchd_output_bound")
    if result.returncode != 0:
        return None
    return {key.strip().strip('"'): value.strip().rstrip(";").strip('"')
            for line in result.stdout.decode().splitlines() if " = " in line
            for key, value in [line.split(" = ", 1)]}


def _stop_service(label: str, root: Path) -> None:
    state = _service_state(label)
    if state is None:
        return
    logs = root / "service-logs"
    require(state.get("Label") == label
            and state.get("Program") == "/usr/bin/env"
            and state.get("StandardOutPath") == str(logs / "stdout")
            and state.get("StandardErrorPath") == str(logs / "stderr"),
            "foreign_launchd_service_label")
    old_pid = state.get("PID")
    require(old_pid is None or (old_pid.isascii() and old_pid.isdigit()
                              and int(old_pid) > 0),
            "ambiguous_launchd_service_pid")
    result = subprocess.run(["/bin/launchctl", "remove", label],
                            capture_output=True, check=False, timeout=15)
    require(result.returncode == 0, "ambiguous_launchd_stop")
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if _service_state(label) is None:
            if old_pid is None:
                return
            try:
                os.kill(int(old_pid), 0)
            except ProcessLookupError:
                return
        time.sleep(0.1)
    raise ValueError("ambiguous_launchd_stop_or_process")


class Provider:
    """One pinned local profile and one immutable admitted release request."""

    def __init__(self, config_path: Path, request_path: Path, helper: Path):
        require(helper.resolve() == Path(__file__).resolve(),
                "loaded_helper_differs_from_agent_selection")
        self.config = read_json(config_path)
        keys(self.config, {"schema", "root", "profile_sha256", "service_label",
                           "port", "python"})
        require(self.config["schema"] == "cohesix-mlx-native/v1"
                and isinstance(self.config["root"], str)
                and isinstance(self.config["python"], str)
                and DIGEST.fullmatch(self.config["profile_sha256"]) is not None
                and LABEL.fullmatch(self.config["service_label"]) is not None
                and Path(self.config["python"]).is_absolute()
                and Path(self.config["python"]).resolve() ==
                Path(sys.executable).resolve()
                and type(self.config["port"]) is int
                and 1024 <= self.config["port"] <= 65535,
                "native_profile_selection")
        self.root = Path(self.config["root"])
        require(self.root.is_absolute() and self.root.is_dir()
                and self.root.resolve() == self.root
                and self.root.stat().st_mode & 0o077 == 0,
                "private_root_required")
        self.profile = json.loads(self.blob(self.config["profile_sha256"], MAX_JSON))
        keys(self.profile, {"schema", "model_directory", "model_sha256",
                            "data_directory", "data_sha256", "memory_limit_bytes",
                            "settings", "canary", "quality", "evaluation_policy", "context",
                            "source_sha256", "license_refs"})
        require(self.profile["schema"] == "cohesix-mlx-profile/v1"
                and DIGEST.fullmatch(self.profile["source_sha256"]) is not None
                and isinstance(self.profile["license_refs"], list)
                and 1 <= len(self.profile["license_refs"]) <= 4
                and all(isinstance(value, str) and 1 <= len(value) <= 256
                        for value in self.profile["license_refs"]),
                "mlx_provenance")
        self.selection = MlxSelection(
            Path(self.profile["model_directory"]), self.profile["model_sha256"],
            Path(self.profile["data_directory"]), self.profile["data_sha256"],
            self.profile["memory_limit_bytes"])
        self.selection.validate()
        self.request_bytes = regular(request_path, MAX_JSON)
        self.request = json.loads(self.request_bytes)
        self.request_sha = sha(self.request_bytes)
        require(self.request["schema"] == "cohesix-peft-release/v1"
                and self.request["profile_sha256"] == self.config["profile_sha256"]
                and IDENTIFIER.fullmatch(self.request["operation_id"]) is not None,
                "release_request_binding")
        self.operation = self.root / "operations" / self.request["operation_id"]
        require(self.operation.is_dir() and not self.operation.is_symlink(),
                "release_operation_missing")
        self.input = json.loads(self.blob(self.request["input_sha256"], MAX_JSON))
        keys(self.input, {"profile_sha256", "source_sha256", "adapter_directory",
                          "adapter_sha256"})
        require(self.input["profile_sha256"] == self.config["profile_sha256"]
                and self.input["source_sha256"] == self.profile["source_sha256"],
                "release_input_provenance")
        self.authority_expiry = 0
        self.phase = ""

    def blob(self, reference: str, maximum: int) -> bytes:
        require(isinstance(reference, str) and DIGEST.fullmatch(reference) is not None,
                "invalid_cas_reference")
        value = regular(self.root / "objects" / reference, maximum)
        require(sha(value) == reference, "cas_digest_mismatch")
        return value

    def store(self, value: bytes) -> str:
        require(len(value) <= MAX_JSON, "native_report_bound")
        reference = sha(value)
        destination = self.root / "objects" / reference
        if destination.exists():
            require(self.blob(reference, MAX_JSON) == value, "cas_collision")
        else:
            require(sum(1 for _ in destination.parent.iterdir()) < 4096,
                    "cas_capacity")
            write(destination, value)
        return reference

    def authority(self) -> None:
        require(type(self.authority_expiry) is int and _now() < self.authority_expiry,
                "native_authority_expired")
        require(self.phase == "rollback" or not
                (self.operation / "forward-cancelled.json").exists(),
                "forward_execution_cancelled")

    def context(self) -> dict[str, Any]:
        """Derive comparable context from actual selected source and data bytes."""
        from . import mlx_native, mlx_service
        files = {}
        for name in ("tokenizer.json", "tokenizer_config.json", "special_tokens_map.json"):
            path = self.selection.model_directory / name
            if path.exists():
                files[name] = sha(regular(path, MAX_MODEL_FILE_BYTES))
        require(bool(files), "tokenizer_identity_missing")
        device = observed_metal(self.selection)
        settings = self.profile["settings"]
        keys(settings, {"steps", "rank", "seed"})
        require(type(settings["steps"]) is int and 2 <= settings["steps"] <= 64
                and settings["rank"] in (2, 4, 8)
                and type(settings["seed"]) is int
                and 0 <= settings["seed"] < 2**32, "mlx_training_settings")
        return {
            "dataset_sha256": sha(regular(self.selection.data_directory / "test.jsonl",
                                          MAX_DATA_FILE_BYTES)),
            "split": "heldout-test",
            "preprocessing_sha256": sha(encode({"loader": "mlx_lm.lora.load_dataset",
                                                  "max_seq_length": 128})),
            "base_sha256": self.selection.model_sha256,
            "tokenizer_sha256": sha(encode(files)),
            "evaluator": "mlx_lm.lora.evaluate",
            "evaluator_version": PINNED_VERSIONS["mlx-lm"],
            "parameters_sha256": sha(encode({"batch_size": 1, "split": "test",
                                               "seed": settings["seed"]})),
            "seed_policy": f'fixed:{settings["seed"]}',
            "runtime_sha256": sha(encode({"versions": PINNED_VERSIONS,
                                            "native": _module_hash(mlx_native),
                                            "service": _module_hash(mlx_service),
                                            "helper": _module_hash(sys.modules[__name__])})),
            "resource_sha256": sha(encode({"device": device["device_name"],
                                             "memory_limit_bytes":
                                             self.selection.memory_limit_bytes})),
        }

    def runtime(self) -> dict[str, Any]:
        value = read_json(self.root / "runtime.json")
        keys(value, {"generation", "adapter_directory", "adapter_sha256"})
        require(type(value["generation"]) is int and 0 <= value["generation"] < 2**32,
                "runtime_generation")
        self.adapter(value)
        return value

    def adapter(self, runtime: dict[str, Any]) -> Path | None:
        path = runtime["adapter_directory"]
        digest = runtime["adapter_sha256"]
        require((path is None) == (digest is None), "runtime_adapter_pair")
        if path is None:
            return None
        require(isinstance(path, str), "runtime_adapter_path")
        selected = Path(path)
        _adapter(self.selection, selected, digest)
        return selected

    def candidate(self) -> dict[str, Any]:
        value = read_json(self.operation / "candidate.json")
        keys(value, {"adapter_directory", "adapter_sha256", "entry"})
        require(value["entry"] == self.request["entry"], "candidate_entry")
        self.adapter(value)
        return value

    def validate(self) -> dict[str, Any]:
        context = self.context()
        require(self.profile["context"] == context
                and self.request["evaluation_policy"] ==
                self.profile["evaluation_policy"], "mlx_policy_or_context_binding")
        baseline = read_json(self.root / "accepted.json")
        require(baseline == self.request["baseline"]
                and baseline["runtime_sha256"] == context["runtime_sha256"]
                and baseline["healthy"] is True
                and baseline["rollback_verified"] is True,
                "baseline_generation_changed")
        runtime = self.runtime()
        require(runtime["generation"] == baseline["generation"]
                and runtime["adapter_sha256"] == baseline["adapter_sha256"]
                and (runtime["adapter_sha256"] or self.selection.model_sha256)
                == baseline["served_artifact_sha256"], "baseline_runtime_binding")
        canary = self.profile["canary"]
        keys(canary, {"prompts", "max_tokens", "maximum_latency_ms"})
        require(isinstance(canary["prompts"], list)
                and 1 <= len(canary["prompts"]) <= 4
                and len(set(canary["prompts"])) == len(canary["prompts"])
                and all(isinstance(prompt, str)
                        and 1 <= len(prompt.encode()) <= 256
                        for prompt in canary["prompts"])
                and type(canary["max_tokens"]) is int
                and 1 <= canary["max_tokens"] <= 64
                and type(canary["maximum_latency_ms"]) is int
                and 100 <= canary["maximum_latency_ms"] <= 120_000,
                "canary_bounds")
        quality = self.profile["quality"]
        keys(quality, {"expected_output_sha256", "maximum_peak_memory_bytes"})
        require(isinstance(quality["expected_output_sha256"], list)
                and len(quality["expected_output_sha256"]) == len(canary["prompts"])
                and all(isinstance(value, str) and DIGEST.fullmatch(value)
                        for value in quality["expected_output_sha256"])
                and type(quality["maximum_peak_memory_bytes"]) is int
                and 536_870_912 <= quality["maximum_peak_memory_bytes"] <=
                self.selection.memory_limit_bytes,
                "quality_policy_bounds")
        policy = self.profile["evaluation_policy"]
        keys(policy, {"minimum_samples", "maximum_age_ms", "metrics"})
        require(type(policy["minimum_samples"]) is int
                and 4 <= policy["minimum_samples"] <= 256
                and type(policy["maximum_age_ms"]) is int
                and 1 <= policy["maximum_age_ms"] <= 3_600_000
                and set(policy["metrics"]) == {"eval_loss"},
                "evaluation_policy_bounds")
        if self.request["entry"] == "import":
            require(isinstance(self.input["adapter_directory"], str)
                    and DIGEST.fullmatch(self.input["adapter_sha256"]) is not None,
                    "import_artifact_required")
            _adapter(self.selection, Path(self.input["adapter_directory"]),
                     self.input["adapter_sha256"])
            write(self.operation / "candidate.json", encode({
                "adapter_directory": self.input["adapter_directory"],
                "adapter_sha256": self.input["adapter_sha256"], "entry": "import"}))
        else:
            require(self.request["entry"] == "train"
                    and self.input["adapter_directory"] is None
                    and self.input["adapter_sha256"] is None,
                    "training_input_required")
        self._start_service(runtime)
        write(self.operation / "baseline-behavior.json",
              encode(self._behavior(runtime)))
        incumbent = self.canary(rollback=True)
        return {"context": context, "device": observed_metal(self.selection),
                "source_sha256": self.profile["source_sha256"],
                "verified_incumbent": incumbent}

    def train(self) -> dict[str, Any]:
        require(self.request["entry"] == "train", "training_entry_required")
        output = self.operation / "training"
        require(not output.exists(), "ambiguous_training_already_started")
        output.mkdir(mode=0o700)
        settings = self.profile["settings"]
        result = train_lora(self.selection, output, steps=settings["steps"],
                            rank=settings["rank"], seed=settings["seed"],
                            deadline_unix_ms=self.authority_expiry,
                            cancelled=lambda: (self.operation /
                                               "forward-cancelled.json").exists())
        write(self.operation / "candidate.json", encode({
            "adapter_directory": str(output),
            "adapter_sha256": result.adapter_sha256, "entry": "train"}))
        return {"adapter_sha256": result.adapter_sha256,
                "training": result.__dict__, "native_checkpoint_resume": False}

    def _behavior(self, runtime: dict[str, Any]) -> dict[str, Any]:
        adapter = self.adapter(runtime)
        texts = []
        resources = []
        output_hashes = []
        for prompt in self.profile["canary"]["prompts"]:
            self.authority()
            result = infer(self.selection, prompt, self.profile["canary"]["max_tokens"],
                           adapter, runtime["adapter_sha256"])
            texts.append(result.text)
            output_hashes.append(result.output_sha256)
            resources.append({"latency_ms": result.elapsed_ms,
                              "peak_memory_bytes": result.peak_memory_bytes,
                              "device_name": result.device_name})
        return {"texts": texts, "output_sha256": output_hashes,
                "resources": resources}

    def evaluate(self) -> dict[str, Any]:
        baseline = read_json(self.root / "accepted.json")
        runtime = self.runtime()
        require(baseline == self.request["baseline"]
                and runtime["generation"] == baseline["generation"],
                "baseline_generation_changed")
        candidate = self.candidate()
        result = {}
        for label, selected in (("baseline", runtime), ("candidate", candidate)):
            self.authority()
            native = evaluate_heldout(self.selection,
                                      self.adapter(selected), selected["adapter_sha256"])
            report_sha = self.store(encode(native))
            artifact = selected["adapter_sha256"] or self.selection.model_sha256
            result[label] = {
                "schema": "cohesix-native-evaluation/v1",
                "artifact_sha256": artifact,
                "context": self.profile["context"],
                "samples": native["samples"],
                "completed_unix_ms": native["completed_unix_ms"],
                "metrics": {"eval_loss": native["eval_loss"]},
                "native_report_sha256": report_sha,
            }
            behavior = self._behavior(selected)
            if label == "candidate":
                require(behavior["output_sha256"] ==
                        self.profile["quality"]["expected_output_sha256"]
                        and all(value["latency_ms"] <=
                                self.profile["canary"]["maximum_latency_ms"]
                                and value["peak_memory_bytes"] <=
                                self.profile["quality"]["maximum_peak_memory_bytes"]
                                for value in behavior["resources"]),
                        "candidate_narrow_quality_or_resource_gate")
            write(self.operation / f"{label}-behavior.json", encode(behavior))
        return result

    def scan(self) -> dict[str, Any]:
        candidate = self.candidate()
        return {"adapter_sha256": candidate["adapter_sha256"],
                "adapter_files": len(list(Path(candidate["adapter_directory"]).iterdir())),
                "format": "mlx-lora-safetensors"}

    def stage(self) -> dict[str, Any]:
        candidate = self.candidate()
        source = Path(candidate["adapter_directory"])
        destination = self.root / "staged" / candidate["adapter_sha256"]
        require(not destination.exists(), "ambiguous_stage_already_started")
        destination.parent.mkdir(mode=0o700, exist_ok=True)
        destination.mkdir(mode=0o700)
        for source_file in sorted(source.iterdir()):
            value = regular(source_file, MAX_ADAPTER_BYTES)
            target = destination / source_file.name
            with target.open("xb") as stream:
                os.chmod(target, 0o600)
                stream.write(value)
                stream.flush()
                os.fsync(stream.fileno())
        require(tree_digest(destination, MAX_ADAPTER_BYTES, 4)
                == candidate["adapter_sha256"], "staged_adapter_changed")
        staged = {"generation": self.request["baseline"]["generation"] + 1,
                  "adapter_directory": str(destination),
                  "adapter_sha256": candidate["adapter_sha256"]}
        write(self.operation / "staged.json", encode(staged))
        return staged

    def _selection_file(self, runtime: dict[str, Any]) -> Path:
        selection = {
            "generation": runtime["generation"],
            "model_directory": str(self.selection.model_directory),
            "model_sha256": self.selection.model_sha256,
            "data_directory": str(self.selection.data_directory),
            "data_sha256": self.selection.data_sha256,
            "memory_limit_bytes": self.selection.memory_limit_bytes,
            "adapter_directory": runtime["adapter_directory"],
            "adapter_sha256": runtime["adapter_sha256"],
        }
        path = self.root / "serving-selection.json"
        write(path, encode(selection))
        return path

    def _start_service(self, runtime: dict[str, Any]) -> dict[str, str]:
        label = self.config["service_label"]
        _stop_service(label, self.root)
        selection_path = self._selection_file(runtime)
        logs = self.root / "service-logs"
        logs.mkdir(mode=0o700, exist_ok=True)
        require(not logs.is_symlink() and logs.stat().st_mode & 0o077 == 0,
                "private_service_logs_required")
        result = subprocess.run([
            "/bin/launchctl", "submit", "-l", label,
            "-o", str(logs / "stdout"), "-e", str(logs / "stderr"), "--",
            "/usr/bin/env", "-i", f'HOME={self.root}', f'TMPDIR={self.root}',
            "PATH=/usr/bin:/bin", "PYTHONNOUSERSITE=1", "HF_HUB_OFFLINE=1",
            "TRANSFORMERS_OFFLINE=1", self.config["python"], "-I", "-m",
            "cohesix.mlx_service", "--selection", str(selection_path),
            "--port", str(self.config["port"]),
        ], capture_output=True, check=False, timeout=15)
        require(result.returncode == 0 and len(result.stdout) <= 8192
                and len(result.stderr) <= 8192, "ambiguous_service_start")
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            state = _service_state(label)
            if state is None or state.get("PID") in {None, "0"}:
                require(state is None or state.get("LastExitStatus") in {None, "0"},
                        "service_exited_before_ready")
                time.sleep(0.1)
                continue
            try:
                with urllib.request.urlopen(
                    f'http://127.0.0.1:{self.config["port"]}/health', timeout=1
                ) as response:
                    health = json.loads(response.read(8193))
            except (OSError, ValueError):
                time.sleep(0.25)
                continue
            expected = runtime["adapter_sha256"] or self.selection.model_sha256
            require(health["model"] ==
                    f'cohesix-g{runtime["generation"]}-{expected}'
                    and health["generation"] == runtime["generation"]
                    and health["device_name"] ==
                    observed_metal(self.selection)["device_name"],
                    "service_generation_or_device_changed")
            return state
        raise ValueError("service_readiness_deadline")

    def load(self) -> dict[str, Any]:
        previous = self.runtime()
        require(read_json(self.root / "accepted.json") == self.request["baseline"],
                "baseline_generation_changed")
        write(self.operation / "rollback-runtime.json", encode(previous))
        staged = read_json(self.operation / "staged.json")
        self.adapter(staged)
        self.authority()
        write(self.root / "runtime.json", encode(staged))
        return {"runtime": staged, "service": self._start_service(staged)}

    def canary(self, rollback: bool = False) -> dict[str, Any]:
        runtime = self.runtime()
        baseline = self.request["baseline"]
        artifact = (baseline["served_artifact_sha256"] if rollback
                    else self.candidate()["adapter_sha256"])
        require((runtime["adapter_sha256"] or self.selection.model_sha256)
                == artifact, "served_artifact_changed")
        reference = read_json(self.operation /
                              ("baseline-behavior.json" if rollback
                               else "candidate-behavior.json"))
        before = _service_state(self.config["service_label"])
        require(before is not None and before.get("PID") not in {None, "0"},
                "serving_process_missing")
        digests, latencies, resources = [], [], []
        for prompt, expected in zip(self.profile["canary"]["prompts"],
                                    reference["texts"], strict=True):
            start = time.monotonic()
            model = f'cohesix-g{runtime["generation"]}-{artifact}'
            request = urllib.request.Request(
                f'http://127.0.0.1:{self.config["port"]}/v1/chat/completions',
                data=encode({"model": model, "messages": [{"role": "user",
                                                          "content": prompt}],
                             "max_tokens": self.profile["canary"]["max_tokens"],
                             "temperature": 0, "stream": False}),
                headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=120) as response:
                payload = response.read(16_385)
            require(len(payload) <= 16_384, "canary_response_bound")
            reply = json.loads(payload)
            actual = reply["choices"][0]["message"]["content"]
            elapsed = int((time.monotonic() - start) * 1000)
            evidence = reply["cohesix_observation"]
            require(reply["model"] == model and actual == expected
                    and evidence["device_name"] ==
                    observed_metal(self.selection)["device_name"]
                    and evidence["peak_memory_bytes"] <=
                    self.profile["quality"]["maximum_peak_memory_bytes"]
                    and elapsed <= self.profile["canary"]["maximum_latency_ms"],
                    "canary_behavior_resource_or_latency")
            digests.append(sha(actual.encode()))
            latencies.append(elapsed)
            resources.append(evidence)
        after = _service_state(self.config["service_label"])
        require(after is not None and before.get("PID") == after.get("PID"),
                "serving_process_changed_during_canary")
        return {"runtime": runtime, "service": after,
                "behavior_sha256": digests, "latency_ms": latencies,
                "resources": resources, "healthy": True}

    def promote(self) -> dict[str, Any]:
        current = read_json(self.root / "accepted.json")
        require(current == self.request["baseline"], "generation_conflict")
        staged = read_json(self.operation / "staged.json")
        require(self.runtime() == staged, "runtime_changed_before_promote")
        observed = self.canary()
        self.authority()
        evaluations = read_json(self.operation / "evaluate.json")["detail"]
        maximum_age = self.request["evaluation_policy"]["maximum_age_ms"]
        require(all(0 <= _now() - evaluations[key]["completed_unix_ms"]
                    < maximum_age for key in ("candidate", "baseline")),
                "evaluation_expired")
        accepted = {**current, "generation": staged["generation"],
                    "adapter_sha256": staged["adapter_sha256"],
                    "served_artifact_sha256": staged["adapter_sha256"],
                    "healthy": True, "rollback_verified": True}
        write(self.root / "accepted.json", encode(accepted))
        return {"accepted": accepted, "canary": observed}

    def rollback(self) -> dict[str, Any]:
        target = self.operation / "rollback-runtime.json"
        previous = read_json(target) if target.exists() else self.runtime()
        baseline = self.request["baseline"]
        require(previous["generation"] == baseline["generation"]
                and (previous["adapter_sha256"] or self.selection.model_sha256)
                == baseline["served_artifact_sha256"],
                "unestablished_recovery_target")
        reference = self.operation / "baseline-behavior.json"
        if not reference.exists():
            write(reference, encode(self._behavior(previous)))
        self.authority()
        write(self.root / "runtime.json", encode(previous))
        _stop_service(self.config["service_label"], self.root)
        self._start_service(previous)
        observed = self.canary(rollback=True)
        self.authority()
        write(self.root / "accepted.json", encode(baseline))
        return {"accepted": baseline, "canary": observed,
                "candidate_release": "failed"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--request", type=Path, required=True)
    parser.add_argument("--phase", choices=PHASES, required=True)
    parser.add_argument("--native-label", required=True)
    parser.add_argument("--authority-expires-unix-ms", type=int, required=True)
    parser.add_argument("--recovery-id")
    args = parser.parse_args()
    require(re.fullmatch(r"cohesix-lora-phase-[0-9a-f]{16}-(?:" +
                         "|".join(PHASES) + r")\Z", args.native_label) is not None,
            "native_phase_label")
    provider = Provider(args.config, args.request, args.helper)
    provider.authority_expiry = args.authority_expires_unix_ms
    provider.phase = args.phase
    target = provider.operation / f"{args.phase}.json"
    if args.recovery_id is not None:
        require(args.phase == "rollback"
                and IDENTIFIER.fullmatch(args.recovery_id) is not None,
                "recovery_scope")
        target = provider.operation / f"recovery-{args.recovery_id}" / "rollback.json"
    with phase_lock(provider.operation, args.phase == "rollback"):
        require(not target.exists(), "duplicate_native_phase_refused")
        succeeded = True
        try:
            provider.authority()
            detail = getattr(provider, args.phase)()
            provider.authority()
        except Exception as error:
            succeeded = False
            detail = {"error": type(error).__name__, "reason": str(error)[:512]}
        usage = resource.getrusage(resource.RUSAGE_SELF)
        prior_cpu_us = 0
        observed = 0
        for phase in PHASES:
            previous = provider.operation / f"{phase}.json"
            if previous.exists():
                resources = read_json(previous)["detail"].get("resources")
                if resources is not None:
                    prior_cpu_us += resources["process_cpu_us"]
                    observed += 1
        detail["resources"] = {
            "process_cpu_us": int((usage.ru_utime + usage.ru_stime) * 1_000_000),
            "process_peak_host_rss_bytes": usage.ru_maxrss,
            "cumulative_observed_cpu_us": prior_cpu_us +
            int((usage.ru_utime + usage.ru_stime) * 1_000_000),
            "completed_phase_observations": observed + 1,
            "metal_allocation_is_hard_partition": False,
            "serving_allocation_released": False,
        }
        observation = {
            "operation_id": provider.request["operation_id"],
            "request_sha256": provider.request_sha,
            "phase": args.phase,
            "native_identity": "launchd:" + args.native_label,
            "completed_unix_ms": _now(),
            "succeeded": succeeded,
            "detail": detail,
        }
        write(target, encode(observation))
        sys.stdout.buffer.write(encode(observation) + b"\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
