// CLASSIFICATION: COMMUNITY
// Filename: QUICKSTART.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Cohesix Quick Start

This short guide gets you from a fresh clone to tracing your first boot in five steps.

All commands emit validator-compatible trace logs to `./log/trace/` and snapshots to `./history/snapshots/`.

1. **Clone the repo**
   ```bash
   git clone https://github.com/cohesix/cohesix.git
   cd cohesix
   ```
2. **Install tools** – ensure Rust, Go, and Python3 are available. On Debian/Ubuntu:
   ```bash
   sudo apt install build-essential golang python3 python3-pip
   curl https://sh.rustup.rs -sSf | sh
   pip3 install -r tools/requirements.txt
   ```
3. **Build everything**
   ```bash
   make all
   ```
4. **Run the sample CLI**
   ```bash
   ./target/debug/cohcli run demo
   ```
5. **Trace and explore**
   ```bash
   ./target/debug/cohtrace last --pretty
   ```
   Traces are stored in `./history/` and can be examined with `cohtrace` commands.

   You can diff traces between runs using:
   ```bash
   ./target/debug/cohtrace diff --from last --to previous
   ```

For more CLI examples see `docs/community/AGENTS_AND_CLI.md`.
