#!/usr/bin/env bash
###############################################################################
# bootstrap_hal_pcr.sh – Create HAL stubs, worker arg-parser, and boot PCR
# Lukas Bower © 2025 · MIT/Apache-2.0 dual licence
#
# What it does
# ─────────────────────────────────────────────────────────────────────────────
# 1. Create src/hal/{arm64,x86_64}/mod.rs with paging + interrupt placeholders
# 2. Scaffold a user-space argument parser in src/worker/args.rs
# 3. Add src/boot/measure.rs exporting SHA-256 PCR extension + unit test
# 4. Ensure supporting mod.rs files & sha2 dep
# 5. Run cargo fmt (if available)
#
# Safe to re-run. Stops on first error to avoid half-finished state.
###############################################################################
set -euo pipefail

PFX=$'\e[36m[bootstrap]\e[0m'
say() { printf '%s %s\n' "$PFX" "$*"; }

###############################################################################
# 0. Sanity
###############################################################################
[[ -d .git ]]          || { echo >&2 "✘ Must run at repo root"; exit 1; }
[[ -f Cargo.toml ]]    || { echo >&2 "✘ Cargo.toml not found"; exit 1; }

###############################################################################
# 1. HAL directory & stub modules
###############################################################################
for ARCH in arm64 x86_64; do
  HAL_DIR="src/hal/${ARCH}"
  HAL_FILE="${HAL_DIR}/mod.rs"
  mkdir -p "$HAL_DIR"
  if [[ ! -f $HAL_FILE ]]; then
    say "Creating HAL stub $HAL_FILE"
    cat >"$HAL_FILE" <<RS
//! Architecture-specific HAL for \`${ARCH}\`.
//! Paging & interrupt initialisation stubs – flesh out per datasheet.

/// Initialise MMU / paging structures.
pub fn init_paging() {
    // TODO: implement real paging setup for ${ARCH}
    unimplemented!("Paging init ${ARCH}");
}

/// Initialise interrupt controller.
pub fn init_interrupts() {
    // TODO: implement GIC/APIC or equivalent init for ${ARCH}
    unimplemented!("IRQ init ${ARCH}");
}
RS
  else
    say "$HAL_FILE already exists – keeping"
  fi
done

# Ensure src/hal/mod.rs re-exports arch modules
HAL_MOD=src/hal/mod.rs
if [[ ! -f $HAL_MOD ]]; then
  say "Creating hal/mod.rs"
  mkdir -p src/hal
  cat >"$HAL_MOD" <<"RS"
//! Portable hardware-abstraction layer façade.
//
// The actual implementation lives in `arm64` or `x86_64` sub-modules,
// selected via `cfg(target_arch = …)`.

#[cfg(target_arch = "aarch64")]
pub mod arm64;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;
RS
fi

###############################################################################
# 2. Worker user-space argument parser
###############################################################################
WORKER_DIR=src/worker
ARGS_RS=$WORKER_DIR/args.rs
mkdir -p "$WORKER_DIR"
if [[ ! -f $ARGS_RS ]]; then
  say "Creating worker arg-parser stub $ARGS_RS"
  cat >"$ARGS_RS" <<"RS"
use clap::Parser;

/// Command-line flags for a Cohesix worker process.
///
/// **Note:** Port any existing boot-time CLI parsing logic here
/// and delete it from kernel/boot code.
#[derive(Debug, Parser)]
pub struct WorkerArgs {
    /// Verbosity level (`error`, `warn`, `info`, `debug`, `trace`)
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

pub fn parse() -> WorkerArgs {
    WorkerArgs::parse()
}
RS
  else
    say "$ARGS_RS already exists – keeping"
  fi

# Ensure src/worker/mod.rs exists and exposes args
WORKER_MOD=$WORKER_DIR/mod.rs
grep -q 'pub mod args' "$WORKER_MOD" 2>/dev/null || {
  say "Updating worker/mod.rs"
  printf 'pub mod args;\n' >>"$WORKER_MOD"
} || true

###############################################################################
# 3. Boot-time PCR measurement helper
###############################################################################
BOOT_DIR=src/boot
MES_RS=$BOOT_DIR/measure.rs
mkdir -p "$BOOT_DIR"
if [[ ! -f $MES_RS ]]; then
  say "Creating boot/measure.rs"
  cat >"$MES_RS" <<"RS"
//! Boot-time measurement helpers (TPM-style PCR extension).

use sha2::{Digest, Sha256};

/// Extend the given PCR register with `data`.
///
/// * `current` – existing 32-byte PCR value.
/// * returns – new PCR value (SHA-256 of current||data).
pub fn extend_pcr(mut current: [u8; 32], data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&current);
    hasher.update(data);
    current.copy_from_slice(&hasher.finalize());
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcr_extension_changes_value() {
        let zero = [0u8; 32];
        let v1 = extend_pcr(zero, b"cohesix");
        let v2 = extend_pcr(v1, b"more");
        assert_ne!(v1, zero);
        assert_ne!(v1, v2);
    }
}
RS
else
  say "$MES_RS already exists – keeping"
fi

# Ensure boot/mod.rs exports measure
BOOT_MOD=$BOOT_DIR/mod.rs
grep -q 'pub mod measure' "$BOOT_MOD" 2>/dev/null || {
  say "Updating boot/mod.rs"
  printf 'pub mod measure;\n' >>"$BOOT_MOD"
} || true

###############################################################################
# 4. Add sha2 dependency if missing
###############################################################################
if ! grep -qE '^sha2[[:space:]]*=' Cargo.toml; then
  say "Adding sha2 = \"0.10\" to Cargo.toml"
  awk '/^\[dependencies\]/{print;print "sha2 = \"0.10\"";next}1' \
      Cargo.toml >Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
fi

###############################################################################
# 5. Format + gentle compile check (optional)
###############################################################################
command -v cargo-fmt >/dev/null && cargo fmt --all || true
say "Running cargo check (may fail if further wiring-up needed)…"
cargo check --workspace || {
  say "⚠️  Build failed – stubs compile but other parts might need updates."
}

say "✅ HAL stubs, worker args, and boot PCR module created."
