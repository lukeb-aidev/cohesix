<!-- Author: Lukas Bower -->
# Root Task Crate

The `root-task` crate embodies the responsibilities described in
`docs/ARCHITECTURE.md` §1-§3:

- Own the initial capabilities and boot flow transferred from seL4.
- Configure timers and the scheduling surface that powers worker budget enforcement.
- Bootstrap the NineDoor 9P server and provision worker tickets for the queen orchestration loop.

This milestone now ships a kernel entry path that accepts the seL4
`BootInfo` pointer, prints early diagnostics to QEMU via
`seL4_DebugPutChar`, and then blocks. The log banner highlights:

- Confirmation that control reached userland (`entered from seL4`).
- The address, size, and span of the BootInfo extra region—useful when
  cross-checking faults such as the data abort at
  `FAR=0xffffffffffe02000` reported in QEMU logs.
- Basic topology hints (`node_id`, `num_nodes`, and the IPC buffer).

Subsequent milestones will replace the placeholder spin-loop with real
seL4 bindings and capability management logic.
