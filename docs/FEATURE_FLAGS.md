// CLASSIFICATION: COMMUNITY
// Filename: FEATURE_FLAGS.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

# Feature Flags

Cohesix uses optional Cargo features to toggle runtime capabilities.

- `secure9p` enables the TLS-backed 9P server using `config/secure9p.toml`.
- `no-cuda` disables GPU initialization even if CUDA is present.
- `busybox_client` provides the BusyBox shell wrapper used in tests.
- `busybox_build` builds BusyBox utilities for initfs.

Run `cargo test --features secure9p` to validate Secure 9P policies.
