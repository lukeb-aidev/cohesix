<!-- Author: Lukas Bower -->
<!-- Purpose: Bound the historical PEFT import fixture to file-registry demonstrations. -->
<!-- Copyright 2026 Lukas Bower -->

# PEFT registry fixture

These historical files exercise the older file-registry import path only.
`adapter.safetensors` contains placeholder text, not safetensors weights;
`lora.json` supplies sample rank metadata and `metrics.json` a synthetic loss.
The JSON files and adapter bytes are retained unchanged for compatibility.

Do not pass this directory to `coh peft release`, a native evaluator or a serving
runtime. Import/activate of these registry bytes proves neither training nor
inference. For the 1.1.0-beta native import/training workflow, follow
[Private LoRA release](../../docs/PRIVATE_LORA_RELEASE.md) and use a genuine
adapter with pinned PEFT configuration and enrolled source provenance.

See the [demo guide](../README.md) for setup, execution and evidence boundaries.
