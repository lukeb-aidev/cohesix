<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Help Cohesix 1.2.0 readers locate current capabilities, execution boundaries and source-bound evidence. -->
<!-- Author: Lukas Bower -->

# Cohesix 1.2.0 status

Use this page to find the right workflow and understand what its evidence means.
The installed bundle's `VERSION.txt`, `RELEASE_NOTES.md` and manifest identify
its exact contents. The [Build Plan](BUILD_PLAN.md) owns task scope and the
[Test Plan](TEST_PLAN.md) owns acceptance checks; neither a source declaration
nor a component result transfers proof to a different image or host.

## Where work runs

| Place | Cohesix role | Start here |
| --- | --- | --- |
| QEMU or Raspberry Pi 4 target | The seL4 root task supplies Queen authority; passive Heartbeat, GPU and LoRA Workers have bounded target roles. Selected child runtimes own the console, namespace or physical devices. | [Architecture](ARCHITECTURE.md), [roles](ROLES_AND_SCHEDULING.md) |
| Mac or Linux controller | `cohsh`, `coh`, Hive Gateway, SwarmUI, Python and agent clients use the target's documented control surfaces. One process owns the target TCP console at a time. | [Quickstart](QUICKSTART.md), [host tools](HOST_TOOLS.md) |
| Linux AArch64 NVIDIA executor | CUDA/NVML, model weights, native jobs and their output checks stay on the GPU host. A target Worker records control and receipts; it does not execute CUDA. | [GPU nodes](GPU_NODES.md), [private LoRA release](PRIVATE_LORA_RELEASE.md) |
| Apple Silicon Mac | Local MLX and vMLX execution stay on macOS. The selected Mac workflow can submit governed work through a separate Cohesix target. | [Mac providers](MACOS_PROVIDERS.md), [SwarmUI](SWARMUI.md) |

The QEMU and Pi manifests each declare 256 passive Worker instances across
Heartbeat, GPU and LoRA roles. Both selected production profiles enable
authenticated MCP and A2A routes at the host gateway.
Check the installed manifest and profile before using an agent client. Host
protocol replies and target admission are distinct from independently verified
native provider outcomes. See [Host API](HOST_API.md) and the
[Glossary](GLOSSARY.md) for those terms.

## Release B qualification

