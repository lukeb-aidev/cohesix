<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents the nine-door crate responsibilities and scope. -->
<!-- Author: Lukas Bower -->
# NineDoor Crate

`nine-door` is the `std` host library and explicit fixture/model adapter for
the Secure9P state machine described in `docs/ARCHITECTURE.md`. It has no
target binary and is not staged in the QEMU or Pi runtime image. Its host
providers and deterministic fixtures can prove protocol and policy behavior,
but cannot prove a target child, live provider, external side effect, or use
case.

The selected target namespace service is `apps/nine-door-runtime`, a separate
`no_std` MCS child reached through the generated bounded service ABI. A future
Milestone 27b host-service use case must be explicitly authorized before this
crate gains any packaged host binary.
