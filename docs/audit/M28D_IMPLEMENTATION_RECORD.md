<!-- Author: Lukas Bower -->
<!-- Purpose: Retain selected M28d MCP live workflow evidence and exact proof boundaries. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M28d implementation record — selected MCP workflows

```text
Title/ID: m28d-mcp-policy-and-transport
Milestone: 28d / m28d-mcp-policy-and-transport
Goal: Expose only compiler-selected MCP operations through authenticated, bounded transports.
Inputs: Accepted M28 standing authority; selected qemu_smp_production manifest; MCP 2025-11-25; Hive Gateway REST identity and ledger.
Changes:
  - tools/coh-rtc and generated catalogue — selected tool schemas, authority, lifecycle, bounds and effective switches.
  - apps/hive-gateway/src/{mcp,jobs,main}.rs — Streamable HTTP and stdio protocol, delegated caller, scoped discovery and durable shared jobs.
  - apps/hive-gateway/tests and docs/{HOST_API,HOST_TOOLS,SECURITY,TEST_PLAN}.md — protocol, disabled mode, refusal and operator contracts.
Commands: Focused coh-rtc, Hive Gateway and generation checks below; live client and target case below.
Checks: Disabled combinations expose no MCP handler; selected calls enforce auth, Origin, revision, bounds and scope before native effects.
Deliverables: Selected generated MCP catalogue, packaged gateway paths and focused transport evidence.

Title/ID: m28d-mcp-selected-workflows
Milestone: 28d / m28d-mcp-selected-workflows
Goal: Complete and recover native CUDA and PEFT work through an ordinary MCP client under one shared authority.
Inputs: Accepted M28a Orin CUDA and M28b PEFT operations; completed M28c1 prerequisite; pinned Mac/Jetson QEMU; installed MCP SDK and NeMo clients.
Changes:
  - apps/hive-gateway/src/{mcp,jobs}.rs — selected preflight, submit, inspect, cancel, recover and evidence projection.
  - scripts/ci/provider_m28d_live.py, provider matrix and focused tests — exact-source live acceptance with original-result and refusal checks.
  - tools/cohesix-py/cohesix/model_release.py and test — verified external admission projects through the same CLI report without claiming controller submission.
  - apps/swarmui/src/lib.rs — correct the shared manual preface for host release commands.
  - docs/{BUILD_PLAN,STATUS,HOST_TOOLS,PYTHON_SUPPORT}.md — as-built completion and Python/operator guidance.
Commands: Selected live case and focused tests below.
Checks: Named MCP client obtains actual native CUDA output and deployed PEFT generation; lost reply and restart retain identity and no effect replay; cancel, exhausted budget and revocation refuse effects.
Deliverables: Retained private live evidence, installed-wheel parity, this audit record and Complete status.
```

## Source, selected profile and packages

The live Queen and gateway used source commit
`432be5a78892d37f58cb5fab0b10cb06c1ea3af4`. Its private selected
`qemu_smp_production` manifest SHA-256 was
`7d4be4c11fa4ba1a0475c8e5d3b843e8d134d42d52242c2266f3a033d75a3ec0`.
The pinned Mac QEMU at
`~/cohesix/qemu/qemu-10.1.0-hvf-gic-sync/install/bin/qemu-system-aarch64`
had SHA-256 `a0471828f464116c51c1d29ebae12a2a0fc713b4edec5c52e81bd5388040135a`
and booted the selected image to `root-console.start.ok`. The pinned Jetson
KVM QEMU at
`/mnt/nvme/cohesix-dev/qemu/qemu-10.1.0-kvm/qemu-10.1.0/build/qemu-system-aarch64`
had SHA-256 `bca19349a5a327625b708963dd5204f7b67e1b264216eb7743a66b8c8271e759`.
Its live PID `1270273` ran rootserver SHA-256
`7a72696fe0243bb63b62672139979a76ac1469a75ac74862601481e9720689a9`,
elfloader `cf8f6013e0a9e2861a3a74d31e190e9f9559c9e70585c2bbc1f06a032e4e2a86`
and CPIO `6e3b884108a95bb398186f77c7f57744a57787a09e5e000ed1a852b7072992cc`.
The rootserver's `[BUILD] 432be5a78892-dirty` marker denotes only the
private selected generated derivatives. The KVM runner rechecked the live PID,
binary SHA, image and authenticated `/proc/boot` manifest identity.

The selected gateway binary SHA-256 was
`82c999dc7075920d52d7d22c41d118f62c82c0642f2c3e83b425eb19a475c1a5`;
the host ticket agent was
`a87992f73d23c35511aca8cf67e51e2b6d4853d01c91144dffdff2dd875cd97c`.
The shared `coh` verifier was
`f66e239dd36f4a9d18937e49b3bba375608e16f63dd6ced5c2714190b09f56b6`.
The named ordinary client used MCP Python SDK 1.29.1 over Streamable HTTP;
NeMo Agent Toolkit 1.9.0 separately discovered the generated recovery schema
and recovered the original CUDA terminal through its native
`MCPStreamableHTTPClient`. Delegated ticket and request credentials remained
in private files; no credentials are in this record.

