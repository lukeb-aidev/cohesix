<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh ticket policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh ticket policy (generated)
- `ticket.max_len`: `192`
- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through.
- `worker-*` tickets are required; role must match and subject identity is mandatory.

_Generated from cohsh-core ticket policy._
