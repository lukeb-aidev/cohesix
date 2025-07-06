

// CLASSIFICATION: COMMUNITY
// Filename: LICENSES_AND_REUSE.md
// Author: Lukas Bower
// Date Modified: 2025-07-02

# Cohesix License and Reuse Strategy

## Overview
This document defines the official license strategy and third-party reuse policy for Cohesix. It ensures compliance with all constraints in our `INSTRUCTION_BLOCK.md`, including atomic hydration, metadata validation, and SPDX tracking.

---

## Approved Licenses
Cohesix explicitly restricts reuse to the following license families:

- **Apache 2.0**  
- **MIT**
- **BSD (2-clause or 3-clause)**

All imported code, libraries, microkernel modules, userland tools, and orchestration frameworks must be validated to conform to these licenses.

---

## Explicit Prohibitions
- **No GPL** in any CUDA, GPU, driver, kernel, or telemetry pipeline. This includes CUDA build chains, device drivers, and Secure9P stack integrations.
- **No LGPL**, AGPL, or non-permissive copyleft licenses.

---

## Scope of License Tracking
| Component                      | License family |
|---------------------------------|----------------|
| Cohesix microkernel & UEFI boot | MIT             |
| Plan9 enhancements & 9P modules | BSD-2-Clause    |
| Secure9P + Rust TLS transport   | Apache 2.0      |
| Rapier physics integrations     | MIT             |
| CUDA device interfaces          | Apache 2.0 wrappers only, no GPL linkage |

---

## Notes on CUDA reuse
While NVIDIA’s CUDA userland binaries remain under NVIDIA’s proprietary terms, all Cohesix integrations use Apache 2.0 / MIT licensed wrappers and no GPL driver modules. The kernel space UEFI environment explicitly avoids Linux GPL taint.

---

## Compliance with instruction block
- Single-step hydration.
- Atomic write verified.
- SPDX compliance tracked in `METADATA.md`.
- Fully aligned with Cohesix pure UEFI + Plan9 strategy. No Linux fallback permitted.

---

# ✅ End of LICENSES_AND_REUSE.md