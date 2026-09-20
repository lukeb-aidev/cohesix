<!-- Author: Lukas Bower -->
<!-- Purpose: Bind the SwarmUI workbench to focused native, package, presentation and compatibility evidence. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# SwarmUI workbench implementation record

**Status: Complete — 20 September 2026.**

Title/ID: `m27f-connections-and-guided-operations` and the complete 27f task set.
Milestone: 27f — SwarmUI desktop workbench and community showcase.
Goal: Connect, operate, inspect and recover through the desktop without requiring
command grammar or a terminal, while preserving the existing authority owners.
Inputs: accepted 27b–27e host contracts, selected QEMU image, retained signed
CUDA/LoRA/recovery graphs and CAS, macOS ARM64 and Jetson Linux AArch64 desktops.
The owner requested full implementation, focused completion checks, Jetson testing,
real documentation images, and commit/push to main. The full workspace suite and
integrated release campaign are outside this component acceptance claim.

## Implementation and authority

- The connection panel validates Queen TCP and gateway base URLs, refuses
  placeholders, resolves private file/environment references and makes role,
  delegation, exclusive-console ownership, disconnect and reconnect explicit.
  Profiles store no credentials. REST attach now requires an actual authenticated
  status read; local transport bookkeeping cannot become a successful connection.
- The small shared `coh-cli` crate owns the CLI schema for both the executable and
  the desktop forms. The installed tool must agree with that schema. Native file
  pickers, search, reviewed fields and bounded results cover supported host
  operations. Arbitrary process execution, persistent mounts and identity enrollment
  retain their existing dedicated administration boundary and explain it in the UI.
- The Rust bridge invokes only installed `coh`, without a shell. A single-use review
  binds exact inputs and is invalidated by session/tool changes. One host operation
  owns the controller; a 120-second deadline and bounded output terminate/reap the
  local process and report uncertainty without claiming remote cancellation.
  Window close and application-menu quit preserve an active controller's lifetime.
- Namespace browsing and structured control forms reuse the existing grammar,
  role, ticket and generated profile gates. Direct diagnostics use canonical `bi`
  and the other admitted console verbs. REST explains its unsupported root-console
  diagnostic boundary. The expert console and byte-stable transcripts remain.
- Spectrum frames the desktop shell and forms; PixiJS retains Live Hive and draws
  the evidence ownership map. The frontend is split into session, navigation,
  operations, namespace, controls, artifacts, story and Hive modules. Polling stops
  for inactive/hidden/offline desks. Reduced motion preserves incoming observations.
- Run story, its evidence ribbon and GPU flight deck consume canonical verified
  graph/CAS projections. The new `coh evidence story` uses the existing shared
  signature verifier, enrollment and exact CAS digest/length checks. Sensitive
  content is withheld. Declaration, Worker receipt, external execution, target
  proof and present readiness remain distinct. Missing sensor facts say unavailable.
- Packaged CUDA, LoRA and failed-canary references retain their original signed
  identities. Materialization refuses altered files. Replays disconnect live access;
  failed canary plus successful rollback remains a recovered failure.
- Default manifest UI roots are expanded through IR and regeneration. A selected
  older production manifest retains its own narrower roots. Optional desktop
  package profiles add SwarmUI to the existing package system; headless profiles,
  protocol formats and immutable public releases are unchanged. The staging helper
  binds all 42 offline assets separately from exact signed native membership.

## Focused validation

Evidence lives under `out/m27f/`; original failed development attempts are retained.
No browser response injection is used in either native desktop walkthrough.

| Contract | Focused evidence |
| --- | --- |
| Shared parser, connection validation, review, offline gates and controls | `backend-final.log`: 18 library, one parity and eight workbench tests pass. |
| Cache, CLI help, log dump, reference tamper refusal, replay, security and Tauri policy | `swarmui-focused-package.log` cache result plus `swarmui-integration-final.log`; corrected help fixture passes. |
| Real local TCP refusal, trace, transcript and workbench | `swarmui-network-final.log`: all pass after granting the test its local listener permission. |
| Signed story, canonical pack/case/timeline consumers | `coh-evidence.log`: 17 tests pass. |
| Shared manual and trace | `cohsh-contracts.log`: five tests pass. |
| Owning CLI dependency boundaries | `dependency-check.log`: default REST and no-default builds pass. |
| Exact installed package membership and staging confinement | Two focused `test_host_package_stage.py` tests pass. |
| Native evidence and capture refusal contracts | Four `tools/swarmui-native` tests pass. |
| Source browser presentation/performance | `ui-source.log`: 81 pass, 12 deliberate project skips; `ui-final-changes.log`: 27 affected cases pass. |
| Candidate browser presentation/performance | `ui-candidate.log`: 81 pass, 12 deliberate project skips. The final presentation/navigation delta passes 12 further focused cases (`ui-candidate-final.log`). |
| macOS native source integration | `native-macos-source-02/result.json`: PASS, including real auth refusal, gateway/QEMU reads, installed host execution, reconnect, signed references, offline refusal and clean shutdown. |
| Production package | Signed build, verify, extraction, install and installed verification retain exact source, native bytes and selected production policies. Qualification signing uses an ephemeral key with independently supplied public trust; the private seed is removed. |

