#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and execute the Rust risk audit without Cargo runner or compiler override ambiguity.
# Copyright 2026 Lukas Bower

set -euo pipefail

PATH=/usr/bin:/bin
export PATH

repo_root=$(cd "$(/usr/bin/dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

for variable_name in $(compgen -e); do
  variable_name_upper=$(printf '%s' "$variable_name" | tr '[:lower:]' '[:upper:]')
  case "$variable_name_upper" in
    RUSTC|RUSTDOC|RUSTFLAGS|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUST_RISK_TARGET_DIR|RUSTUP_TOOLCHAIN|RUSTUP_HOME|RUSTUP_DIST_SERVER|RUSTUP_UPDATE_ROOT|RUSTUP_OVERRIDE_HOST|CARGO_HOME|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_RUSTC|CARGO_BUILD_RUSTDOC|CARGO_BUILD_RUSTFLAGS|CARGO_BUILD_RUSTC_WRAPPER|CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER|CARGO_BUILD_TARGET|CARGO_TARGET_*_LINKER|CARGO_TARGET_*_RUSTFLAGS|CARGO_TARGET_*_RUNNER)
      printf 'rust-risk gate refuses compiler, runner, or target-directory override environment variable: %s\n' "$variable_name" >&2
      exit 1
      ;;
  esac
done

canonical_home=$(/usr/bin/python3 -I -c 'import os, pwd; print(pwd.getpwuid(os.getuid()).pw_dir)')
if [[ -z "$canonical_home" || ! -d "$canonical_home" ]]; then
  printf 'rust-risk gate cannot resolve the canonical OS-account home\n' >&2
  exit 1
fi
if [[ "${HOME:-}" != "$canonical_home" ]]; then
  printf 'rust-risk gate refuses non-canonical HOME: expected=%s actual=%s\n' \
    "$canonical_home" "${HOME:-<unset>}" >&2
  exit 1
fi
HOME="$canonical_home"
export HOME

reject_external_cargo_config() {
  local config_path="$1"
  if [[ -e "$config_path" ]]; then
    printf 'rust-risk gate refuses external Cargo config: %s\n' "$config_path" >&2
    exit 1
  fi
}

cargo_home="${HOME}/.cargo"
if [[ -z "$cargo_home" ]]; then
  printf 'rust-risk gate cannot resolve Cargo home for external-config validation\n' >&2
  exit 1
fi
reject_external_cargo_config "${cargo_home}/config"
reject_external_cargo_config "${cargo_home}/config.toml"

ancestor="$repo_root"
while [[ "$ancestor" != "/" ]]; do
  ancestor=$(dirname "$ancestor")
  reject_external_cargo_config "${ancestor}/.cargo/config"
  reject_external_cargo_config "${ancestor}/.cargo/config.toml"
done

sha256_file() {
  local path="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    printf 'rust-risk gate requires shasum or sha256sum\n' >&2
    return 1
  fi
}

verify_bootstrap_file() {
  local path="$1"
  local expected="$2"
  local actual
  if [[ ! -f "$path" || -L "$path" ]]; then
    printf 'rust-risk bootstrap file is not a regular non-symlink file: %s\n' "$path" >&2
    return 1
  fi
  actual=$(sha256_file "$path")
  if [[ "$actual" != "$expected" ]]; then
    printf 'rust-risk bootstrap file hash mismatch: %s expected=%s actual=%s\n' \
      "$path" "$expected" "$actual" >&2
    return 1
  fi
}

verify_bootstrap_file \
  ".cargo/config.toml" \
  "96bd8e5f562e7bc9232b54fb918853a340c9a65fffc1b1d71a98e9e39d0aeb4d"
verify_bootstrap_file \
  "scripts/rustc-wrapper.sh" \
  "7e0b859852b0bab86736fd0a14f1706fa0681dd912469cc82d18f3dff5243de4"
verify_bootstrap_file \
  "rust-toolchain.toml" \
  "5b1a816f5e652e9e1ddd100ffc2cc8648a13fca72aa010e551a7053b405d8394"

