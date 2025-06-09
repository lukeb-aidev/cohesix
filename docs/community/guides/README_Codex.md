// CLASSIFICATION: COMMUNITY
// Filename: README_Codex.md v1.4
// Date Modified: 2025-06-16
// Author: Lukas Bower

# README Codex

This document explains how the Cohesix team integrates OpenAI Codex as a core development accelerator. Codex enables contextual code generation, scaffolding of new modules, and AI-assisted refactoring directly in VS Code or via the `cohcli codex` commands.

With these instructions, developers will be able to:
- Rapidly scaffold new service templates, device drivers, and test stubs with minimal boilerplate.
- Automate repetitive coding tasks (e.g., generating RPC interfaces, serialization routines, and CI workflows).
- Iterate on design patterns by leveraging AI-driven suggestions inline in VS Code.
- Maintain best practices for prompt design and secure API usage.

Follow the steps below to configure your environment and start using Codex within the Cohesix project.


## Prerequisites

1. **OpenAI API Key**  
   - Export your key:  
     ```bash
     export OPENAI_API_KEY="sk-<your_key_here>"
     ```
2. **GitHub Authentication**  
   - Install the GitHub CLI: `brew install gh` (macOS) or `sudo apt install gh` (Linux).  
   - Authenticate: `gh auth login` (choose GitHub.com, OAuth flow).
3. **Cohesix CLI (`cohcli`)**  
   - Ensure Python 3.10+ and `pip` are installed.  
   - Install:  
     ```bash
     pip install cohcli
     ```
4. **VS Code & ChatGPT Extension**
   - Install the ChatGPT or CodeGPT extension in VS Code.
   - Configure the extension to use `OPENAI_API_KEY` from your environment.
5. **Verify macOS Setup**
   - Run the helper script:
     ```bash
     scripts/verify-macos-setup.sh
     ```

## macOS Setup Verification

Run the verification script whenever you clone the repository or upgrade
tooling. It checks for Homebrew, Xcode command line tools, Python 3.10+, git,
and runs `validate_metadata_sync.py`.

## Git Workflow Setup

- **Branching Model**: Use feature branches named `codex/<task-name>`.  
- **Commit Hooks**: Install `pre-commit` and enable the Codex prompt linter:  
  ```bash
  pip install pre-commit
  pre-commit install
  ```
 - **Agent Definitions**: `docs/community/architecture/AGENTS_AND_CLI.md` contains JSON schema for agent roles and prompts. Review and extend to add new Codex agents or tasks.

## Agents & Task Schema

 - **Agents.md**: Defines each automated agent (e.g., `docs/community/architecture/AGENTS_AND_CLI.md`). Contains fields:
  - **`id`**: Unique agent name.  
  - **`role`**: Defines permissions and context (e.g., `codegen`, `testing`).  
  - **`prompt_template`**: Reusable prompt variables for tasks.  
- Codex commands (`cohcli codex run <agent_id> [--file <path>]`) will look up `AGENTS_AND_CLI.md` and fill in prompts automatically.

## Best Practices for Bulletproof Operation

1. **Pin Versions**: In `pyproject.toml` or `requirements.txt`, pin `openai`, `cohcli`, and any AI tooling to a specific version.  
2. **Prompt Tests**: For each agent, add sample input/expected output tests in `tests/codex/`. Run with `pytest tests/codex/`.  
3. **CI Integration**:  
   - Add a GitHub Actions workflow `.github/workflows/codex.yml` to validate prompts and responses:  
     ```yaml
     on: [pull_request]
     jobs:
       codex:
         runs-on: ubuntu-latest
         steps:
           - uses: actions/checkout@v3
           - name: Setup Python
             uses: actions/setup-python@v4
             with:
               python-version: '3.10'
           - run: pip install cohcli pre-commit pytest
           - run: pre-commit run --all-files
           - run: pytest tests/codex/
     ```
4. **Audit Logs**: Store all Codex-generated outputs in `codex_logs/` with timestamps. Configure `cohcli` to write logs:  
   ```bash
   cohcli codex run <agent_id> --file path --log-dir codex_logs/
   ```
5. **Human Review Gate**: Require at least one code review approval for any PR with Codex-generated changes. Enforce via branch protection rules.
6. **Metadata Sync**: Run `python scripts/validate_metadata_sync.py` before pushing changes to ensure document headers match `METADATA.md`.

## Getting Started Quickly

1. **Run a sample agent:**  
   ```bash
   cohcli codex run scaffold_service --file src/service.rs
   ```
2. **Review & iterate:** Open the generated file in VS Code, refine the prompt if needed.  
3. **Test & commit:** Run local tests, commit to `codex/<task>` branch, open PR.

---

By following these steps, you’ll ensure a repeatable, secure, and maintainable workflow for AI-assisted coding in the Cohesix project. Happy coding!