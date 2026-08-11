<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents the gpu-bridge-host host tool scope and usage. -->
<!-- Author: Lukas Bower -->
# GPU Bridge Host Tools

Host-side bridge utilities live here as defined in `docs/ARCHITECTURE.md` and
`docs/GPU_NODES.md`. Live mode uses a compiled real NVML/CUDA discovery backend
and publishes only registry state read from an explicitly selected, validated
registry root. Missing registry state produces an empty model catalog with no
active model; malformed or stale active state fails closed. Live TCP publish
requires a non-placeholder token before any connection is attempted.

Published `gpu-bridge-snapshot/v2` records bind source mode and identity,
epoch/sequence, observation time, TTL, canonical catalog digest, model
manifest/CAS/base/adapter identities, and activation generation/receipt. A live
registry manifest requires `[model] id`, `cas_sha256`, and `format`; adapters
also require a base model present in the same catalog. Missing state never
selects the first model. Target root rejects fixture snapshots, stale
generations, incompatible catalogs, and invalid receipts, then withdraws an
accepted generation when its TTL expires.

`coh peft import` writes the strict adapter extension: the `[model]`
`cas_sha256` and `adapter_sha256` both identify the exact
`adapter.safetensors` bytes, `format` is `safetensors+lora`, and the manifest
also carries bounded `[provenance]` and `[hashes]` tables for the LoRA metadata,
optional metrics, policy, and telemetry inputs. The bridge rejects partial
extensions, unknown fields, invalid paths or digests, and any mismatch between
the model CAS/adapter identity and the recorded adapter hash.

`--mock` is an explicit deterministic fixture for local tests and `--list`.
Its fabricated GPUs and models never appear in live mode and cannot satisfy
target, integration, release, attestation, or use-case evidence. The root
namespace begins `state=unavailable source=none` and has no preseeded GPUs,
temperatures, leases, models, or implicit active model.
