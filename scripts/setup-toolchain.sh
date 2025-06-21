# CLASSIFICATION: COMMUNITY
# Filename: setup-toolchain.sh v0.1
# Date Modified: 2025-06-05
# Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# setup-toolchain.sh – Cohesix helper
#
# Installs the Rust, Go and C cross-compilation toolchain needed to
# build Cohesix. This script is idempotent and safe to run multiple times.
###############################################################################
set -euo pipefail

msg() { printf "\e[32m==>\e[0m %s\n" "$*"; }
warn() { printf "\e[33m[WARN]\e[0m %s\n" "$*" >&2; }

if command -v apt-get >/dev/null; then
  msg "Updating package lists"
  apt-get update -y > /dev/null
  msg "Installing cross compiler packages"
  DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential gcc-aarch64-linux-gnu g++-aarch64-linux-gnu > /dev/null
fi

# Install rustup if missing
if ! command -v rustup >/dev/null; then
  msg "Installing rustup"
  curl https://sh.rustup.rs -sSf | sh -s -- -y > /dev/null
  source "$HOME/.cargo/env"
fi

# Ensure Go is installed
if ! command -v go >/dev/null; then
  msg "Installing Go"
  curl -L https://go.dev/dl/go1.23.8.linux-amd64.tar.gz -o /tmp/go.tgz
  rm -rf /usr/local/go && tar -C /usr/local -xzf /tmp/go.tgz
  export PATH=$PATH:/usr/local/go/bin
fi

msg "Adding Rust target aarch64-unknown-linux-gnu"
if ! rustup target list --installed | grep -q 'aarch64-unknown-linux-gnu'; then
  if ! rustup target add aarch64-unknown-linux-gnu; then
    warn "Rust target aarch64-unknown-linux-gnu could not be installed."
  fi
fi

msg "Toolchain installation complete."