Final generated consistency (`generated-publish-final.log`), Test Plan integrity
(`test-plan-publish-final.log`), formatting (`fmt-publish-final.log`), focused
compilation (`compile-closure.log`) and default/minimal dependency policy
(`dependency-policy-final.log`) pass. Task-owned applications, QEMU, gateway,
tunnel and preview server are stopped; temporary test credentials are removed.

Commands are scoped to these owners, not the full workspace suite:

```sh
cargo test --offline -p swarmui
cargo test --offline -p swarmui --test tcp_console_warning --test trace --test transcript --test workbench
cargo test --offline -p coh --test desktop_story --test evidence_pack --test evidence_case --test evidence_timeline
cargo test --offline -p cohsh --test manual_cli --test trace
cargo clippy --offline -p swarmui -p coh-cli --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -m unittest discover -s tools/swarmui-native -v
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
```

Native launch/verify and capture command options are documented in
[SwarmUI](../SWARMUI.md). The source and candidate browser runs use the existing
three-project Playwright matrix and installed WebKit/Chromium, with `FIXTURE`
labels. They cover nonblank PixiJS rendering, backlog/cadence, polling suspension,
keyboard navigation, narrow layouts, proof distinctions and deterministic replay.

## Findings resolved during the native walkthrough

The first source lane exposed REST's local-only attach bookkeeping and was marked
FAIL. The actual authenticated status read now rejects a wrong token with HTTP 403.
Other repairs preserve canonical `bi` diagnostics, clear stale session identity,
make application-menu quit obey controller ownership, replace a stale connected
notice when entering replay, retain the mode label during scrolling and reset a
new workspace's scroll position. The native file picker selects the actual
installed policy file and the reviewed installed provider command succeeds.

A first package attempt correctly refused development authority. The final
candidate is generated and compiled against the accepted production profile;
no package-verifier rule was relaxed. Sandbox refusal to bind a local TCP test
listener was rerun with the specific permission. None of those failed attempts
is counted as acceptance.

## Native closure and final UI correction

The complete macOS source lane (`native-macos-source-02/result.json`) and installed
production candidate lane (`native-macos-final/result.json`) pass. The latter has
23 successful checks, including actual authentication refusal, authenticated
QEMU reads, host-tool execution, reconnect, signed LoRA/recovery replay, offline
refusal, process exit and exact artifact verification. Its original screenshots
supply the README hero, connection/operation/recovery gallery and deterministic
six-frame HTML capture. Frames are unaltered; this is a frame walkthrough, not
continuous video.

The Jetson native lane (`native-jetson-final/result.json`) passes its ten selected
Linux integration checks: actual gateway authentication, canonical shards and
boot reads, Live Hive, installed provider query, signed LoRA/recovery replay,
offline control refusal and application shutdown. Authentication refusal and
reconnect are not repeated on Linux; their native Mac evidence remains separate.
The host is Ubuntu 24.04 AArch64, kernel `6.8.12-1021-tegra`, GTK 3.24.41 and
WebKit2GTK 2.52.6. Remote keyboard punctuation required ordinary native clipboard
paste of public file references; no secret values or bridge state were injected.

The final frontend-only correction changes the session label to **ACTIVE SESSIONS**,
keeps read controls above listings, clears stale entries/notices and opens files
through a bounded read-only probe after a typed path-kind refusal. Auth/scope
refusals do not trigger a second command. Six source and six candidate browser
cases pass. Exact rebuilt Mac and Jetson native delta lanes independently pass
live file opening and shutdown (`native-macos-namespace/result.json` and
`native-jetson-namespace/result.json`). Mac additionally passes a direct Queen
connection, live boot read and reviewed canonical `bi` diagnostic. These delta
records supplement, and do not relabel, the earlier complete native records.
The namespace and Jetson gallery images come from these final builds.

