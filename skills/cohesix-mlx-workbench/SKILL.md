---
name: cohesix-mlx-workbench
description: Run private local MLX inference, bounded LoRA training and held-out comparison in Cohesix on Apple Silicon. Use for the SwarmUI Local MLX desk or Python experiments; admitted deployment and promotion use the release skills.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Help Mac users produce useful local Metal observations without inferring governed deployment. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Compare a private model on your Mac

Use this for a small local task such as extracting fields from example notes,
drafting a support reply or testing a private adapter. Define the desired
output and held-out quality criterion before running candidates. Keep private
examples and generated text local. This workflow needs Apple Silicon and Metal;
it does not need a Queen, gateway, CUDA or a deployment ticket.

Read [SwarmUI's Local MLX guide](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/SWARMUI.md#run-a-local-metal-model-on-mac)
and the [Python/native contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md#swarmui)
at the installed revision, replacing `main` with its tag/commit or using the
matching local guides. If MLX or the matching Cohesix package is missing, load
`cohesix-ai-host-setup`. Use a private environment with the selected package's
`apple-mlx` extra; 1.2.0 pins `mlx==0.32.2` and `mlx-lm==0.31.3`.

## Freeze compatible local inputs

Choose an already downloaded, licensed MLX model and independent dataset.
If a download is needed, select its immutable upstream revision and size before
fetching; the native helper accepts local bytes, not a Hub model name.
Materialize files into private ordinary directories: the helper rejects
symlink paths, including a symlink-based model cache.

The dataset directory contains exactly `train.jsonl`, `valid.jsonl` and
`test.jsonl`, with at least 16, 4 and 4 rows respectively. Each row has the
single `text` field (16–2048 UTF-8 bytes), with unique rows and no text overlap
across splits. Each file has at most 256 rows and 262144 bytes; each row is at
most 4096 bytes. Compute content hashes
with `cohesix.mlx_native.tree_digest` and retain a private selection JSON with
`model_directory`, `model_sha256`, `data_directory`, `data_sha256` and
`memory_limit_bytes`. Choose memory for the actual Mac within the helper's
512 MiB–12 GiB bound. Optional `adapter_directory` and `adapter_sha256` must
be paired and satisfy the native adapter provenance checks; an arbitrary
downloaded LoRA is not automatically compatible.

## Produce a comparison

For desktop use, open SwarmUI's **Local MLX** desk and select the absolute
Python virtual-environment directory and private selection file. **Run local
inference** accepts 1–64 new tokens and up to 2048 UTF-8 prompt bytes.
**Measure held-out loss** uses the independent test split. Record the observed
Metal device, hashes, memory, response/loss and any refusal.

For a Python workflow, use the same package's `MlxSelection`, `infer` and
`evaluate_heldout`. To train a candidate, call `train_lora` with a fresh empty
private output directory, 2–64 steps, rank 2/4/8, explicit seed, finite absolute
deadline and a cancellation callback. Follow the exact installed signatures
and output-directory permissions. Freeze adapter identity after training;
compare base and adapter on the same test split and application examples.
SwarmUI's two local buttons perform inference/evaluation; training uses Python.

The fixed stdin interface also supports one prepared inference request:

```bash
: "${MLX_PYTHON:?set the absolute matching environment Python executable}"
: "${MLX_REQUEST:?set the private JSON request file}"
"$MLX_PYTHON" -m cohesix.mlx_workbench < "$MLX_REQUEST"
```

That JSON has exactly `selection_path`, `operation`, `prompt`, `max_tokens`;
use `"infer"` with bounded prompt/tokens, or `"evaluate"`, `""`, `0`.
Inspect the JSON `proof_class`: a process exit of zero can contain
`local_refusal`. Preserve the reason and correct the input/runtime mismatch;
never weaken hash, device, dataset or memory checks to force a result.

Return a local comparison with frozen inputs, candidate quality and resource
observations. Lower held-out loss alone does not establish application quality.
Optional vMLX serving uses a separately selected disposable model copy and
compatibility check. Neither a local generation label nor model text proves
promotion. For an admitted release, load `cohesix-private-adapter-release` or
`cohesix-model-rollout` and require an advertised, qualified native profile;
the 28c1 Mac admission lifecycle remains planned in the 1.2.0 scope.
