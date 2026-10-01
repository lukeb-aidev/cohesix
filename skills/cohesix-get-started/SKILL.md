---
name: cohesix-get-started
description: Choose and complete a first useful Cohesix task from the user's hardware, installation and goal. Use for getting started, evaluating fit or choosing a workflow; route ongoing jobs to their operating skill.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Turn a user's adoption goal into the smallest useful, verifiable Cohesix journey. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Get a useful first result

Start with what the user wants to accomplish and the machines they already
have. Inspect available OS, architecture, installed Cohesix version and tools,
and any existing gateway or executor. Reuse information already supplied;
ask only for a missing choice that changes the journey. A Mac controller can
use a remote CUDA host; a Linux controller does not need a GPU. On Mac,
discover either the selected extracted tarball's `bin/` or the `.pkg` tools in
`/Library/Application Support/Cohesix/bin` through the setup skill; retain the
chosen installation's resources and absolute paths.

Read the selected release's [Quickstart](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/QUICKSTART.md)
and [status](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/STATUS.md).
Use the matching installed guides or replace `main` with the selected commit
or tag. Newer adoption instructions can describe 1.2.0, but its bundled help,
manifest and operating contracts decide which commands actually work.

## Choose one journey

For a concrete project or feature-to-skill coverage, read
[researched use cases](references/use-cases.md). It maps primary-source
application patterns to a first useful deliverable and the companion skills.

| User's starting point | First useful result | Load next |
| --- | --- | --- |
| Curious, with no target or GPU | A labelled model namespace tour or an offline evidence case | `cohesix-inspect` or `cohesix-evidence` |
| An existing hive | Target identity, resources, current work and strongest blocker | `cohesix-inspect` |
| New host installation or optional QEMU target | Verified matching tools and the selected setup checks | `cohesix-ai-host-setup` |
| Apple Silicon Mac and a private model task | One local inference and a base/candidate comparison on Metal | `cohesix-mlx-workbench` |
| Prepared Linux AArch64 NVIDIA host and batch data | Reviewed workload package, frozen inputs and an independent verifier | `cohesix-workload-authoring`, then `cohesix-gpu-operations` |
| Agent that needs a permitted action | One discovered, scoped job with a retained original identity | `cohesix-agent-delegation`; for the pinned Linux kit, `cohesix-nemo-workflows` |
| A private adapter or model change | Held-out comparison, canary and verified serving outcome on a selected admitted profile | `cohesix-private-adapter-release` or `cohesix-model-rollout` |
| A failed service or several hives | Before/after recovery evidence or a fleet table with freshness | `cohesix-edge-recovery` or `cohesix-fleet-operations` |

Load the named companion's complete folder from the same skill collection.
If it is unavailable, use the corresponding same-version operating guide and
report that limitation. Skills are instructions; MCP/A2A transports, native
runtimes, target admission and credentials have their own setup.

## Complete the chosen task

Choose the smallest path that meets the goal. An offline case needs no target,
local MLX exploration needs no Queen, and controlling an existing Pi needs no
local QEMU. For a new governed deployment, follow Quickstart through target
startup and authenticated inspection before enrolling native work. Use the
setup skill for package verification and compatible runtime selection.

Keep the existing gateway as the sole target TCP owner. Let the user's request
and already supplied authority determine permitted actions; a read-only tour
does not grant service restarts or job submission. For a write, establish the
exact operation, subject, inputs, limits and current scope before using the
chosen skill. Recover a lost response under its original identity.

The initial deliverable should be usable: an inventory, incident case, measured
model comparison, independently checked batch result or scoped agent job.
Name the host and evidence source. Explain what the user gained, what remains
unavailable and the next specific step. A mock tour, local observation, accepted
request and verified native result must retain their actual proof class.