if [[ -e ".cargo/config" ]]; then
  printf 'rust-risk gate refuses ambiguous .cargo/config alongside .cargo/config.toml\n' >&2
  exit 1
fi

pinned_toolchain="1.93.1"
rustup_path="${canonical_home}/.cargo/bin/rustup"
if [[ ! -x "$rustup_path" ]]; then
  printf 'rust-risk gate requires canonical Rustup: %s\n' "$rustup_path" >&2
  exit 1
fi

resolve_pinned_tool() {
  local tool_name="$1"
  local tool_path
  tool_path=$("$rustup_path" which --toolchain "$pinned_toolchain" "$tool_name")
  case "$tool_path" in
    "${canonical_home}/.rustup/toolchains/${pinned_toolchain}-"*/bin/"${tool_name}") ;;
    *)
      printf 'rust-risk gate resolved %s outside the canonical named toolchain: %s\n' \
        "$tool_name" "$tool_path" >&2
      return 1
      ;;
  esac
  if [[ ! -x "$tool_path" ]]; then
    printf 'rust-risk gate requires executable pinned tool: %s\n' "$tool_path" >&2
    return 1
  fi
  printf '%s\n' "$tool_path"
}

cargo_path=$(resolve_pinned_tool cargo)
rustc_path=$(resolve_pinned_tool rustc)
rustdoc_path=$(resolve_pinned_tool rustdoc)
for tool_path in "$cargo_path" "$rustc_path" "$rustdoc_path"; do
  tool_release=$("$tool_path" --version --verbose | awk '$1 == "release:" { release = $2 } END { print release }')
  if [[ "$tool_release" != "$pinned_toolchain" ]]; then
    printf 'rust-risk gate resolved an unexpected tool release: %s expected=%s actual=%s\n' \
      "$tool_path" "$pinned_toolchain" "${tool_release:-<missing>}" >&2
    exit 1
  fi
done
host_target=$("$rustc_path" --print host-tuple)
bootstrap_root=$(mktemp -d -t cohesix-rust-risk.XXXXXX)
target_dir="${bootstrap_root}/target"
private_cargo_home="${bootstrap_root}/cargo-home"
cleanup() {
  rm -rf "$bootstrap_root"
}
trap cleanup EXIT HUP INT TERM
mkdir -p "${private_cargo_home}/registry"
for cache_kind in cache index; do
  archive_path="${cargo_home}/registry/${cache_kind}"
  if [[ ! -d "$archive_path" || -L "$archive_path" ]]; then
    printf 'rust-risk gate requires a regular canonical Cargo registry %s directory: %s\n' \
      "$cache_kind" "$archive_path" >&2
    exit 1
  fi
  ln -s "$archive_path" "${private_cargo_home}/registry/${cache_kind}"
done
scanner="${target_dir}/${host_target}/debug/rust-risk-audit"

env \
  CARGO_HOME="$private_cargo_home" \
  CARGO_NET_OFFLINE=true \
  RUSTC="$rustc_path" \
  RUSTDOC="$rustdoc_path" \
  RUSTC_WRAPPER="${repo_root}/scripts/rustc-wrapper.sh" \
  RUSTC_WORKSPACE_WRAPPER= \
  RUSTFLAGS= \
  CARGO_ENCODED_RUSTFLAGS= \
  "$cargo_path" build --quiet --locked \
    --manifest-path "${repo_root}/tools/rust-risk-audit/Cargo.toml" \
    --target "$host_target" \
    --target-dir "$target_dir"

if [[ -L "${private_cargo_home}/registry/src" ]]; then
  printf 'rust-risk private Cargo home exposed an external extracted source tree\n' >&2
  exit 1
fi

if [[ ! -x "$scanner" ]]; then
  printf 'rust-risk scanner was not built at expected host path: %s\n' "$scanner" >&2
  exit 1
fi

if [[ $# -eq 0 ]]; then
  set -- --root "$repo_root" --baseline docs/audit/rust_risk_baseline.toml
fi
"$scanner" "$@"
