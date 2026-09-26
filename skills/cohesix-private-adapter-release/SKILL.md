---
name: cohesix-private-adapter-release
description: Train or import a private PEFT or MLX adapter, compare it with the incumbent, and verify the served generation after promotion or rollback. Use for one adapter lifecycle, not for fleet-wide model selection or raw Hugging Face setup.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Guide a private adapter from pinned inputs to independently verified serving and recovery. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Release one private adapter

Use the exact [Private LoRA release](../../docs/PRIVATE_LORA_RELEASE.md) guide
and matching installed `coh peft release --help`. Confirm the selected host
runtime and WorkerLora receipt path. Training and serving stay on the native
Mac or NVIDIA host; Cohesix owns admission, phase identity and verification.
If the task is to install CUDA, MLX, PEFT or NeMo, use
[AI host setup](../cohesix-ai-host-setup/SKILL.md) first.

Select the native path from the **executor host**, not the controller. On
Apple Silicon macOS, use the explicitly configured `cohesix-mlx-native/v1`
profile and `launchd`-owned `cohesix.mlx_release` helper; require observed
Metal work and its direct serving canary. On Linux AArch64 NVIDIA, use the
`cohesix-hf-native/v1` profile and selected `systemd` user service; require
real CUDA work and the served generation. A Mac or Linux controller may
operate the remote admitted path. Do not substitute a profile across hosts
or treat a CPU fallback as native release proof.
Inspect the installed helper, model/adapter format, accelerator and service
identity on that host before applying a plan. If they do not match the native
profile, explain the blocker and offer the matching
[host setup](../cohesix-ai-host-setup/SKILL.md) or a supported profile; keep
the candidate unapplied until the incompatibility is resolved.

## Freeze the comparison before admission

Record the licensed base revision and digest, adapter or training input digest,
separate held-out data, exact runtime, quality metric, canary prompts,
incumbent generation and limits. Training, import and checkpoint resume are
different origins; an imported adapter has unknown training provenance unless
independently supplied. Check adapter/base compatibility and scan results
before load. Do not change the held-out task or threshold after seeing a poor
candidate.

Prepare the native request using the selected release profile. Inspect its
plan and delegated scope, then apply only with the user's existing
authorisation. Follow one original operation through validation, optional
training or import, evaluation, scan, stage, load, canary and promotion. A
failed comparison stops before load. A checkpoint resume is a new authorised
operation only after the interrupted original effect has been reconciled.

## Check what serves users

Use `coh peft release verify` with the independently enrolled trust and
retained graph/CAS evidence. Match subject, operation, base/adapter/data,
phase chain and native generation. Send the separate application request
described in the guide to the exact promoted generation. If promotion fails,
record `recovered_failure` as a failed candidate and observe the incumbent
serving after rollback. A registry pointer, HTTP success or signed terminal
for a *different* effect is not a successful release.

If a reply is lost, inspect or recover the same operation and its native
phase journal. Never generate a new ID to repeat an uncertain load or
promotion. Keep model bytes, dataset, credentials and private keys on their
authorised host; export only redacted, bounded evidence. Return a release
decision containing the pinned inputs, quality comparison, original ID,
canary and client observation, verifier result, active generation and any
remaining uncertainty. Use the [model rollout skill](../cohesix-model-rollout/SKILL.md)
when choosing or distributing among hosts.
