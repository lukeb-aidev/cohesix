<!-- Author: Lukas Bower -->
<!-- Purpose: Bind the narrow macFUSE mount-selection patch to its immutable upstream source. -->
<!-- Copyright 2026 Lukas Bower -->
# fuser 0.18.0 macFUSE mounting compatibility

Upstream: https://github.com/cberner/fuser, crates.io `fuser` 0.18.0.
Archive SHA-256: `b82b6597d216503555ead6b358f341ef748869bf5c6fbae6a0cb9dd231baecfd`. Upstream MIT licensing is retained in `LICENSE.md`.

Cohesix carries this exact crate because its macOS build script ignores the
`libfuse3` feature and selects a legacy libfuse2 mount function.
macFUSE 5.3.3 implements `fuse_mount_compat25` as an unconditional failure.
`COHESIX.patch` is the complete upstream code change: honor explicit
`libfuse3` selection on macOS and preserve macFUSE's kernel protocol layout.
The existing libfuse3 session adapter, fd duplication, and FUSE request handlers
are unchanged. No new unsafe code or public API is introduced by this patch.
Linux retains its default pure Rust mount implementation; other selections
retain upstream behavior. `macos-no-mount` still takes precedence.

`apps/coh/Cargo.toml` enables `libfuse3` only for macOS. The build discovers the
installed library with pkg-config (`fuse3 >= 3.0.0`); it embeds no local SDK path.
macOS packaging therefore requires the libfuse3 library supplied by macFUSE 5.
Native Mac and Linux mount/read/unmount evidence is required for this repair;
source compilation alone does not prove native mounting.

All other crate files retain upstream content. Registry bookkeeping and the
crate's standalone Cargo.lock are omitted; the Cohesix workspace lock is the
build authority. Remove this patch when a pinned upstream version provides the
same Mac selection and passes both native checks.

The Rust risk audit admits this dependency only as the exact workspace
crates.io patch at `third_party/fuser`. A sorted tree digest covers every file,
including this provenance note, the complete patch and upstream build inputs.
Added, removed or changed files, special files, symlinks and path redirection
fail closed. Upstream runtime code stays an external dependency; first-party
risk ceilings and source-path checks remain in force.
