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
| Diagnostic vMLX conversion | MLX-LM fused the adapter into a separate model tree SHA-256 `4c6876a4c0a78e6b9e902f652af442e8763b3b844e6c3c55a69e8de6ca7ccff2`. Direct MLX held-out loss was 6.0157470703125 on the same 16 rows and its bounded output digest matched the adapter. The installed vMLX 1.6.65 engine served a disposable copy at loopback; the bounded Cohesix client returned the same output SHA-256 `38add7b12b21120a4860ef45a855d1de7d5173ef7351ac2a3a3921a420826c9d`. | vMLX's text adapter load is unsupported by its documented `--lora-paths` image-model option; this proves a conversion and transport route only. The response interpreted “canary release” as a bird and exhausted its token cap, so it fails useful response quality. No Cohesix canary generation was admitted. |
| vMLX mutation and incumbent switch | vMLX repaired 272 tensor alignments only in the disposable fused serving copy; `model.safetensors` changed from SHA-256 `7f31bedbc3de3647f4cb3220e3373303fab2323870215f918af07563a75cdb25` to `9360693cf724310a0a98af35a2b82d494cec0a8981ae9794a4a06cfdd5fe57da`. The fused source tree stayed unchanged. After stopping the candidate server, the same loopback port served a copied incumbent tree with unchanged SHA-256 `8b05c3c6097752da054b392d5abab559452fd5c14ae191ee5386130adca4677d`; the old candidate model ID was refused, and the incumbent output digest `474b6d26060740ef5c0763435e9947ee80b4b37ad5b23cd322e586043b6f8990` matched direct base inference. | This is a manual diagnostic server switch, not Cohesix generation-fenced rollback or a quality-qualified incumbent. Neither server imported Cohesix credentials or enabled MCP tool execution. |
| Client switch refusal | The bounded client now reads health and model listing again after a response; a focused test switches the server's advertised model during inference and the client refuses to return the output. | This detects an observed model switch across a request, not a same-ID weight replacement or an exact process/generation binding. |
| Content-bound vMLX serving session | `cohesix.vmlx_runtime.VmlxSession` verified the installed `net.vmlx.app` 1.6.65 signature, Team `55KGF2S5AY` and engine commit `22f9c77711fb580df32f4d40bbaea989c2d5421b` with entrypoint SHA-256 `6ae7d9f0b5db2035b623fc0cacecc3f572bc46db18b69cc8fd611f71b0c2ac7d`. It copied the unchanged fused source tree into a private disposable directory, served it with a minimal environment and no inherited Cohesix credentials on loopback port 18085 as `cohesix-g3-4c6876a4c0a78e6b`, observed process 23689 and loaded tree SHA-256 `c41783392b1d884e8915cba48f822a73eae0061315f33f080e174ecd8d5291fa`, returned one bounded response, and stopped the process. The source tree remained SHA-256 `4c6876a4c0a78e6b9e902f652af442e8763b3b844e6c3c55a69e8de6ca7ccff2`. | Generation 3 was a local diagnostic label, not an admitted Cohesix deployment. The response exhausted its 32-token bound; no predeclared useful-quality policy or root ticket applied. `VmlxSession` records bytes and process while running but does not retain a signed outcome or arrange recovery after its own process exits. |
| Stronger-model quality attempt | The [MLX Community Qwen2.5-1.5B-Instruct-4bit model](https://huggingface.co/mlx-community/Qwen2.5-1.5B-Instruct-4bit) was pinned to revision `8b403126fc14f14cfc99bb4cfa72ecbc129ea677`, Apache-2.0, and local tree SHA-256 `8b40b6d325ea432fda5d9d9612813d203d44bde7c232b465b0af32454b1d4791`. Before inference or training, ignored policy `out/m28c/qwen-quality-policy.md` was written with SHA-256 `bb0eff0788be397e2d443d623063da3195a995e977ba5081e8e5509826ee54e0`. With the same 16-row held-out dataset, base loss was 7.088871955871582; 16-step, rank-4, seed-41 Metal LoRA created adapter tree SHA-256 `d64ca54360acfc8d1e20094972b1d089ec94e9330388f16d036aff7c16010f29` and candidate loss 3.7540886402130127, with peak Metal allocation below 1 GiB. | The predeclared held-out and resource bounds passed, but the response gate failed: the canary and worse-candidate answers met their criteria, the HTTP answer did not explicitly require both observed generation and quality/outcome evidence, and the cancellation answer refused and repeated punctuation. This is a negative quality result and cannot be promoted or counted as useful-work acceptance. No admitted ticket or signed outcome was involved. |

`m28c-vmlx-compatibility` also remains **In Progress**. The conversion and
manual server switch prove the installed engine can transport the fused model
and distinguish selected model IDs. A better model or task-specific dataset,
predeclared quality/resource policy, admitted generation, process and mutation
binding, and automated rollback are still required by its build-plan checks.

The host-tool/Python/benchmark compatibility review finds the native MLX and
vMLX modules additive and unused by the current selected agent, gateway,
`coh` and SwarmUI. The standing-authority parser now recognises
`peft.release` as an optional third compiler-selected action, with a focused
scope/duplicate refusal test. The host agent now checks a selected release's
model, target and request digest, but its release executor has no standing
dispatch barrier or Mac MLX phase adapter. Neither selected manifest enables
the action, and the gateway still refuses its submission. No benchmark metric
or target interface changes. Integration must preserve M28b's durable phase
journal and signed result custody; a call to these modules alone cannot
satisfy that contract.

Material AI assistance contributed the implementation, tests and this record.
The ignored local artifacts are diagnostic and must be replaced by exact-source
live evidence after the admitted implementation is complete.
