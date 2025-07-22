// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.2
// Author: Lukas Bower
// Date Modified: 2028-11-28

This crate provides a thin wrapper around seL4 headers for use with `bindgen`.

## Requirements

Set the following environment variables before building:

- `SEL4_INCLUDE` — path to the seL4 include directory
- `SEL4_LIB_DIR` — directory containing `libsel4.a` and related static libraries
- `SEL4_ARCH` — target architecture (e.g. `aarch64`)

`build.rs` expects these variables and will fail if they are missing.

The crate is `no_std` and links statically against `libsel4` and any
architecture-specific libraries found in `SEL4_LIB_DIR`.

The `include` directory must contain the following seL4 headers (copied from
`third_party/seL4/include` or stubbed if unavailable):

```
autoconf.h
libsel4_autoconf.h
invocation.h
sel4/macros.h
sel4/syscalls.h
sel4/syscalls_master.h
sel4/simple_types.h
sel4/shared_types.h
sel4/bootinfo_types.h
sel4/errors.h
sel4/faults.h
sel4/objecttype.h
sel4/config.h
sel4/arch/vspace.h
```

`build.rs` verifies these headers at compile time and will fail with a clear
message if any are missing.

Compile using nightly Rust with:

```bash
cargo +nightly build -p sel4-sys-extern-wrapper --release \
  --target=cohesix_root/sel4-aarch64.json \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem
```
