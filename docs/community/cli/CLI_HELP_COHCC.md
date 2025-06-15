// CLASSIFICATION: COMMUNITY
// Filename: CLI_HELP_COHCC.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-15

# cohcc

*(Applies to: DroneWorker, SimulatorTest)*

```bash
Usage: cohcc --input <file> [--output a.out] [--target x86_64|aarch64] [--timeout ms]
```

See CLI README.md for full role-by-command mapping.

## Options
- `--input` – path to IR file to compile (required)
- `--output` – output file path (default `a.out`)
- `--target` – compilation target architecture
- `--timeout` – request timeout in milliseconds

## Examples
```bash
# Compile an IR file for aarch64
cohcc --input demo.ir --output demo.c --target aarch64
```
