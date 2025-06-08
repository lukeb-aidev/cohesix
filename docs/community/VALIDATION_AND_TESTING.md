// CLASSIFICATION: COMMUNITY
// Filename: VALIDATION_AND_TESTING.md v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Validation and Testing

The trace validator ensures simulations and SLM deployments behave predictably.
Example traces live in [`examples/trace_example_physics.json`](../../examples/trace_example_physics.json).

## Running the Validator
```bash
cohtrace push_trace worker01 examples/trace_example_physics.json
```

Run `python scripts/validate_metadata_sync.py` before committing docs.

## CI Hardware Validation

The GitHub Actions workflow now boots the Jetson Orin Nano and Raspberry Pi 5
test units. `scripts/collect_boot_logs.sh` gathers `/srv/boot.log` and
`/trace/boot.log` from each device, uploading them as artifacts. The workflow
replays `examples/trace_example_physics.json` via `cohtrace.py` and runs
`./test_all_arch.sh` to ensure all architectures pass.
