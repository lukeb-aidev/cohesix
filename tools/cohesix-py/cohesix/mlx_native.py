# Author: Lukas Bower
# Purpose: Run content-bound local MLX inference and LoRA work on an observed Apple Metal device for admitted host phases.
# Copyright 2026 Lukas Bower
"""Bounded native MLX operations. A caller must supply Cohesix admission.

This module observes compute and artifacts; it never creates a ticket, promotes
a deployment, or treats its own return value as a signed Cohesix outcome.
"""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import importlib.metadata
import json
import math
import os
import platform
from pathlib import Path
import re
import stat
import tempfile
import time
from types import SimpleNamespace
from typing import Any, Callable


MAX_MODEL_FILES = 32
MAX_MODEL_FILE_BYTES = 1_073_741_824
MAX_DATA_FILE_BYTES = 262_144
MAX_ADAPTER_BYTES = 67_108_864
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")
PINNED_VERSIONS = {"mlx": "0.32.2", "mlx-lm": "0.31.3"}


class MlxRefusal(ValueError):
    """A selected identity, device, bound or observed native result is invalid."""


def require(condition: bool, reason: str) -> None:
    """Keep unsupported inputs and missing observations outside native execution."""
    if not condition:
        raise MlxRefusal(reason)


def _regular(path: Path, maximum: int) -> bytes:
    """Read one selected ordinary file with no symlink component or size race."""
    require(path.is_absolute() and ".." not in path.parts
            and not any(part.is_symlink() for part in (path, *path.parents)),
            "mlx_absolute_regular_path_required")
    directory = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            next_directory = os.open(component, os.O_RDONLY | os.O_DIRECTORY |
                                     os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = next_directory
        descriptor = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW,
                             dir_fd=directory)
        with os.fdopen(descriptor, "rb") as stream:
            state = os.fstat(stream.fileno())
            require(stat.S_ISREG(state.st_mode) and state.st_size <= maximum,
                    "mlx_file_kind_or_size")
            data = stream.read(maximum + 1)
    except OSError as error:
        raise MlxRefusal("mlx_file_path_or_kind") from error
    finally:
        os.close(directory)
    require(len(data) <= maximum, "mlx_file_size")
    return data


def tree_digest(directory: Path, maximum_file_bytes: int,
                maximum_files: int = MAX_MODEL_FILES) -> str:
    """Bind all regular immediate children before and after native loading."""
    require(directory.is_absolute() and directory.is_dir()
            and not any(part.is_symlink() for part in (directory, *directory.parents)),
            "mlx_absolute_directory_required")
    files = sorted(directory.iterdir())
    require(1 <= len(files) <= maximum_files, "mlx_file_count")
    digest = hashlib.sha256()
    for path in files:
        require(NAME.fullmatch(path.name) is not None and path.is_file()
                and not path.is_symlink(), "mlx_file_name_or_kind")
        data = _regular(path, maximum_file_bytes)
        digest.update(len(path.name).to_bytes(2, "big"))
        digest.update(path.name.encode())
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(hashlib.sha256(data).digest())
    return digest.hexdigest()


def _rows(path: Path) -> list[dict[str, str]]:
    """Require bounded independent text rows in a local MLX-LM data file."""
    raw = _regular(path, MAX_DATA_FILE_BYTES)
    lines = raw.splitlines()
    require(1 <= len(lines) <= 256, "mlx_data_row_count")
    rows = []
    for line in lines:
        require(0 < len(line) <= 4096, "mlx_data_row_size")
        try:
            row = json.loads(line)
        except (UnicodeError, ValueError) as error:
            raise MlxRefusal("mlx_data_json") from error
        require(isinstance(row, dict) and set(row) == {"text"}
                and isinstance(row["text"], str)
                and 16 <= len(row["text"].encode()) <= 2048,
                "mlx_text_row_required")
        rows.append(row)
    require(len({row["text"] for row in rows}) == len(rows),
            "mlx_duplicate_rows")
    return rows


def _write_atomic(path: Path, data: bytes) -> None:
    """Replace a native output only after its bytes and containing directory sync."""
    temporary: str | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".mlx-",
                                         delete=False) as stream:
            temporary = stream.name
            os.chmod(temporary, 0o600)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        temporary = None
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary is not None:
            os.unlink(temporary)


