// CLASSIFICATION: COMMUNITY
// Filename: DEPENDENCIES.md v0.2
// Date Modified: 2025-05-24

# System Dependencies

| Dependency            | Version      | Source / License            | Notes                                               |
|-----------------------|--------------|-----------------------------|-----------------------------------------------------|
| seL4 L4 microkernel   | 2025.05      | https://sel4.systems (MIT)  | Kernel foundation with Cohesix patches              |
| Plan 9 userland       | 9front 2025.05| https://9front.org (MIT/BSD)| 9P filesystem, `rc` shell, minimal POSIX subset     |
| BusyBox               | 1.35.0       | https://busybox.net (GPL-2) | Core UNIX tools + shell for lightweight utilities   |
| musl libc             | 1.2.3        | https://musl.libc.org (MIT) | POSIX-compatibility for Plan 9 ports and BusyBox    |
| Go                    | 1.20+        | https://golang.org (BSD-style)| CSP-based 9P services and auxiliary tooling       |
| Python                | 3.10+        | https://python.org (PSF)    | DSL, testing harnesses, runtime validators          |
| C++17 & CUDA Toolkit  | 11.8 / 11.8  | NVIDIA EULA (Proprietary)   | Physics core (Rapier), Torch/TensorRT for GPU deploy|

# Rust Crate Dependencies

| Crate           | Version   | Source / License     | Purpose                        |
|-----------------|-----------|----------------------|--------------------------------|
| anyhow          | 1.0.68    | crates.io (MIT)      | Ergonomic error handling       |
| clap            | 4.1       | crates.io (MIT)      | Command-line argument parsing  |
| serde           | 1.0       | crates.io (MIT)      | Data serialization/deserialization |
| serde_json      | 1.0       | crates.io (MIT)      | JSON support                   |
| tokio           | 1.28      | crates.io (MIT)      | Async runtime                  |
| rapier3d        | 0.14      | crates.io (MIT)      | Physics simulation engine      |

# Tooling Dependencies

| Tool            | Version    | Source / License       | Purpose                           |
|-----------------|------------|------------------------|-----------------------------------|
| OpenSSH         | 9.4p1      | https://openssh.com (BSD) | Secure remote access           |
| mandoc / man-db | 2.0.10     | BSD                    | Manual page rendering             |
| BusyBox (CLI)   | 1.35.0     | https://busybox.net (GPL-2) | Coreutils and shell support    |