<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document AWS AMI bring-up status and constraints. -->
<!-- Author: Lukas Bower -->
# AWS AMI Bring-up (Status)

## Current status (as-built)
- Cohesix does **not** ship an AWS AMI build pipeline yet.
- There is **no** ENA driver, DHCP/TLS bootstrap, or IMDSv2 integration in the
  current root-task runtime.
- Any AWS-specific boot flow is therefore **not** part of the as-built system.

## Planned work
AWS AMI support is planned under Milestone 27 in `docs/BUILD_PLAN.md`. The plan
is authoritative for future implementation steps; this document will be updated
only when the AMI path is actually implemented.
