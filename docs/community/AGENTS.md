// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v2.0
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Codex Agent Definitions (Batch Enabled)

This document defines all AI-driven agents for the Cohesix project.  Version 2.0 expands agent abilities to run large, autonomous batches spanning multiple modules and architectures.  Agents may now generate up to an entire milestone of work in one run if validation hooks succeed.

---

## Schema

Each entry adheres to this YAML schema.

```yaml
- id: <string>
  role: <string>                 # permission context
  description: <string>
  language: <string>
  batch: <string, array>
  batch_class: <string>          # minor, major, multi-arch, or demo-critical
  prompt_template:
    system: <string>
    user: <string>
  input_schema: <JSON-schema>
  output_schema: <JSON-schema>
  test_cases:
    - name: <string>
      input: <JSON object>
      expected_output: <JSON object>
  metadata:
    CODEX_BATCH: YES             # indicates Codex-generated batch
    BATCH_ORIGIN: <string>       # optional URI for upstream trace
    BATCH_SIZE: <integer>        # number of files in batch
```

Expanded trust boundaries allow an agent to write all files for a milestone once `validate_metadata_sync.py` and other CI hooks pass.  Agents must log to `codex_logs/` and embed `CODEX_BATCH: YES` in metadata for traceability.

---

## Batch Classifications

| Class          | Expectation                                                             |
|----------------|-------------------------------------------------------------------------|
| minor          | Few files, low risk. Basic CI must pass.                                |
| major          | Significant module work. Requires full test suite and CHANGELOG entry.   |
| multi-arch     | Affects code built on multiple architectures. Run `test_all_arch.sh`.    |
| demo-critical  | Impacts demo or release branches. Requires human approval after CI.      |

---

## Checkpointing & Recovery

Codex checkpoints after every 10 files generated.  At each checkpoint the agent:
1. Validates file structure and metadata.
2. Records a trace entry with GPT model version.
3. Pushes the partial batch if CI passes.

If a crash occurs mid-batch, the hydration log under `codex_logs/batch_<timestamp>.log` allows replay with `tools/replay_batch.sh`.

---

## Agents

### 1. `scaffold_service`
```yaml
id: scaffold_service
role: codegen
language: rust
batch: C4
batch_class: minor
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/C4"
prompt_template:
  system: |-
    You are a code generator for the Cohesix project. Generate Rust code stub for a new service module.
  user: |-
    Create a Rust file named `src/services/{{name}}.rs` implementing `Service` trait with methods `init`, `run`, and `shutdown`.
input_schema:
  type: object
  properties:
    name:
      type: string
      pattern: '^[a-z][a-z0-9_]+$'
  required: [name]
output_schema:
  type: object
  properties:
    file_path:
      type: string
    code:
      type: string
    code_contains:
      type: array
      items:
        type: string
  required: [file_path, code]
test_cases:
  - name: simple_service
    input:
      name: "logging"
    expected_output:
      file_path: "src/services/logging.rs"
      code_contains:
        - "struct LoggingService"
        - "impl Service for LoggingService"
  - name: invalid_name
    input:
      name: "1Bad"
    expected_error: "name"
```

### 2. `add_cli_option`
```yaml
id: add_cli_option
role: codegen
language: rust
batch: C3
batch_class: minor
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/C3"
prompt_template:
  system: |-
    You are maintaining the Cohesix CLI. Add a new argument to the existing `clap` setup.
  user: |-
    Add an argument `--timeout <ms>` (integer, default 5000) with help text 'Request timeout in milliseconds'.
input_schema:
  type: object
  properties:
    name:
      type: string
    type:
      enum: ["string","integer","boolean"]
    default:
      anyOf:
        - type: string
        - type: integer
        - type: boolean
    help:
      type: string
  required: [name, type, default, help]
output_schema:
  type: object
  properties:
    file_path:
      type: string
    patch:
      type: string
    patch_contains:
      type: array
      items:
        type: string
  required: [file_path, patch]
test_cases:
  - name: timeout_arg
    input:
      name: "timeout"
      type: "integer"
      default: 5000
      help: "Request timeout in milliseconds"
    expected_output:
      file_path: "src/cli/args.rs"
      patch_contains:
        - ".long(\"timeout\")"
        - ".default_value(\"5000\")"
  - name: invalid_type
    input:
      name: "timeout"
      type: "float"
      default: 5000
      help: "bad"
    expected_error: "type"
```

