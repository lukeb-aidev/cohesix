<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Retain local M28c MLX measurements and historical deployment proof limits. -->
<!-- Author: Lukas Bower -->

# Milestone 28c native MLX component record

**Scope update (25 September 2026):** The revised [M28c](../BUILD_PLAN.md#28c)
accepts actual local Metal training plus installed SwarmUI inference and
evaluation as developer work. Its [completion record](M28C_COMPLETION_RECORD.md)
binds that UI and the final app. The admitted Mac release, accepted generation,
governed vMLX canary and rollback remain [28c1](../BUILD_PLAN.md#28c1)
requirements. Earlier “In Progress” labels below describe the original
deployment scope and are not current M28c completion gates.

At the time of this component record, `m28c-mlx-metal-provider` was **In
Progress**. The native Python component
now rejects remote or changed model and dataset inputs, pins MLX 0.32.2 and
MLX-LM 0.31.3, requires the observed Metal GPU, caps unified memory, and
returns hashed prompt/output and resource observations. Training checks a
deadline and cancellation at every loss report; MLX-LM's final adapter config
is normalized to content and settings rather than scratch output paths.
The MLX code does not yet run under a selected Cohesix job or promote a
generation. Gateway release admission and a release-agent standing dispatch
fence now have focused source tests, but the selected native executor remains
Linux/CUDA; no Mac release has been admitted.

```text
Title/ID: m28c-mlx-metal-provider
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Run selected local MLX inference and LoRA through the shared admitted release lifecycle.
Inputs: accepted M28b lifecycle; M28c Apple feasibility; selected M4, MLX and local model/data.
Changes: tools/cohesix-py/cohesix/mlx_native.py + tools/cohesix-py/tests/test_mlx_native.py implement and check bounded native primitives. The gateway and agent now check selected release admission and dispatch against the exact accepted baseline, while the selected Mac native executor and provider graph still need integration.
Commands: PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_mlx_native.py; local content-bound MLX inference, train, held-out evaluate and repeatability probes in ignored out/m28c.
Checks: Four focused pure refusal tests pass. The selected Mac completed genuine four-step LoRA on Metal and two same-seed runs with the 16-row held-out dataset produced identical adapter bundle digests. The held-out comparison improved loss versus the base; no predeclared 28b policy or independent admitted comparison has been run.
Deliverables: Local MLX primitives and diagnostic compute measurements, not an accepted M28c provider or developer journey.
```

```text
Title/ID: m28c-swarmui-mlx-workbench
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Provide the installed app's terminal-free route to the selected MLX release and its signed evidence.
Inputs: 28b release report and exact selected job JSON; current SwarmUI session and installed coh tool.
Changes: SwarmUI's Local MLX desk routes plan/follow/verify/recover through coh peft release, reviewed start through coh job submit, and original-ID status/reconcile/cancel through coh job. The host parser now forwards parent command gateway connection arguments. A signed-journal projection shows Metal, held-out comparison, canary and rollback separately.
Commands: cargo test --locked -p swarmui --lib; cargo test --locked -p swarmui --test workbench; node --test apps/swarmui/tests/mlx_frontend.test.mjs; targeted WebKit Desktop Playwright Local MLX desk case.
Checks: 18 SwarmUI library tests, 11 workbench tests, two pure frontend projection tests and the one targeted browser form test pass. The browser fixture proves form wiring, not an admitted MLX release or installed app acceptance.
Deliverables: A source-level guided UI that preserves gateway admission and original job identity. Native Mac MLX execution, installed-byte validation and m28c-developer-live remain open.
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
| Instruction-formatted quality attempt | A second ignored policy was frozen at SHA-256 `5f9cf02413345d1c2a9f7c6fc89d12141f9223b3e8f7d17b90b729e23bdb64b3` before compute. Its 16/4/16-row local dataset tree is `6b2ef5031d278330c0e4db1a40f5de3ddcc2647df74e2fa7a1c2d0f3229670b7`. A 48-step, rank-8, seed-42 Metal LoRA created adapter tree `a9b837751e207b3c2c52d166f4fb341921cd8d5082197a82915d88f1ea498850`, with 1,182,363,920 bytes peak allocation. Baseline loss was 5.796783924102783 and candidate loss 0.8091069459915161 on 16 test rows. All four frozen operational questions met their explicit semantic criteria within 2,045 ms each and 1,006,469,604 bytes maximum peak allocation. Ignored detailed record `out/m28c/qwen-instruction-attempt.json` has SHA-256 `de93b5bcdb6d5f88d9ff2fd75236379b5d11d6b368619279f51ca1e742e1080b`. | The four answer templates are repeated verbatim across disjoint question phrasings in train, validation and test rows. The loss and responses show the selected narrow facts can be learned; they do not measure broad, independent developer usefulness. This remains an unadmitted local diagnostic: no shared phase journal, target ticket, signed result, accepted serving generation or vMLX canary/rollback. |
| Direct MLX loopback transport | `cohesix.mlx_service` bound one fixed model and the instruction-formatted adapter to `127.0.0.1:18086` under a diagnostic ID containing the full adapter SHA-256. A real bounded chat request observed Apple M4 Metal, 984,493,744 bytes peak allocation and output SHA-256 `d54caf7c83dd0f7f18a72bab8339bf62449e77e67ba63665021ca7d1b1da8f3b`, matching the direct MLX question-2 response from the frozen quality attempt. The process exited after the check. | This API has no Cohesix credential, admitted generation, durable supervisor or signed result. It does not replace the 28b load/canary/promote/rollback phases, and the local `g0` label is not accepted deployment generation zero. |
| Instruction-adapter vMLX compatibility | Before serving, ignored policy `out/m28c/qwen-instruction-vmlx-policy.md` was frozen at SHA-256 `ff1b3a525da6cd239ef8b69734fedbf3583c1b4649dd988d2e8aea9988a7c872`: four operational questions, exact model, no tools or stream, 64-token cap, each answer within 5,000 ms. MLX-LM fused the selected Qwen model and 48-step adapter into a separate source tree SHA-256 `b75efc50e56aec7c671357ee6eec7d4df55b8c82896a4308d941e0b86e629777`. Direct fused inference produced four useful responses; its first three output digests matched the unfused adapter and the fourth differed in wording while retaining the required decision. Installed signed vMLX 1.6.65 engine commit `22f9c77711fb580df32f4d40bbaea989c2d5421b`, executable SHA-256 `6ae7d9f0b5db2035b623fc0cacecc3f572bc46db18b69cc8fd611f71b0c2ac7d`, served a disposable copy as diagnostic `cohesix-g1-b75efc50e56aec7c`, process 83239. All four vMLX texts exactly matched direct fused inference and took 4,161/2,774/2,840/2,710 ms. The source model and adapter trees stayed unchanged; the loaded copy tree was `a2e3c00622388097a0e8ca3c2b8e600ba765ff44be7d56c88aca5bca43f7cec4` after local alignment repair. Ignored detailed report `out/m28c/qwen-vmlx-session.json` has SHA-256 `76f43119c53feb37f1ff9b9cc59591a5dcefd6b899dbfb0180e717fc5d0b1acb`. | This qualifies a real serving format and narrow response comparison, not a Cohesix admitted generation: `g1` is only a diagnostic label. The trained adapter was produced outside the shared phase journal, no signed result or governed canary exists, and no Cohesix rollback was run. The answer templates remain narrow. |

`m28c-vmlx-compatibility` is **Complete at the revised diagnostic scope**.
The later instruction adapter's fused copy passed a predeclared, narrow
four-question quality and latency policy on the installed engine, including
process and repaired-byte binding. An admitted generation, signed Cohesix
result, same-generation canary and governed incumbent rollback are separate
28c1 requirements. The earlier manual server switch does not supply them.

The host-tool/Python/benchmark compatibility review finds the native MLX and
vMLX modules additive. The standing-authority parser recognises
`peft.release` as an optional compiler-selected action. The gateway now
admits it only with an exact private request, helper and accepted baseline,
while the agent rechecks the incumbent and standing dispatch barrier before
native effects. Forward phases also recheck a cancellation request; rollback
remains permitted under valid recovery authority. The current selected
manifest does not enable that action,
and the agent's phase executor remains Linux/CUDA, so this is not Mac MLX
admission. The new SwarmUI desk uses the existing `coh` parser and gateway job
route; it does not add a transport or provider. Python and raw/REST benchmark
schemas and target interfaces are unchanged. Integration must preserve M28b's
durable phase journal and signed result custody; a call to these modules
alone cannot satisfy that contract.

A later M28c native-action source change added a gateway-derived start for an
already selected `systemd.restart` standing scope, with matching `coh`, Python
REST and App Intents clients. It supplies a useful governed start path without
changing target interfaces, benchmarks, the selected `peft.release` ceiling or
MLX admission. The Mac release executor, admitted generation and live
cross-surface evidence above remain outstanding.

Material AI assistance contributed the implementation, tests and this record.
The ignored local artifacts are diagnostic and must be replaced by exact-source
live evidence after the admitted implementation is complete.
