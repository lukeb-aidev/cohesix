<!-- Author: Lukas Bower -->
# NineDoor Crate

`nine-door` implements the Secure9P server responsibilities from `docs/ARCHITECTURE.md` §2-§3.
It will own the session tables, access policy integration, and namespace providers
responsible for queen control, telemetry, and worker directories. The initial skeleton
records those expectations and exposes stable APIs for downstream crates while the
full protocol machinery is developed in later milestones.
