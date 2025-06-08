// CLASSIFICATION: PRIVATE
// Filename: QUEEN_POLICY.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Queen Policy

This document defines internal enforcement policies for the Queen role.

Federated queens may delegate sub-roles to peers. The policy engine resolves conflicts by preferring the latest timestamped policy file within `/srv/<peer>/policy_override.json`. Administrators can supply explicit rules to override time-based resolution.
