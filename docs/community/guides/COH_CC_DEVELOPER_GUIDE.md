

// CLASSIFICATION: COMMUNITY
// Filename: COH_CC_DEVELOPER_GUIDE.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Coh_CC Developer Guide

Welcome to the Coh_CC compiler and runtime environment guide for Cohesix. This document introduces supported languages, platform capabilities, and coding conventions to help you develop secure, performant, and resilient applications on the Cohesix OS.

---

## Supported Languages

Coh_CC currently supports a streamlined and secure subset of the following languages:

| Language | Purpose | Notes |
|----------|---------|-------|
| **Rust** | Low-level, secure systems code | Recommended for drivers, kernel extensions, and secure agents |
| **Go** | Userland services and orchestration | Ideal for CLI tools, APIs, and concurrent workflows |
| **Python** | Tooling, scripts, prototyping | Used for glue logic, simulation, and validator hooks |
| **C** | Kernel-level patches, legacy code support | Required for seL4 compatibility |
| **C++ (CUDA)** | GPU workloads on Jetson targets | Used for inference, vector processing, and compute acceleration |

Additional language support is under evaluation, including WebAssembly for portable sandboxed agents.

---

## Build Targets and Capabilities

Coh_CC enables development for all Cohesix roles and execution layers:

### Target Roles
- **QueenPrimary**: cloud orchestrator, runs CI, agent coordination, policy engines
- **Worker (Jetson / Pi)**: edge compute agents, simulators, CUDA inference
- **KioskInteractive**: minimal input/output clients
- **GlassesAgent**: wearable interfaces, HUDs
- **SimulatorTest**: scenario execution, trace validation
- **DroneWorker / SensorRelay**: streaming, telemetry

### Features Supported by Coh_CC:
- Sandboxed syscall wrappers
- Secure 9P namespace exposure
- Live validator hooks
- Trusted boot and attestation flows
- GPU offload (via `/srv/cuda`)
- Physics runtime (via `/sim/` with Rapier)

---

## Hello World Examples

### 1. Rust (Trusted Agent)
```rust
// src/agent/hello.rs
use cohesix_syscalls::log_info;

fn main() {
    log_info!("Hello from a secure Coh_CC agent!");
}
```

### 2. Go (Userland CLI)
```go
// tools/hello/main.go
package main
import "fmt"

func main() {
    fmt.Println("Hello from Coh_CC Go CLI")
}
```

### 3. Python (Trace Tool)
```python
# tools/trace_hello.py
print("Hello from Python trace utility")
```

### 4. C (Bootloader Extension)
```c
// src/boot/hello.c
#include <coh/boot.h>
void bootlog() {
    boot_log("Hello from Coh_CC boot module");
}
```

---

## Compilation and Deployment

Use the `cohbuild` tool to compile and deploy code:
```bash
cohbuild build --target worker --role SensorRelay --release
cohbuild deploy --target /mnt/cohesix_active/
```

Coh_CC ensures:
- Deterministic builds across platforms
- Capability-safe linking
- Automatic validator registration for agents

---

## Contributing and Extending

If you wish to:
- Add support for a new language or runtime
- Extend validator rules
- Submit kernel-mode logic or GPU modules

... please refer to `CONTRIBUTING.md` and `FABRIC_OS_API.md`.

---

## Next Steps

- Read `COH_CC_ARCHITECTURE.md` for compiler internals
- Review example agents in `examples/`
- Validate using `cohtrace` and `cohrun`
- Join the dev channel: `#cohesix-dev` on Matrix

---

_This document is part of the Cohesix Community Documentation Set. For updates, check METADATA.md._