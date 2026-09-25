<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Track the incomplete M28c native MLX component and its measured Mac proof limits. -->
<!-- Author: Lukas Bower -->

# Milestone 28c native MLX component record

`m28c-mlx-metal-provider` remains **In Progress**. The native Python component
now rejects remote or changed model and dataset inputs, pins MLX 0.32.2 and
MLX-LM 0.31.3, requires the observed Metal GPU, caps unified memory, and
returns hashed prompt/output and resource observations. Training checks a
deadline and cancellation at every loss report; MLX-LM's final adapter config
is normalized to content and settings rather than scratch output paths.
The code does not admit a Cohesix job or promote a generation.

```text
Title/ID: m28c-mlx-metal-provider
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Run selected local MLX inference and LoRA through the shared admitted release lifecycle.
Inputs: accepted M28b lifecycle; M28c Apple feasibility; selected M4, MLX and local model/data.
Changes: tools/cohesix-py/cohesix/mlx_native.py + tools/cohesix-py/tests/test_mlx_native.py implement and check bounded native primitives. The host-ticket-agent, Hive Gateway, coh, SwarmUI and selected provider graph still need the admitted integration.
Commands: PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_mlx_native.py; local content-bound MLX inference, train, held-out evaluate and repeatability probes in ignored out/m28c.
Checks: Four focused pure refusal tests pass. The selected Mac completed genuine four-step LoRA on Metal and two same-seed runs with the 16-row held-out dataset produced identical adapter bundle digests. The held-out comparison improved loss versus the base; no predeclared 28b policy or independent admitted comparison has been run.
Deliverables: Local MLX primitives and diagnostic compute measurements, not an accepted M28c provider or developer journey.
```

| Local diagnostic observation | Result | Proof limit |
| --- | --- | --- |
| Model and data | Model tree SHA-256 `8b05c3c6097752da054b392d5abab559452fd5c14ae191ee5386130adca4677d`; later dataset tree SHA-256 `0911cc05194749d0045d590e59333bec4d393ca5802c795dc31f9b726ac39961`. Inputs are ignored local copies. | The later fixture has 16 training, four validation and 16 test rows. Row count alone does not establish useful model quality. |
| Metal training | Two four-step LoRA runs on the Apple M4 with the later dataset produced the same adapter tree SHA-256 `98e6b1ff936d2614c1e32a12853f328ab2dc4ac3340d19be7408d95703446e8e`. Peak Metal allocation was about 310–316 MB. | Local MLX-LM process only; no Root ticket, host-agent journal, signed outcome, cancellation/interrupt or Cohesix deployment. |
| Held-out evaluation | Base loss 6.320817947387695; later adapter loss 6.016604900360107 on the same 16 test rows. | Improvement is diagnostic, not predeclared acceptance. Semantic response quality and policy compatibility remain unqualified. |
| Inference | The base and later adapter both produced nonempty bounded output through the on-device chat template. Later adapter output SHA-256 `38add7b12b21120a4860ef45a855d1de7d5173ef7351ac2a3a3921a420826c9d`, with 292,800,832 bytes peak Metal allocation. | No served generation, vMLX canary, quality assertion or signed Cohesix result. Prompt and response text are excluded from status evidence. |

The host-tool/Python/benchmark compatibility review finds this module additive
and unused by the current selected agent, gateway, `coh` and SwarmUI. It does
not change their schemas, ticket actions, benchmark measurements or target
interfaces. Integration must preserve M28b's durable phase journal and signed
result custody; a call to this module alone cannot satisfy that contract.

Material AI assistance contributed the implementation, tests and this record.
The ignored local artifacts are diagnostic and must be replaced by exact-source
live evidence after the admitted implementation is complete.
