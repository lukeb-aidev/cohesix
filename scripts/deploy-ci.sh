// CLASSIFICATION: COMMUNITY
// Filename: deploy-ci.sh v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# deploy-ci.sh – Cohesix helper
#
# A simple one‑liner CI wrapper that:
#   1. Verifies the workspace builds & tests green
#   2. Generates an artefact ZIP of compiled binaries + docs
#   3. Optionally tags the commit and pushes the artefact to a local `ci_out/`
#
# Intended for use by GitHub Actions, GitLab CI, or local smoke runs.
#
# Usage:
#   ./scripts/deploy-ci.sh                # build, test, zip → ./ci_out
#   RELEASE=1 ./scripts/deploy-ci.sh      # additionally tag & sign git release
#
# Required tools: cargo, git, zip
###############################################################################
set -euo pipefail

msg() { printf "\e[36m[deploy]\e[0m %s\n" "$*"; }
err() { printf "\e[31m[error]\e[0m %s\n" "$*" >&2; exit 1; }

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

[[ -f Cargo.toml ]] || err "No Cargo.toml found at repo root"

###############################################################################
# 1. Build & test
###############################################################################
msg "Running cargo check …"
cargo check --workspace

msg "Running cargo test …"
cargo test --workspace

###############################################################################
# 2. Compile release binaries
###############################################################################
msg "Compiling release binaries …"
cargo build --workspace --release

###############################################################################
# 3. Collect artefacts
###############################################################################
OUT_DIR="$ROOT_DIR/ci_out"
BIN_DIR="$OUT_DIR/bin"
DOC_DIR="$OUT_DIR/docs"

msg "Collecting artefacts → $OUT_DIR"
rm -rf "$OUT_DIR"
mkdir -p "$BIN_DIR" "$DOC_DIR"

# Copy all release binaries
find target/release -maxdepth 1 -type f -perm -111 -name "coh*" -exec cp {} "$BIN_DIR" \;

# Copy COMMUNITY docs
find docs/community -name '*.md' -exec cp {} "$DOC_DIR" \;

# Zip them
ZIP_NAME="cohesix_$(date +%Y%m%d_%H%M%S).zip"
(
  cd "$OUT_DIR"
  zip -qr "../$ZIP_NAME" .
)
msg "✅ Artefact created: $ZIP_NAME"

###############################################################################
# 4. Optional git tag + push (RELEASE=1)
###############################################################################
if [[ "${RELEASE:-0}" == "1" ]]; then
  TAG="ci-$(date +%Y%m%d_%H%M%S)"
  msg "Tagging commit as $TAG"
  git tag -a "$TAG" -m "CI release $TAG"
  git push origin "$TAG"
  msg "Git tag pushed."
fi

msg "deploy-ci.sh completed successfully."