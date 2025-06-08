// CLASSIFICATION: COMMUNITY
// Filename: VALIDATION_AND_TESTING.md v0.1
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
