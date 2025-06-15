// CLASSIFICATION: COMMUNITY
// Filename: README_NATIVE.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Native Components

This directory demonstrates how native C++ and CUDA extensions can be built
with CMake. The example `hello` library prints a simple greeting and is
intended as a starting point for low-level integrations.

These native components support optional CUDA acceleration and can be linked into the Rust or Go layers of Cohesix. All builds should produce trace logs and optionally emit validator-compatible telemetry for debugging and simulation replay. See `BUILD_PLAN.md` and `VALIDATION_AND_TESTING.md` for integration details.
