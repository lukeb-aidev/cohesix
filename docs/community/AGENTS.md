// CLASSIFICATION: COMMUNITY
// Filename: AGENTS.md v1.5
// Date Modified: 2025-06-04
// Author: Lukas Bower

# Codex Agent Definitions

This file defines all AI-driven agents (via `cohcli codex`) used by the Cohesix project. Each agent is designed for a small, testable batch of work that composes reliably into the overall system.

---

## Schema

Each entry must adhere to this YAML schema:

```yaml
- id: <string>                   # unique agent identifier
  role: <string>                 # permission context (e.g. 'codegen', 'testing')
  description: <string>          # clear summary of agent’s purpose
  language: <string>             # optional language context for Codex optimization
  batch: <string, array>        # relevant work batches from BATCH_PLAN
  prompt_template:              # template with placeholders
    system: <string>            # system-level instruction
    user: <string>              # user-level task prompt
  input_schema: <JSON-schema>    # JSON Schema for agent inputs
  output_schema: <JSON-schema>   # JSON Schema for expected outputs
  test_cases:                    # list of validation cases
    - name: <string>
      input: <JSON object>
      expected_output: <JSON object>
# retries:
#   max_attempts: 3
#   backoff_ms: 500
```

Example:

```yaml
id: example_agent
batch: C3
role: codegen
language: rust
... # remaining fields
```

---

## Agents

### 1. `scaffold_service`
```yaml
id: scaffold_service
role: codegen
language: rust
batch: C4
description: Generates a new service module stub with boilerplate (imports, struct, trait impl).
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
description: Appends a new CLI argument to the `clap` parser in `src/cli/args.rs`.
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
id: add_pass
role: codegen
language: rust
batch: C4
description: Adds a new IR pass registration to the `PassManager` pipeline in `src/pass_framework/mod.rs`.
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
id: run_pass
role: testing
language: rust
batch: C4
description: Generates a test harness for running a specified IR pass against example IR data.
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
id: validate_metadata
role: testing
language: shell
batch: C5
description: Executes the metadata synchronization check and reports discrepancies.
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
id: hydrate_docs
role: codegen
language: rust
batch: D4
description: Generates missing canonical docs stubs under `docs/community` or `docs/private`.
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

1. **Error Handling:** Agents must validate inputs against `input_schema` before prompting to avoid ambiguous outputs.  
2. **Prompt Clarity:** Split prompts into `system` and `user` contexts to leverage OpenAI’s role separation.  
3. **Test Isolation:** Each `test_case` must run independently; mock file system where necessary.  
4. **Edge Cases:** Include tests for invalid inputs to ensure agents fail gracefully.  
5. **Versioning:** Bump `vX.Y` on any change and update `Date Modified`.  
6. **Orchestration Sequencing:** Ensure multi-agent workflows chain reliably; define clear hand-off inputs and outputs.  
7. **Timeouts & Retries:** Enforce per-agent execution time limits and retry logic for transient API failures.  
8. **Logging & Audit Trails:** Agents must emit logs in `codex_logs/` for each step, including request and response.  
9. **Version Control:** Update agent `vX.Y` and `Date Modified` on every change; maintain CHANGELOG.md entries.  
10. Language Context: Include a `language` field for Codex routing optimizations (e.g., Rust, Shell).

---

End of `AGENTS.md` — ensures bulletproof, small‐batch, testable agents for the Cohesix AI pipeline.