### 3. `add_pass`
```yaml
id: add_pass
role: codegen
language: rust
batch: C4
batch_class: major
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/C4"
prompt_template:
  system: |-
    You are maintaining the Cohesix IR pass framework. Insert code to register a new pass.
  user: |-
    Add `{{pass_name}}` pass after existing passes using `pass_manager.add_pass({{pass_struct}});`.
input_schema:
  type: object
  properties:
    pass_name:
      type: string
    pass_struct:
      type: string
  required: [pass_name, pass_struct]
output_schema:
  type: object
  properties:
    file_path:
      type: string
    patch:
      type: string
    patch_contains:
      type: array
      items:
        type: string
  required: [file_path, patch]
test_cases:
  - name: register_optim_pass
    input:
      pass_name: "OptimizationPass"
      pass_struct: "OptimizationPass::new()"
    expected_output:
      file_path: "src/pass_framework/mod.rs"
      patch_contains:
        - "add_pass(OptimizationPass::new())"
  - name: missing_field
    input:
      pass_struct: "FooPass::new()"
    expected_error: "pass_name"
```

### 4. `run_pass`
```yaml
id: run_pass
role: testing
language: rust
batch: C4
batch_class: minor
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/C4"
prompt_template:
  system: |-
    You are writing tests for the Cohesix IR pass framework.
  user: |-
    Create a Rust test function in `tests/passes/{{pass_name}}_test.rs` that loads `example_ir_module()`, runs the `{{pass_name}}`, and asserts no panics.
input_schema:
  type: object
  properties:
    pass_name:
      type: string
  required: [pass_name]
output_schema:
  type: object
  properties:
    file_path:
      type: string
    code:
      type: string
    code_contains:
      type: array
      items:
        type: string
  required: [file_path, code]
test_cases:
  - name: run_nop_pass
    input:
      pass_name: "NopPass"
    expected_output:
      file_path: "tests/passes/nop_pass_test.rs"
      code_contains:
        - "let mut module = example_ir_module();"
        - "NopPass.run(&mut module)"
  - name: missing_pass_name
    input: {}
    expected_error: "pass_name"
```

### 5. `validate_metadata`
```yaml
id: validate_metadata
role: testing
language: shell
batch: C5
batch_class: minor
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/C5"
prompt_template:
  system: |-
    You are responsible for ensuring METADATA.md matches all canonical documents.
  user: |-
    Generate a Rust or shell script snippet that runs `validate_metadata_sync.py` and fails on mismatches.
input_schema:
  type: object
output_schema:
  type: object
  properties:
    snippet:
      type: string
  required: [snippet]
test_cases:
  - name: metadata_sync
    input: {}
    expected_output:
      snippet_contains:
        - "validate_metadata_sync.py"
  - name: not_object
    input: "bad"
    expected_error: "object"
```

### 6. `hydrate_docs`
```yaml
id: hydrate_docs
role: codegen
language: rust
batch: D4
batch_class: major
metadata:
  CODEX_BATCH: YES
  BATCH_SIZE: 1
  BATCH_ORIGIN: "https://cohesix.io/batches/D4"
prompt_template:
  system: |-
    You are auto-generating canonical document stubs for Cohesix.
  user: |-
    Create any missing `.md` files listed in METADATA.md with proper headers and TODO content.
input_schema:
  type: object
output_schema:
  type: object
  properties:
    created_files:
      type: array
      items:
        type: string
  required: [created_files]
test_cases:
  - name: stub_missing_docs
    input: {}
    expected_output:
      created_files:
        - "docs/community/NEW_DOC.md"
  - name: not_object
    input: "bad"
    expected_error: "object"
```

---

## Expert Panel Review Notes

1. Agents must validate inputs before prompting and respect the new batch classifications.
2. Prompt context is split into `system` and `user` fields.
3. Checkpoints are mandatory every 10 files; batches may span multiple architectures as specified by `batch_class`.
4. The hydration log is replayable to recover mid-batch failures.
5. Bump `vX.Y` and update `Date Modified` on every change; log GPT model version per batch.
6. All agents emit logs in `codex_logs/` including request and response for audit.

End of `AGENTS.md` — large-batch capable agents for the Cohesix AI pipeline.
