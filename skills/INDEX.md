<!-- Author: Lukas Bower -->
<!-- Purpose: Help Cohesix users load portable skills and choose a useful 1.2.0 workflow. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Put Cohesix to work with your agent

Tell your agent what you want to achieve. These skills help it choose matching
tools, complete the task and bring back a result you can check. Start with
[get started](cohesix-get-started/SKILL.md) if you are evaluating Cohesix or
have not chosen a workflow. You can explore without a GPU or running target;
an Apple Silicon user can begin with a private local model comparison.

## Load the skills

Use a trusted checkout and retain the skill collection's source revision.
Load a skill's `SKILL.md` directly, or copy the `cohesix-*` folders into your
agent's documented skills directory. Copy complete folders, including
`references/`, and the companion folders used by the chosen workflow.
Copying the whole collection keeps sibling links available. Inspect an
existing destination before updating it; preserve local edits.

Agents implementing the [Agent Skills format](https://agentskills.io/specification)
can discover names and descriptions before loading instructions. Other agents
can read or receive the relevant `SKILL.md` and selected references as task
instructions. Actual discovery depends on the agent. Loading a Markdown file
does not configure an MCP/A2A transport, native runtime or credential.

The collection targets published **Cohesix 1.2.0** and can receive adoption
clarifications after release. Web links use `main` for discovery; replace it
with your installed tag/commit or use the matching bundled guide before
executing version-sensitive commands. Installed help, manifest and native
profile decide available behavior. [Quickstart](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/QUICKSTART.md) owns
verified installation and target setup.

On Mac, reuse either the selected extracted tarball or the native `.pkg`
installation. The setup skill discovers the tarball's absolute `bin/` or
`/Library/Application Support/Cohesix/bin`, checks its own release identity,
and carries that choice through the task. Loading skills requires no source
checkout on the operating host.

## Choose by the result you need

| Ask your agent | Skill | Useful result and prerequisites |
| --- | --- | --- |
| “Help me get a useful first result with the hardware I have.” | [Get started](cohesix-get-started/SKILL.md) | One completed journey selected from actual host, goal and installation. |
| “Install the matching tools and check this host.” | [AI host setup](cohesix-ai-host-setup/SKILL.md) | Verified Mac or Ubuntu ARM64 tools and selected runtime checks; QEMU and GPU runtimes are optional. |
| “Show resources and current work without changing anything.” | [Inspect](cohesix-inspect/SKILL.md) | A labelled model tour, or live state from an existing gateway and read scope. |
| “Compare this private model and adapter on my Mac.” | [MLX workbench](cohesix-mlx-workbench/SKILL.md) | Local inference and held-out comparison; Apple Silicon, pinned MLX environment and local inputs. |
| “Prepare these grayscale frames for a governed CUDA batch.” | [Workload authoring](cohesix-workload-authoring/SKILL.md) | Reviewed registration, pinned request and independent verifier; prepared Linux AArch64 NVIDIA executor and owner access for enrollment. |
| “Run this enrolled GPU job and verify the output.” | [GPU operations](cohesix-gpu-operations/SKILL.md) | Native result tied to the original job; fresh Worker, resource/control leases and submit scope. |
| “Let this agent request one permitted action.” | [Agent delegation](cohesix-agent-delegation/SKILL.md) | One MCP/A2A operation with budget and original-ID recovery; compatible configured client. |
| “Connect NeMo and compare direct and governed work.” | [NeMo workflows](cohesix-nemo-workflows/SKILL.md) | Pinned client checks, same-ID recovery and optional evaluation; selected Linux AArch64 kit and gateway. |
| “Diagnose this service and perform the authorized recovery.” | [Edge recovery](cohesix-edge-recovery/SKILL.md) | Before/after evidence for one native action; enrolled provider and action scope. |
| “Choose the next model and check which version users reach.” | [Model rollout](cohesix-model-rollout/SKILL.md) | Frozen comparison and canary/rollback record; advertised admitted native profile for deployment. |
| “Release this private adapter after checking its quality.” | [Private adapter release](cohesix-private-adapter-release/SKILL.md) | Held-out result and verified serving outcome; selected base/runtime and admitted release profile. |
| “Give me a trustworthy view across these hives.” | [Fleet operations](cohesix-fleet-operations/SKILL.md) | Per-hive freshness, pressure, work and uncertainty; each hive's own identity and read credentials. |
| “Explain what happened and what the evidence proves.” | [Evidence](cohesix-evidence/SKILL.md) | Offline case, capture comparison or signed verification; retained pack or scoped capture/trust inputs. |

## Follow a task through its outcome

- **No deployment yet:** get started → inspect a model or review an offline pack.
- **Local Mac task:** setup if needed → MLX workbench → local comparison record.
- **User CUDA batch:** setup if needed → workload authoring → GPU operations
  → independent output and signed evidence checks.
- **Agent-operated job:** client setup → agent delegation or NeMo workflows
  → GPU/private-release skill → evidence review.
- **Incident:** inspect → edge recovery → evidence; use fleet operations when
  observations span hives.

Each skill checks the actual executor as well as the controller, carries
existing authorization forward and identifies concrete missing prerequisites.
Recover an uncertain operation using its original identity.

Mac Local MLX is available as a local observation workflow. Its admitted release
lifecycle belongs to planned 28c1, and verified model distribution needs its
separately selected and qualified path. Those conditional stages are not
baseline 1.2.0 prerequisites. A listing, allocation or accepted request is
distinct from verified useful work.

Read [Researched projects](cohesix-get-started/references/use-cases.md) for researched
applications and feature coverage. [llms.txt](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/llms.txt) is the compact agent
entry point; [AGENTS.md](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/AGENTS.md) applies to repository development.
