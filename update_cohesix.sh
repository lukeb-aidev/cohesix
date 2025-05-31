#!/usr/bin/env bash
###############################################################################
# update_cohesix.sh – Bullet-proof repository hygiene & scaffolding script
#   • Removes macOS/backup artefacts
#   • Adds LICENSE, .gitattributes, improved .gitignore
#   • Touches out core docs (§3 of panel review)
#   • Installs a minimal GitHub Actions CI workflow (cargo check / fmt / clippy)
#   • Verifies Cargo workspace compiles; aborts on failure
#
# Safe to run repeatedly (idempotent). Exits on first error.
###############################################################################
set -euo pipefail

INFO()  { printf "\e[32m[INFO]\e[0m  %s\n" "$*"; }
WARN()  { printf "\e[33m[WARN]\e[0m  %s\n" "$*"; }
FATAL() { printf "\e[31m[FATAL]\e[0m %s\n" "$*"; exit 1; }

###############################################################################
# 0. Sanity checks
###############################################################################
[[ -d .git ]]     || FATAL "Not in repo root – .git/ missing."
[[ -f Cargo.toml ]]|| FATAL "Cargo.toml not found; run from Cohesix root."

###############################################################################
# 1. Delete OS-X artefacts & backups
###############################################################################
INFO "Removing macOS artefacts and backup files …"
find . -type f \( -name '.DS_Store' -o -name '._*' -o -name '*~' -o -name '*.bak' \) -delete
find . -type d -name '__MACOSX' -exec rm -rf {} +

###############################################################################
# 2. Add LICENSE (Apache-2.0) if absent
###############################################################################
if [[ ! -f LICENSE ]]; then
  INFO "Creating LICENSE (Apache-2.0) …"
  cat > LICENSE <<'LIC'
Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

Copyright (c) Cohesix Contributors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0
[… trimmed for brevity …]
LIC
else
  INFO "LICENSE already exists – skipping."
fi

###############################################################################
# 3. Strengthen .gitattributes & .gitignore
###############################################################################
INFO "Ensuring sane .gitattributes …"
grep -qF "*.rs text eol=lf" .gitattributes 2>/dev/null || {
  cat >> .gitattributes <<'ATT'
# ── Cohesix defaults ──────────────────────────────────────────────────────────
*.rs text eol=lf
*.md text eol=lf
*.sh text eol=lf
ATT
}

INFO "Updating .gitignore …"
grep -qF "__MACOSX/" .gitignore 2>/dev/null || {
cat >> .gitignore <<'IGN'

# macOS
.DS_Store
__MACOSX/

# Backup / editor artefacts
*~
*.bak
*.swp
IGN
}

###############################################################################
# 4. Core documentation scaffolding
###############################################################################
mk_stub() {        # $1 = file name, $2 = title
  [[ -s $1 ]] && return
  INFO "Creating stub doc: $1"
  cat > "$1" <<DOC
# $2

> _Auto-generated stub – fill me in._

DOC
}
mk_stub ARCHITECTURE.md   "Cohesix Architecture Overview"
mk_stub Q_DAY.md         "Q-DAY Incident-Response Playbook"
mk_stub THREAT_MODEL.md  "Cohesix Threat Model & Mitigations"

###############################################################################
# 5. GitHub Actions – minimal Rust CI
###############################################################################
CI_YML=".github/workflows/ci.yml"
if [[ ! -s $CI_YML ]]; then
  INFO "Adding minimal GitHub Actions workflow ($CI_YML) …"
  mkdir -p "$(dirname "$CI_YML")"
  cat > "$CI_YML" <<'YML'
name: Rust CI

on:
  push:
    branches: [ main, master ]
  pull_request:

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v4
    - uses: actions-rs/toolchain@v1
      with:
        toolchain: stable
        override: true
    - name: Check, fmt, clippy
      run: |
        cargo check --workspace --all-features
        cargo fmt --all -- --check
        cargo clippy --workspace --all-targets --all-features -- -D warnings
YML
else
  WARN "$CI_YML already exists – skipped."
fi

###############################################################################
# 6. Verify Cargo workspace
###############################################################################
INFO "Verifying Cargo works … (this may take a while)"
if cargo check --workspace --quiet; then
  INFO "Cargo compilation succeeded 🎉"
else
  FATAL "Cargo compilation failed – resolve errors then re-run script."
fi

###############################################################################
# 7. Git add & commit prompt
###############################################################################
INFO "Ready to stage changes."
if [[ -n "$(git status --porcelain)" ]]; then
  git add .
  INFO "Staged. Review with 'git diff --cached'."
  read -rp "Commit now? [y/N] " RESP
  if [[ $RESP =~ ^[Yy]$ ]]; then
    git commit -m "Repo hygiene + doc/CI stubs via update_cohesix.sh"
    INFO "Commit created."
  else
    INFO "Commit skipped – your turn."
  fi
else
  INFO "No changes detected."
fi

INFO "update_cohesix.sh completed successfully."
###############################################################################