def _adapter(selection: MlxSelection, directory: Path, expected: str) -> None:
    """Reject a changed or unsupported adapter before MLX-LM reads its config."""
    require(SHA256.fullmatch(expected) is not None
            and tree_digest(directory, MAX_ADAPTER_BYTES, 4) == expected,
            "mlx_adapter_changed")
    names = {path.name for path in directory.iterdir()}
    require({"adapter_config.json", "adapters.safetensors"}.issubset(names)
            and all(name in {"adapter_config.json", "adapters.safetensors"}
                    or re.fullmatch(r"[0-9]{7}_adapters\.safetensors", name)
                    for name in names), "mlx_adapter_files")
    try:
        config = json.loads(_regular(directory / "adapter_config.json",
                                     MAX_DATA_FILE_BYTES))
    except (UnicodeError, ValueError) as error:
        raise MlxRefusal("mlx_adapter_config_json") from error
    require(isinstance(config, dict)
            and config.get("fine_tune_type") == "lora"
            and config.get("num_layers") == 4
            and config.get("cohesix_model_sha256") == selection.model_sha256
            and config.get("cohesix_data_sha256") == selection.data_sha256
            and config.get("cohesix_versions") == PINNED_VERSIONS,
            "mlx_adapter_provenance")
    parameters = config.get("lora_parameters")
    require(isinstance(parameters, dict)
            and set(parameters) == {"rank", "scale", "dropout"}
            and type(parameters["rank"]) is int
            and parameters["rank"] in {2, 4, 8}
            and type(parameters["scale"]) in {int, float}
            and math.isfinite(parameters["scale"])
            and parameters["scale"] == 20.0
            and parameters["dropout"] == 0.0,
            "mlx_adapter_parameters")


@dataclass(frozen=True)
class MlxSelection:
    """Exact local inputs chosen before executing a native phase."""

    model_directory: Path
    model_sha256: str
    data_directory: Path
    data_sha256: str
    memory_limit_bytes: int

    def validate(self) -> None:
        """Refuse changed model/data bytes, remote names and unbounded memory."""
        require(SHA256.fullmatch(self.model_sha256) is not None
                and SHA256.fullmatch(self.data_sha256) is not None,
                "mlx_selection_digest")
        require(type(self.memory_limit_bytes) is int
                and 536_870_912 <= self.memory_limit_bytes <= 12_884_901_888,
                "mlx_memory_limit")
        require(tree_digest(self.model_directory, MAX_MODEL_FILE_BYTES)
                == self.model_sha256, "mlx_model_changed")
        require(tree_digest(self.data_directory, MAX_DATA_FILE_BYTES, 3)
                == self.data_sha256, "mlx_data_changed")
        files = {path.name for path in self.data_directory.iterdir()}
        require(files == {"train.jsonl", "valid.jsonl", "test.jsonl"},
                "mlx_dataset_files")
        train, valid, test = (_rows(self.data_directory / f"{name}.jsonl")
                              for name in ("train", "valid", "test"))
        require(len(train) >= 16 and len(valid) >= 4 and len(test) >= 4,
                "mlx_data_samples")
        require(not set(row["text"] for row in train).intersection(
                    row["text"] for row in valid + test)
                and not set(row["text"] for row in valid).intersection(
                    row["text"] for row in test), "mlx_data_leakage")


def observed_metal(selection: MlxSelection) -> dict[str, Any]:
    """Pin the selected packages and require an actual Metal GPU."""
    selection.validate()
    require(platform.system() == "Darwin" and platform.machine() == "arm64",
            "mlx_apple_silicon_required")
    require({name: importlib.metadata.version(name) for name in PINNED_VERSIONS}
            == PINNED_VERSIONS, "mlx_version_mismatch")
    import mlx.core as mx
    require(mx.metal.is_available(), "mlx_metal_unavailable")
    mx.set_default_device(mx.gpu)
    mx.set_memory_limit(selection.memory_limit_bytes)
    require(str(mx.default_device()).startswith("Device(gpu,"),
            "mlx_gpu_device_required")
    info = mx.device_info()
    require(isinstance(info, dict) and info.get("device_name"),
            "mlx_device_identity_missing")
    return {"device": str(mx.default_device()),
            "device_name": info["device_name"],
            "memory_limit_bytes": selection.memory_limit_bytes,
            "versions": dict(PINNED_VERSIONS)}


@dataclass(frozen=True, repr=False)
class MlxInference:
    """Private generated text and reportable GPU/artifact measurements."""

    text: str
    prompt_sha256: str
    output_sha256: str
    model_sha256: str
    adapter_sha256: str | None
    elapsed_ms: int
    peak_memory_bytes: int
    device_name: str

    def evidence(self) -> dict[str, str | int | None]:
        """Report native identity and bounds without logging prompt or output."""
        return {key: getattr(self, key) for key in (
            "prompt_sha256", "output_sha256", "model_sha256", "adapter_sha256",
            "elapsed_ms", "peak_memory_bytes", "device_name")}


