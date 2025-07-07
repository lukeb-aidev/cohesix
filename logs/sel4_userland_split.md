// CLASSIFICATION: COMMUNITY
// Filename: sel4_userland_split.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-09-30

# seL4 Rootserver vs Userland Split

## Rootserver Crates (no_std)
- `cohesix_root`
- `cohesix` binaries: `sel4_entry`, `kernel`

## Userland Binaries (std)
- All CLI tools from `cohesix`
- `cohesix-9p`
- `cohesix-secure9p`
- Go services: physics-server, indexserver, srvctl, devwatcher, gui-orchestrator

## Notes
- Rootserver compiled with `no_std` and minimal alloc handlers
- Userland targets musl and includes full networking and logging stack