[Milestone 28g](BUILD_PLAN.md#28g) is **In Progress** for the 1.2.0
candidate. The selected source now records version-aligned Python packaging,
native Mac and Ubuntu ARM64 installer builders, and installed SwarmUI resource
lookup. At clean source `522463fa1ade`, the canonical QEMU and physical Pi
Test Plans passed Stages 01–05, including exact-source authenticated TCP, REST
and fresh due diligence under the owner's renewed `EX-2026-0024` exception.
Earlier failed Stage 05 attempts retain their results. Medium and high QEMU
pressure passed on Mac HVF with correlated Worker receipts. The selected Jetson
KVM pressure replay reached authenticated target and fault checks, then failed
when host integration selected a Mac profile on Linux; a corrected replay is
pending. Signed native installers,
clean platform installs, graphical launch, installed cross-client parity and
integrated Release B qualification remain outstanding. The
[M28g implementation record](audit/M28G_IMPLEMENTATION_RECORD.md)
retains the exact identities, diagnostic failures and blocked gates. Release A
remains the published release.

At later source `4a46989432ae`, the exact production Pi image booted by
TFTP/RAM with verified image hash, post-reset CRCs and build marker. QEMU and
Pi Stages 01–02 passed. The production Pi Stage 03 attempt then failed because
its development-authority fixture tried legacy `/queen/ctl`, which production
correctly denied. The release factory now has a separate strict-production
QEMU TCP proof path, but its native target runs and the complete development
and physical Pi gates remain pending. No SD rebuild is requested before the
production Pi release lane passes.

The Release B Python SDK source and manual now use `1.2.0`. Its PyPI
workflow is prepared to build from the reviewed `v1.2.0` tag and compare
the exact public distributions after reviewer-gated publishing. The tag and
public files do not yet exist. Optional QEMU setup now probes a real four-core
startup; Mac can build the verified pinned 10.1.0 HVF fix when its installed
QEMU fails, while the selected JetPack 7.2.1 / L4T 39.2.1 ARM64 reference
uses `qemu-system-arm` from apt.
The pinned Mac source build passed its entitlement, startup and setup checks.
A dedicated Debian publisher key also passed Mac and Jetson detached-signature
probes. These checks do not supply a signed installer, packaged guest or
release acceptance. The owner's change from the earlier beta plan to stable
`1.2.0` requires fresh exact-source release qualification.

The selected QEMU and Pi production manifests now enable host-side MCP and A2A
under the compiler-controlled master switch. The latest 60-minute Pi 4 GENET
RAM-image diagnostic completed four MCP and six A2A original jobs with
target-confirmed, independently checked Jetson CUDA outputs and 122 healthy
samples. Its collector was amended during the run, and it did not exercise
installed Mac/Linux packages or the full release matrix. Earlier timed
read-only and failed job attempts remain separate evidence. Direct Jetson CUDA
and NeMo and Mac MLX checks passed at their stated host-only scope.

Nine operator skills passed syntax, link and copied-location guidance checks
on Mac and Jetson. Read-only and mock host operations ran from outside a source
directory, and the guides now require verified client transport, two private
protocol headers, one gateway owner and separate user credentials. The agent
delegation and GPU guides now cover cumulative standing attempts, finite
caller operations and publisher-epoch Worker replacement. Diverse-vendor live
jobs, cross-user isolation and useful installed Release B workflows are still
unproven.

[Milestone 28](BUILD_PLAN.md#28) is complete at its selected foundation scope.
The source declares
false-default, compiler-controlled MCP and A2A access switches and implements
selected REST/CLI/Python jobs with a private standing ledger for GPU submit and
service restart. Exact-source KVM target and native Linux AArch64 CUDA/service
observations passed the selected jobs and standing authority cases, including
stale request refusal and recovery of pending result delivery. At that
foundation commit the gateway had no MCP or A2A routes. The [M28 implementation record](audit/M28_IMPLEMENTATION_RECORD.md)
retains the evidence and limits. This component result does not change Release A
acceptance or qualify Release B.

[Milestone 28d](BUILD_PLAN.md#28d) is complete for the selected MCP-only
Jetson Orin scope. A named MCP SDK client reached an authenticated gateway
and exact-source KVM Queen, completed native CUDA and PEFT work, and recovered
the original results after a lost response and gateway restart. The shared
verifier confirmed the deployed PEFT generation; an installed Python wheel
projected the same outcomes. Focused transport and manual checks covered
disabled, scoped, authentication and refusal behavior. Cancellation, exhausted
budget and revocation caused no new effect. The
[M28d implementation record](audit/M28D_IMPLEMENTATION_RECORD.md) retains
identities and proof limits. This selection did not advertise mixed MLX/CUDA,
weight distribution or vMLX MCP client compatibility; Pi and A2A stayed
disabled. It does not qualify a physical Pi or Release B.

[Milestone 28e](BUILD_PLAN.md#28e) is **Complete** for the selected Linux
CUDA/PEFT A2A scope. The compiler-controlled A2A 0.3.0 gateway projected real
Jetson CUDA and PEFT jobs through an exact-source KVM Queen. The standard A2A
SDK recovered both original tasks after gateway restart; independent CUDA
output verification and the shared signed PEFT verifier accepted their native
results. NeMo Agent Toolkit's native client resolved the scoped card, looked
up the CUDA task and requested cancellation without changing its completed
outcome. Focused tests and live refusals covered disabled access, other
subjects, exhausted budget and revocation. The [M28e implementation
record](audit/M28E_IMPLEMENTATION_RECORD.md) retains source, image, package
and evidence identities. Pi A2A remains disabled; mixed MLX/CUDA, weight
distribution and vMLX composition were not selected. This component result
does not qualify a physical Pi or Release B.

[Milestone 28f](BUILD_PLAN.md#28f) is **Complete** for the selected Linux
AArch64 NeMo Agent Toolkit 1.9.0 component. A digest-pinned kit wheel and
version-pinned dependency lock installed into a fresh Jetson environment.
Toolkit's native MCP client submitted and recovered an independently verified
CUDA job, while its native A2A client delegated a real HF PEFT release whose
signed verifier accepted serving generation 9. A changed evaluation policy
refused a second PEFT candidate before training or promotion. Separate
model-backed NeMo agents connected to the authenticated MCP and A2A paths;
native Toolkit evaluation and profiling compared the same completed task with
and without governed lookup. The [M28f implementation
record](audit/M28F_IMPLEMENTATION_RECORD.md) retains the exact package,
target, model and evidence identities, focused checks, adverse results and
limits. One budgeted A2A CUDA attempt remains reserved with no confirmed
target result; a cross-protocol retry was refused and no success or released
allocation is claimed. This component result does not qualify a physical Pi
or Release B.

[Milestone 28a](BUILD_PLAN.md#28a) is complete at its selected Orin scope. The source contains a
private digest-pinned workload registration path, version 2 request validation,
native GPU diagnostics and an independently checked batch-edge example. An
exact-source KVM Queen on Merlin2 admitted reference and independently
configured adaptation jobs through both systemd and Docker GPU owners. Signed
original terminals and independently verified output bytes bind each case to
the selected Orin. Cancellation, lost-response, bridge-restart and native-timeout
checks settled their reservations without replay. The [M28a implementation
record](audit/M28A_IMPLEMENTATION_RECORD.md) retains the focused evidence and
limits; this component result does not qualify Release B.

[Milestone 28b](BUILD_PLAN.md#28b) is complete for the selected private model
reference. Cohesix can train or import a small model adapter, measure it against
the running version, and reversibly promote it with a verified application
request. A pinned Linux AArch64 NVIDIA host completed genuine LoRA training,
independent compatible import, and full Trainer checkpoint resume through an
exact-source KVM Queen and WorkerLora receipt path. Held out comparison rejected
a worse adapter before load. An interrupted promotion restored its incumbent,
which another application request observed. The [M28b implementation
record](audit/M28B_IMPLEMENTATION_RECORD.md) keeps the distinct source, profile,
negative-result and network-observation limits. This is component evidence, not
Pi 4 or integrated Release B qualification.

[Milestone 28c](BUILD_PLAN.md#28c) is **Complete for the narrowed Mac developer workflow** on 25 September 2026. The [completion record](audit/M28C_COMPLETION_RECORD.md) binds the supported macOS 27 Apple M4, focused tests, selected private KVM service work, pinned model/data/adapter, installed vMLX 1.6.65 and the final Developer ID app. Spoken Siri was removed from the developer value gate; macOS user-created Shortcuts supply the useful native action path. The subsequent governed release and serving work is [Complete in 28c1](BUILD_PLAN.md#28c1) under separate admitted evidence.

[Milestone 28c1](BUILD_PLAN.md#28c1) is **Complete for the selected Mac component** on 25 September 2026. An exact-source pinned QEMU Queen admitted real Apple M4 Metal training and an imported release interruption under original ticket identities. The shared verifier reported signed `succeeded` and `recovered_failure` outcomes; the latter restored accepted generation 1. The pinned signed vMLX 1.6.65 engine served four frozen responses from the content-bound fused model, refused a changed generation, and observed the verified incumbent. The [implementation record](audit/M28C1_IMPLEMENTATION_RECORD.md) retains source, image, profile, native and graph identities, focused checks, and the physical Pi/Release B proof limits.

The installed development-signed App Intents path enrolled a delegated Keychain connection and used user-created Shortcuts to start and inspect an approved private KVM service job. `coh` resolved its original admission and confirmed result. A later held cancellation settled `refused_no_effect` before dispatch, and revocation removed the native scope choice. Foundation Models explained the scoped job and proposed a typed inspect follow-up without submitting it. The [actions record](audit/M28C_APPLE_ACTIONS_RECORD.md) retains exact identities, the initial cancellation defect and correction. These are selected component observations, not Release B qualification.

The final SwarmUI release binary and App Intents extension were Developer ID signed under Team `KB88FQXUX2`; Apple accepted notarisation submission `b14be979-3330-4770-94f1-73fce245bf6e`. The stapled app installed at `~/Applications/SwarmUI-M28c-Build2.app` passed strict signature verification and Gatekeeper assessment, and its two signed executable hashes matched the notarised stage. `pluginkit` registered only that final extension. A saved read-only status Shortcut received HTTP 403 after the earlier scope revocation; this is a live refusal, not a fresh authorised job on final bytes.

The final installed **Local MLX** panel ran bounded inference and 16-row held-out evaluation using the pinned Qwen2.5-1.5B instruction model and 48-step LoRA adapter on the observed Apple M4 Metal device. The panel displayed the model/adapter hashes, about 974 MB/1.14 GB peak allocation, a useful bounded answer and held-out loss `0.8091070055961609`, labelled **local observation, no Cohesix admission or promotion**. The Python native component's earlier real 48-step LoRA training and four frozen operational answers remain a narrow diagnostic quality result; answer templates repeat across the train/test split. The installed signed vMLX 1.6.65 engine served a disposable fused copy and reproduced four direct fused answers inside the frozen latency bound; its `g1` label is diagnostic, with source and repaired loaded hashes retained separately in the [MLX record](audit/M28C_MLX_COMPONENT_RECORD.md).

Focused Python MLX (6), vMLX (12), frontend (3), SwarmUI workbench (12), native compile, Rust formatting and generated-consistency checks passed; the installed Metal inference/evaluation and final Apple signing path were exercised. The full suite was not run. M28c adds no Pi hardware, mixed MLX/CUDA, accepted Mac deployment, broad model-quality or integrated Release B claim.

## Choose a workflow

| Need | Guide | Evidence to retain |
| --- | --- | --- |
| Boot or connect to a target | [Quickstart](QUICKSTART.md), [hardware bring-up](HARDWARE_BRINGUP.md) | Exact image/profile, boot, transport and authenticated response; QEMU results do not establish Pi hardware behavior. |
| Inspect or operate a hive | [Host tools](HOST_TOOLS.md), [operator walkthrough](OPERATOR_WALKTHROUGH.md) | Original request identity, bounded response and relevant target or host observation. |
| Run a CUDA job or release an adapter | [GPU nodes](GPU_NODES.md), [private LoRA release](PRIVATE_LORA_RELEASE.md) | Native output or signed release verification as well as the admitted job and Worker receipt. Reconcile an uncertain reply under its original identity. |
| Use MCP, A2A or NeMo | [Host API](HOST_API.md), [use cases](USE_CASES.md) | Selected protocol/profile, delegated subject, original task or ticket, and native outcome verification. |
| Use the Mac MLX workflow | [Mac providers](MACOS_PROVIDERS.md), [SwarmUI](SWARMUI.md) | Local Metal/model identity and, for governed work, the separate target admission and signed native result. |

## Read evidence at its original scope

Selected component records cover the [host job foundation](audit/M28_IMPLEMENTATION_RECORD.md),
[CUDA workload](audit/M28A_IMPLEMENTATION_RECORD.md),
[private adapter](audit/M28B_IMPLEMENTATION_RECORD.md),
[Mac MLX and governed serving](audit/M28C1_IMPLEMENTATION_RECORD.md),
[MCP](audit/M28D_IMPLEMENTATION_RECORD.md),
[A2A](audit/M28E_IMPLEMENTATION_RECORD.md) and
[NeMo](audit/M28F_IMPLEMENTATION_RECORD.md). The
[Mac app and Shortcuts record](audit/M28C_APPLE_ACTIONS_RECORD.md) covers its
separate installed client path. Each record identifies the exercised source,
profile, host or target and limits; use the release's own evidence to judge an
assembled 1.2.0 distribution.

The earlier [1.1.0-beta release notes](../releases/RELEASE_NOTES-1.1.0-beta.md)
and [qualification record](audit/M27G_IMPLEMENTATION_RECORD.md) retain their
original Pi, host and package evidence, including disclosed limits. They remain
useful for history and comparison, but do not qualify a new 1.2.0 image.

To verify a specific claim, match its source and selected manifest to the
installed bundle, then use [Hardware Bring-up](HARDWARE_BRINGUP.md) for physical
proof, the [Test Plan](TEST_PLAN.md) for acceptance, and
[Benchmarks](BENCHMARKS.md) for measured performance. The
[exceptions register](audit/EXCEPTIONS.md) retains accepted historical gaps.