def infer(selection: MlxSelection, prompt: str, max_tokens: int,
          adapter_directory: Path | None = None,
          adapter_sha256: str | None = None) -> MlxInference:
    """Run one local bounded inference without an implicit remote or CPU fallback."""
    require(isinstance(prompt, str) and 1 <= len(prompt.encode()) <= 2048,
            "mlx_prompt_bound")
    require(type(max_tokens) is int and 1 <= max_tokens <= 64,
            "mlx_token_bound")
    require((adapter_directory is None) == (adapter_sha256 is None),
            "mlx_adapter_selection")
    if adapter_directory is not None:
        _adapter(selection, adapter_directory, adapter_sha256)
    device = observed_metal(selection)
    import mlx.core as mx
    import mlx_lm
    mx.reset_peak_memory()
    started = time.monotonic()
    model, tokenizer = mlx_lm.load(
        str(selection.model_directory),
        tokenizer_config={"trust_remote_code": False},
        adapter_path=str(adapter_directory) if adapter_directory else None)
    formatted = tokenizer.apply_chat_template(
        [{"role": "user", "content": prompt}], tokenize=False,
        add_generation_prompt=True)
    require(isinstance(formatted, str) and 1 <= len(formatted.encode()) <= 4096,
            "mlx_formatted_prompt_bound")
    text = mlx_lm.generate(model, tokenizer, formatted, verbose=False,
                           max_tokens=max_tokens)
    elapsed_ms = int((time.monotonic() - started) * 1000)
    require(isinstance(text, str) and 0 < len(text.encode()) <= 8192,
            "mlx_output_bound")
    require(tree_digest(selection.model_directory, MAX_MODEL_FILE_BYTES)
            == selection.model_sha256, "mlx_model_mutated")
    if adapter_directory is not None:
        require(tree_digest(adapter_directory, MAX_ADAPTER_BYTES, 4)
                == adapter_sha256, "mlx_adapter_mutated")
    return MlxInference(text, hashlib.sha256(formatted.encode()).hexdigest(),
                        hashlib.sha256(text.encode()).hexdigest(),
                        selection.model_sha256, adapter_sha256, elapsed_ms,
                        mx.get_peak_memory(), device["device_name"])


@dataclass(frozen=True)
class MlxTraining:
    """Observed local training artifact and resource use, without deployment authority."""

    adapter_sha256: str
    model_sha256: str
    data_sha256: str
    steps: int
    peak_memory_bytes: int
    device_name: str
    completed_unix_ms: int