| Artifact | SHA-256 |
| --- | --- |
| Complete Mac candidate app | `5dd3218652d293cde32cdfd095dffcf2021433124e351968b7872a8ade35c360` |
| Complete Jetson candidate app | `1ae8c17c97ff5389458dde3a0cc4425bdb5a950098f45c0f71e0bdad99ea7867` |
| Namespace-correction Jetson app | `890178ad720aad9e5378d9086707735def9ae231213c8ed5a2a9854f335ed073` |
| Namespace-correction Mac app | `0448993dae537475f2815f4af937fd8863dd33e9f7e40355729155ab4971cdcb` |
| Namespace-correction Mac source | `463d87c8e61e232e10d59cc13899102e4d7128e836ec30afad26022b1e8dbc6d` |
| Selected Mac registry | `442165e9d358d3509de886e0ed2ae656a2198d17ffb09cc68d0e6c04a6053798` |
| Live gateway | `5b305b3616693efac65ce2eac20061c03f4e82404472fbc25c5de652ebcb2df2` |
| Accepted QEMU CPIO | `85fbf2bcc98ca948e8c8d23a1c874f0baeccbeb6ecce89f6b60f6a7e8019eb32` |
| Observed target manifest | `20978a614c2cad6a37dc36aaea908592e37b3dc17fe60e9a9d361ce0b926bac7` |

The namespace-correction Mac source identities are recorded above. Every native lane
retains its own source inventory, app/tool/protocol identities and command log.
Production candidates use the accepted provisioned profile, including its own
narrower roots. They do not claim to be builds of the default development manifest.

**Retained platform observation:** Jetson's first closed app left a WebKit child
SIGSEGV report at 13:56:52 (child 200705, app 200664). The app had recorded an idle
shutdown and all selected operations had completed. The rebuilt app repeated live
connection/file reads and exited; the old crash report timestamp did not change
and no new report appeared. This one external WebKit teardown observation is
retained in `native-jetson-final/platform-note.json`; it is not erased or presented
as a repeated Linux shutdown guarantee. No OS/driver workaround was applied.

## Plain-English contextual help

Task `m27f-concise-contextual-help` adds one discreet **?** beside each screen title
and four short hints for presentation, action history, recent file activity and
snapshot naming. The copy explains what to do without assuming Cohesix terminology.
It uses existing Spectrum buttons and color tokens, plain text, keyboard focus,
hover, Escape dismissal and touch access. Essential instructions remain inline;
no new dependency or remote content is introduced. The interaction follows
[Adobe's tooltip guidance](https://opensource.adobe.com/spectrum-web-components/components/tooltip/).

Three focused browser-layout cases pass (`ui-help-english.log`), and all 12
candidate help/keyboard/showcase cases pass (`ui-help-candidate.log`). Exact Mac
and Jetson builds pass. Native help and dismissal are checked separately in
`native-macos-help` and `native-jetson-help`; the Mac also reverifies the signed
LoRA reference and exits with code zero. The new help screenshot and refreshed
README hero come from this final signed Mac package. Earlier connection,
operation, recovery and live namespace screenshots retain their original source
identities; the six-frame capture remains the complete earlier native gate.

| Final help artifact | SHA-256 |
| --- | --- |
| Mac app | `2e23a908c92e0b65a7112498696e3d6841776ba9f24aa778822238a201a26b93` |
| Mac source | `8216e21c93e3025d2a9965b5e54ebef9d9ac0f38d17e8e3ec035dc5569ca47db` |
| Jetson app | `f228f5bb11ae598d9a69815cbeb758afd976ff373fa8155ac788f7e3371e6243` |
| Jetson source | `12bbf7cb3db8a7c7c361b908bbe419a8f7649ba9b25ab31c8b00dac5cd4312d4` |

## Compatibility review

- `coh` and `cohsh`: the shared CLI extraction preserves existing flags, defaults,
  command parsing and transport semantics. The additive story/read-report/schema
  surfaces are host-only. Shared manuals, SwarmUI help and exact help fixtures are
  aligned. The REST authentication check is in the SwarmUI session adapter.
- Hive Gateway, host-ticket agent, GPU bridge, sidecar bridge and sidecar bus:
  existing admission/execution/receipt owners and wire contracts are unchanged.
  Desktop controllers invoke those owners through `coh`; no new provider executor,
  in-VM listener, GPU integration or service administration path is introduced.
- `cohesix-py`: generated roots/registry profiles are refreshed; existing lifecycle,
  evidence, transport and SDK APIs need no implementation change. The desktop
  does not create a separate Python workflow or classifier.
- Performance scripts and benchmark schemas: no backend workload, timing contract,
  threshold or report schema changes. UI cadence/backlog evidence is explicitly
  separate from REST throughput, physical Pi performance and staged acceptance.
- Packaging and guides: optional Mac/Linux desktop profiles retain canonical
  signed membership and production-policy verification. Public guides, README,
  native gallery and operator walkthrough describe the as-built UI; the existing
  1.0.0-beta archives and historical evidence remain immutable.

## Claim boundary

This closes a host UI component, not Milestone 27g, a public release cut, a new
CUDA/LoRA execution, or physical Pi acceptance. QEMU identity reads and historical
signed Worker evidence remain separate from fresh target pressure/repeatability
and GPU execution. Jetson is the tested Linux AArch64 desktop reference, not a
restriction of the host contract to one board. Full-workspace audit/advisory runs,
full staged target/release qualification, external-user studies and new human
review were not performed by this focused automated acceptance run.