## Live workflow and recovery

The client discovered only selected tools and preflighted native work under
standing authority. Missing authentication returned HTTP 401, bad Origin 403,
and unsupported revision 400. A different subject saw only two read tools;
invalid caller receipt preflight returned an MCP error. Focused endpoint tests
covered all eight master/MCP/A2A enablement combinations, malformed and
oversize input, concurrent request bounds, stdio framing and existing-job
preservation when MCP is disabled.

CUDA request `m28d-reference-systemd-07` used the real Jetson CUDA provider.
Its submit response was deliberately discarded after transmission. Reconnect
and gateway/agent restart recovered the same admission, target ticket and
idempotency key, confirmed/acknowledged terminal and result SHA-256
`8a227c3ac70cae2ddbd0417c7edc07e5830557c47ca9e7f9353d486567a9e598`.
Recovery set `effect_replay_allowed=false`. An independent native output
verifier passed SHA-256
`e4cd347ccc7288ca0670b7356d2e2cabbcd7dc280e4909d60062773c2e0938ea`.

PEFT operation `m28d-mcp-peft-05` trained a 32-step adapter on the Jetson,
then validated, evaluated, scanned, staged, loaded, canaried and promoted it.
The 16-row held-out baseline and candidate losses were both
`4.928388595581055`, meeting the selected zero-regression policy. The
candidate adapter SHA-256
`d13c512ef238fecddea5c7bbb5e4d6731b610e1374cbd1c0a36bf7e6671a976d`
became accepted serving generation 7. The shared `coh peft release verify`
accepted the requested outcome under signed graph
`e3468c93fd618e7b58e74eeb465e8f335913646118c1421a6bc95e94d4c504a2`.
The native comparison, accepted-state file and signed phases agree. MCP
recovery before and after gateway restart returned the original operation and
result SHA-256
`11151c8b8de61165ccbfed57a0530a9deefca836087e9baf9db33cbd824eadb1`
with effect replay forbidden. A source-matched installed `cohesix 1.1.0b1`
wheel projected the same CUDA admission/result and PEFT operation/verified
graph; its controller journal correctly remained `submitted=false` because
MCP, not that controller, submitted the release.

Pre-dispatch cancellation of `m28d-reference-systemd-08` retained
`cancel_requested`, `refused_no_effect`, no dispatch time and no result digest.
A one-unit standing budget admitted and completed
`m28d-reference-systemd-09`; the next preflight was refused without job 10.
After a successful preflight for the revocation scope, REST submit refused
`EPERM standing-scope-revoked`, MCP hid the tool, and no job 11 appeared.
The runner rechecks these retained ledger records and live Queen state.

The private `m28d-mcp-live` summary is at
`/mnt/nvme/cohesix-dev/m28d-live2-20260925/source/out/private/m28d-session/m28d-mcp-live-final4/summary.json`;
its SHA-256 is
`7ed878de5fbe3efcecc02232f8d9240be459624fe0094e895b3f9671263fe68a`.
It reports `PASS` for the selected `jetson-orin-nano-jp7` component case.
Earlier copied-profile PEFT failures and a raw disconnect that never reached
the ledger remained diagnostic and were not counted as success. The retained
accepted PEFT root was used for the successful native comparison/deployment.

## Focused validation and limits

The selected live command was `scripts/ci/provider_conformance_run.sh
--matrix configs/provider_conformance.toml --case m28d-mcp-live
--reference-config "$PWD/out/private/m28d-session/m28d-live-reference.json"
--host-profile jetson-orin-nano-jp7
--state-dir "$PWD/out/private/m28d-session/m28d-mcp-live-final4"` on Merlin2.
Focused `coh-rtc` protocol-control, Hive Gateway protocol/control/workflow,
`host-ticket-agent --lib`, provider-matrix/runner negative, Python release,
`cohsh` manual and SwarmUI console-manual checks passed. The actual
`cohsh --man spawn` and `--man ls` output was inspected: production spawn is
explicitly a disabled compatibility command, GPU lease and Worker budget are
distinct, and `ls` describes role-scoped namespace discovery. SwarmUI uses
the same manual body in both console backends and now directs host releases
to `coh`. Generated consistency and formatting checks passed.

The selected catalogue advertises CUDA and PEFT operations, not a mixed
Mac MLX/Jetson CUDA recipe, verified weight-transfer operation or vMLX MCP
client compatibility. M28c1 completed beforehand but does not activate those
conditional paths. The Pi profile and A2A are disabled. This is component
acceptance on pinned QEMU plus a native Jetson host, not physical Pi evidence,
integrated Release B acceptance or a complete NeMo agent workflow. The full
test suite and release campaign were not run.