def train_lora(selection: MlxSelection, output_directory: Path,
               *, steps: int, rank: int, seed: int, deadline_unix_ms: int,
               cancelled: Callable[[], bool]) -> MlxTraining:
    """Train a bounded local adapter and retain resource and artifact identity."""
    require(type(steps) is int and 2 <= steps <= 64
            and rank in {2, 4, 8} and type(seed) is int and 0 <= seed < 2**32,
            "mlx_training_bounds")
    require(output_directory.is_absolute() and output_directory.is_dir()
            and not any(part.is_symlink() for part in
                        (output_directory, *output_directory.parents))
            and output_directory.stat().st_mode & 0o077 == 0
            and not any(output_directory.iterdir()), "mlx_empty_private_output")
    require(output_directory != selection.model_directory
            and output_directory != selection.data_directory
            and selection.model_directory not in output_directory.parents
            and selection.data_directory not in output_directory.parents,
            "mlx_output_separate_from_inputs")
    require(type(deadline_unix_ms) is int
            and int(time.time() * 1000) < deadline_unix_ms, "mlx_deadline")
    device = observed_metal(selection)
    import mlx.core as mx
    import mlx_lm
    import numpy as np
    from mlx_lm import lora
    from mlx_lm.tuner.callbacks import TrainingCallback

    class CheckAuthority(TrainingCallback):
        def on_train_loss_report(self, _info: dict[str, Any]) -> None:
            require(not cancelled() and int(time.time() * 1000) < deadline_unix_ms,
                    "mlx_training_cancelled_or_expired")

        def on_val_loss_report(self, _info: dict[str, Any]) -> None:
            self.on_train_loss_report(_info)

    mx.reset_peak_memory()
    np.random.seed(seed)
    mx.random.seed(seed)
    model, tokenizer = mlx_lm.load(str(selection.model_directory),
                                   tokenizer_config={"trust_remote_code": False})
    args = SimpleNamespace(
        data=str(selection.data_directory), train=True, test=False,
        seed=seed, num_layers=4, fine_tune_type="lora",
        lora_parameters={"rank": rank, "scale": 20.0, "dropout": 0.0},
        resume_adapter_file=None,
        adapter_path=str(output_directory), batch_size=1, iters=steps,
        val_batches=4, steps_per_report=1, steps_per_eval=steps,
        save_every=max(1, steps // 2), max_seq_length=128,
        grad_checkpoint=False, grad_accumulation_steps=1,
        lr_schedule=None, learning_rate=0.0001,
        optimizer="adam", optimizer_config={},
    )
    train, valid, _test = lora.load_dataset(args, tokenizer)
    require(len(train) >= 16 and len(valid) >= 4, "mlx_data_samples")
    require(not cancelled() and int(time.time() * 1000) < deadline_unix_ms,
            "mlx_training_cancelled_or_expired")
    lora.train_model(args, model, train, valid, CheckAuthority())
    require(not cancelled() and int(time.time() * 1000) < deadline_unix_ms,
            "mlx_training_cancelled_or_expired")
    require((output_directory / "adapters.safetensors").is_file(),
            "mlx_adapter_missing")
    generated = json.loads(_regular(output_directory / "adapter_config.json",
                                    MAX_DATA_FILE_BYTES))
    require(isinstance(generated, dict)
            and generated.get("fine_tune_type") == "lora"
            and generated.get("num_layers") == 4
            and generated.get("lora_parameters") == args.lora_parameters,
            "mlx_generated_adapter_config")
    generated.pop("adapter_path", None)
    generated.pop("data", None)
    generated["cohesix_model_sha256"] = selection.model_sha256
    generated["cohesix_data_sha256"] = selection.data_sha256
    generated["cohesix_versions"] = PINNED_VERSIONS
    _write_atomic(output_directory / "adapter_config.json",
                  json.dumps(generated, sort_keys=True, separators=(",", ":"),
                             allow_nan=False).encode())
    require(tree_digest(output_directory, MAX_ADAPTER_BYTES, 4),
            "mlx_adapter_digest")
    require(tree_digest(selection.model_directory, MAX_MODEL_FILE_BYTES)
            == selection.model_sha256, "mlx_model_mutated")
    adapter_sha256 = tree_digest(output_directory, MAX_ADAPTER_BYTES, 4)
    _adapter(selection, output_directory, adapter_sha256)
    return MlxTraining(adapter_sha256,
                       selection.model_sha256, selection.data_sha256,
                       steps, mx.get_peak_memory(), device["device_name"],
                       int(time.time() * 1000))


def evaluate_heldout(selection: MlxSelection,
                     adapter_directory: Path | None = None,
                     adapter_sha256: str | None = None) -> dict[str, str | int | float | None]:
    """Measure loss on the exact independent local test split using Metal."""
    require((adapter_directory is None) == (adapter_sha256 is None),
            "mlx_adapter_selection")
    if adapter_directory is not None:
        _adapter(selection, adapter_directory, adapter_sha256)
    device = observed_metal(selection)
    import mlx.core as mx
    import mlx_lm
    from mlx_lm import lora
    mx.reset_peak_memory()
    model, tokenizer = mlx_lm.load(
        str(selection.model_directory),
        tokenizer_config={"trust_remote_code": False},
        adapter_path=str(adapter_directory) if adapter_directory else None)
    args = SimpleNamespace(data=str(selection.data_directory),
                           train=False, test=True)
    _train, _valid, test = lora.load_dataset(args, tokenizer)
    samples = len(test)
    require(4 <= samples <= 256, "mlx_heldout_sample_count")
    loss = lora.evaluate(model, lora.CacheDataset(test), batch_size=1,
                         num_batches=samples, max_seq_length=128)
    require(type(loss) in {int, float} and math.isfinite(loss)
            and loss >= 0, "mlx_heldout_loss")
    require(tree_digest(selection.model_directory, MAX_MODEL_FILE_BYTES)
            == selection.model_sha256
            and tree_digest(selection.data_directory, MAX_DATA_FILE_BYTES, 3)
            == selection.data_sha256, "mlx_input_mutated")
    if adapter_directory is not None:
        require(tree_digest(adapter_directory, MAX_ADAPTER_BYTES, 4)
                == adapter_sha256, "mlx_adapter_mutated")
    return {"model_sha256": selection.model_sha256,
            "data_sha256": selection.data_sha256,
            "adapter_sha256": adapter_sha256,
            "samples": samples, "eval_loss": float(loss),
            "peak_memory_bytes": mx.get_peak_memory(),
            "device_name": device["device_name"],
            "completed_unix_ms": int(time.time() * 1000)}
