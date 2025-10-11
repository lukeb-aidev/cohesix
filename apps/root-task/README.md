<!-- Author: Lukas Bower -->
# Root Task Crate

The `root-task` crate embodies the responsibilities described in `docs/ARCHITECTURE.md` §1-§3:

- Own the initial capabilities and boot flow transferred from seL4.
- Configure timers and the scheduling surface that powers worker budget enforcement.
- Bootstrap the NineDoor 9P server and provision worker tickets for the queen orchestration loop.

This milestone provides a stub binary that records these responsibilities in code and enables
workspace compilation. Subsequent milestones will replace the stub with real seL4 bindings and
capability management logic.
