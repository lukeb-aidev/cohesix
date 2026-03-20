<!-- Copyright © 2026 Lukas Bower -->
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
AWS AMI support is planned under Milestone 30 in `docs/BUILD_PLAN.md`. Generic
UEFI ESP tooling already exists in the repository; the remaining work is the
AWS-specific delta: AWS profile admission, ENA, outbound bootstrap, optional
IMDSv2, and AMI registration. This document will be updated only when that AWS
path is actually implemented.
