#!/bin/bash
# CLASSIFICATION: COMMUNITY
# Filename: setup_mac_env.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-06-09

set -euo pipefail

export COHROLE=QueenPrimary
export LOG_FILE="./log/mac_env.log"
export SUMMARY_FILE="VALIDATION_SUMMARY.md"
export CLI_PATH="./tools"
export TRACE_BASE="./traces"

mkdir -p "$(dirname "$LOG_FILE")" "$CLI_PATH" "$TRACE_BASE"

{
  echo "🛠️ Cohesix Mac Setup — $(date)"
  echo ""

  echo "==> Checking Python..."
  if command -v python3 && command -v pip3; then
    echo "✅ Python3 and pip3: OK"
  else
    echo "❌ Python3 or pip3 missing. Install via Homebrew: brew install python"
    exit 1
  fi

  echo "==> Installing Python dependencies (if requirements.txt exists)..."
  if [[ -f "$CLI_PATH/requirements.txt" ]]; then
    python3 -m pip install -r "$CLI_PATH/requirements.txt"
    echo "✅ Python dependencies installed"
  else
    echo "⚠️ No requirements.txt found at $CLI_PATH"
  fi

  echo "==> Checking for 9P tools (optional)..."
  if command -v 9pfuse &>/dev/null; then
    echo "✅ 9P tools installed"
  else
    echo "⚠️ 9P tools not found. Install via brew: brew install plan9port"
  fi

  echo "==> Checking CUDA (optional)..."
  if command -v nvidia-smi &>/dev/null; then
    echo "✅ NVIDIA GPU detected"
  else
    echo "⚠️ No NVIDIA GPU found — expected on most Macs"
  fi

  echo ""
  echo "✅ Mac environment setup complete."

} | tee "$LOG_FILE"

# Optional: Write human-readable summary
cat > "$SUMMARY_FILE" <<EOF
# ✅ Cohesix Mac Environment Summary

- Role: $COHROLE
- CLI Path: $CLI_PATH
- Trace Path: $TRACE_BASE
- Python: $(python3 --version)
- Pip: $(pip3 --version)

EOF

echo "Setup finished. Summary written to $SUMMARY_FILE"
