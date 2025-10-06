#!/usr/bin/env sh
# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v1.59
# Author: Lukas Bower
# Date Modified: 2029-11-19

# This script fetches and builds the Cohesix project, including seL4 and other dependencies.

# Ensure we have a POSIX-compatible environment even when invoked via zsh
if [ -n "${ZSH_VERSION:-}" ]; then
  emulate -L sh
  setopt sh_word_split
fi

HOST_ARCH="$(uname -m)"
HOST_OS="$(uname -s)"
SUDO=""
: "${ENABLE_PYTHON:=0}"
PYTHON_HELPER_NOTICE=0

python_skip_log() {
  if [ "${PYTHON_HELPER_NOTICE}" -eq 0 ]; then
    PYTHON_HELPER_NOTICE=1
    if command -v log >/dev/null 2>&1; then
      log "⏭️ python3 unavailable; using POSIX fallbacks for helper utilities"
    else
      printf '%s\n' "⏭️ python3 unavailable; using POSIX fallbacks for helper utilities" >&2
    fi
  fi
}

CROSS_GCC=""
for candidate in aarch64-linux-gnu-gcc aarch64-unknown-linux-gnu-gcc; do
  if command -v "$candidate" >/dev/null 2>&1; then
    CROSS_GCC="$candidate"
    break
  fi
done

MUSL_CC=""
for candidate in aarch64-linux-musl-gcc aarch64-unknown-linux-musl-gcc; do
  if command -v "$candidate" >/dev/null 2>&1; then
    MUSL_CC="$candidate"
    break
  fi
done
MUSL_CC_FALLBACK=0
if [ -z "$MUSL_CC" ] && [ -n "$CROSS_GCC" ]; then
  MUSL_CC="$CROSS_GCC"
  MUSL_CC_FALLBACK=1
fi

MUSL_AR=""
for candidate in aarch64-linux-musl-ar aarch64-unknown-linux-musl-ar; do
  if command -v "$candidate" >/dev/null 2>&1; then
    MUSL_AR="$candidate"
    break
  fi
done
if [ -z "$MUSL_AR" ] && command -v aarch64-linux-gnu-ar >/dev/null 2>&1; then
  MUSL_AR="aarch64-linux-gnu-ar"
fi

MUSL_RANLIB=""
for candidate in aarch64-linux-musl-ranlib aarch64-unknown-linux-musl-ranlib; do
  if command -v "$candidate" >/dev/null 2>&1; then
    MUSL_RANLIB="$candidate"
    break
  fi
done
if [ -z "$MUSL_RANLIB" ] && command -v aarch64-linux-gnu-ranlib >/dev/null 2>&1; then
  MUSL_RANLIB="aarch64-linux-gnu-ranlib"
fi

if { [ "$HOST_ARCH" = "aarch64" ] || [ "$HOST_ARCH" = "arm64" ]; } && [ -z "$CROSS_GCC" ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO=sudo
  else
    SUDO=""
  fi
fi

if [ -z "$SUDO" ] && [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then
  SUDO=sudo
fi

version_ge() {
  ver_a="$1"
  ver_b="$2"
  if [ -z "$ver_a" ] || [ -z "$ver_b" ]; then
    return 1
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$ver_a" "$ver_b" <<'PY'
import sys
from itertools import zip_longest

def parse(value):
    parts = []
    for token in value.replace('-', '.').split('.'):
        if not token:
            continue
        try:
            parts.append(int(token))
        except ValueError:
            parts.append(0)
    return parts

a = parse(sys.argv[1])
b = parse(sys.argv[2])
for left, right in zip_longest(a, b, fillvalue=0):
    if left > right:
        sys.exit(0)
    if left < right:
        sys.exit(1)
sys.exit(0)
PY
    return $?
  fi
  python_skip_log
  awk -v a="$ver_a" -v b="$ver_b" 'BEGIN {
    n = split(a, aa, ".")
    m = split(b, bb, ".")
    len = (n > m ? n : m)
    for (i = 1; i <= len; i++) {
      x = (i in aa ? aa[i] + 0 : 0)
      y = (i in bb ? bb[i] + 0 : 0)
      if (x > y) { exit 0 }
      if (x < y) { exit 1 }
    }
    exit 0
  }'
}

resolve_path() {
  target="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$target" <<'PY'
import os
import sys

print(os.path.realpath(sys.argv[1]))
PY
    return
  fi
  python_skip_log
  if command -v readlink >/dev/null 2>&1; then
    readlink -f "$target" && return
  fi
  (
    cd "$(dirname "$target")" 2>/dev/null || exit 1
    printf '%s\n' "$(pwd -P)/$(basename "$target")"
  )
}

prepend_ld_library_path() {
  new_path="$1"
  case "${new_path}" in
    "") return ;;
  esac
  case ":${LD_LIBRARY_PATH:-}:" in
    *:"${new_path}":*) ;;
    *)
      if [ -n "${LD_LIBRARY_PATH:-}" ]; then
        LD_LIBRARY_PATH="${new_path}:${LD_LIBRARY_PATH}"
      else
        LD_LIBRARY_PATH="${new_path}"
      fi
      export LD_LIBRARY_PATH
      ;;
  esac
}

add_library_path_list() {
  list="$1"
  if [ -z "$list" ]; then
    return
  fi
  old_ifs=$IFS
  IFS=':'
  for dir in $list; do
    prepend_ld_library_path "$dir"
  done
  IFS=$old_ifs
}

create_temp_dir() {
  template="${1:-cohesix}"
  if [ "$HOST_OS" = "Darwin" ]; then
    mktemp -d -t "$template"
  else
    mktemp -d -t "${template}.XXXXXX" 2>/dev/null || mktemp -d
  fi
}

resolve_tool_path() {
  candidate="$1"
  if [ -z "$candidate" ]; then
    return 1
  fi
  if [ -x "$candidate" ]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  command -v "$candidate" 2>/dev/null
}

select_linker() {
  # Allow callers to override via LD_LLD or COHESIX_LINKER
  if [ -n "${COHESIX_LINKER:-}" ] && [ -n "${COHESIX_LINKER_FLAVOR:-}" ]; then
    return 0
  fi

  if resolved=$(resolve_tool_path "${LD_LLD:-}"); then
    COHESIX_LINKER="$resolved"
    COHESIX_LINKER_FLAVOR="ld.lld"
    return 0
  fi

  if resolved=$(resolve_tool_path ld.lld); then
    COHESIX_LINKER="$resolved"
    COHESIX_LINKER_FLAVOR="ld.lld"
    return 0
  fi

  if resolved=$(resolve_tool_path ld64.lld); then
    COHESIX_LINKER="$resolved"
    COHESIX_LINKER_FLAVOR="ld.lld"
    return 0
  fi

  if resolved=$(resolve_tool_path aarch64-linux-gnu-ld); then
    COHESIX_LINKER="$resolved"
    COHESIX_LINKER_FLAVOR="ld"
    return 0
  fi

  if resolved=$(resolve_tool_path aarch64-linux-gnu-gcc); then
    COHESIX_LINKER="$resolved"
    COHESIX_LINKER_FLAVOR="gcc"
    return 0
  fi

  return 1
}

detect_cpu_count() {
  if command -v nproc >/dev/null 2>&1; then
    nproc
    return
  fi
  if [ "$HOST_OS" = "Darwin" ] && command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.logicalcpu
    return
  fi
  getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1
}
# Resolve the repository root relative to this script when ROOT is unset
if [ -z "${ROOT:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
  for candidate in "$SCRIPT_DIR" "$SCRIPT_DIR/.." "$SCRIPT_DIR/../.."; do
    if [ -d "$candidate/.git" ] || [ -f "$candidate/.git" ] || [ -d "$candidate/third_party/seL4" ]; then
      ROOT="$(cd "$candidate" && pwd -P)"
      break
    fi
  done
  : "${ROOT:=$SCRIPT_DIR}"
else
  ROOT="$(cd "$ROOT" && pwd -P)"
fi
export ROOT

LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
set -eu
if (set -o pipefail) 2>/dev/null; then
  set -o pipefail
fi
export MEMCHR_DISABLE_RUNTIME_CPU_FEATURE_DETECTION=1
export CUDA_HOME="${CUDA_HOME:-/usr}"
export CUDA_INCLUDE_DIR="${CUDA_INCLUDE_DIR:-$CUDA_HOME/include}"

if [ -z "${CUDA_LIBRARY_PATH:-}" ]; then
  case "$HOST_OS" in
    Darwin)
      for candidate in \
        "/usr/local/cuda/lib" \
        "/usr/local/lib" \
        "/opt/homebrew/lib"; do
        if [ -d "$candidate" ]; then
          CUDA_LIBRARY_PATH="$candidate"
          break
        fi
      done
      ;;
    *)
      case "$HOST_ARCH" in
        x86_64|amd64)
          DEFAULT_CUDA_LIB="/usr/lib/x86_64-linux-gnu"
          ;;
        aarch64|arm64)
          DEFAULT_CUDA_LIB="/usr/lib/aarch64-linux-gnu"
          ;;
        *)
          DEFAULT_CUDA_LIB=""
          ;;
      esac
      if [ -n "${DEFAULT_CUDA_LIB:-}" ] && [ -d "$DEFAULT_CUDA_LIB" ]; then
        CUDA_LIBRARY_PATH="$DEFAULT_CUDA_LIB"
      fi
      ;;
  esac
  if [ -n "${CUDA_LIBRARY_PATH:-}" ]; then
    export CUDA_LIBRARY_PATH
  fi
fi

if [ -n "${CUDA_LIBRARY_PATH:-}" ]; then
  add_library_path_list "$CUDA_LIBRARY_PATH"
fi

if [ -d "$CUDA_HOME/bin" ]; then
  export PATH="$CUDA_HOME/bin:$PATH"
fi
WORKSPACE="${WORKSPACE:-$ROOT/third_party/seL4}"

cd "$ROOT"

LOG_FILE="$LOG_DIR/build_$(date +%Y%m%d_%H%M%S).log"
SUMMARY_ERRORS="$LOG_DIR/summary_errors.log"
SUMMARY_TEST_FAILS="$LOG_DIR/summary_test_failures.log"
: > "$LOG_DIR/libsel4_link_and_boot_trace.md"
TRACE_LOG="$LOG_DIR/libsel4_link_and_boot_trace.md"
{
  echo "// CLASSIFICATION: COMMUNITY"
  echo "// Filename: libsel4_link_and_boot_trace.md v0.1"
  echo "// Author: Lukas Bower"
  echo "// Date Modified: $(date +%Y-%m-%d)"
  echo
} > "$TRACE_LOG"
: > "$SUMMARY_ERRORS"
: > "$SUMMARY_TEST_FAILS"
exec 3>&1  # Save original stdout
LOG_PIPE=""
TEE_PID=""
if command -v mkfifo >/dev/null 2>&1; then
  LOG_PIPE="$LOG_DIR/build_pipe_$$"
  if mkfifo "$LOG_PIPE"; then
    tee -a "$LOG_FILE" <"$LOG_PIPE" >&3 &
    TEE_PID=$!
    exec >"$LOG_PIPE" 2>&1
  else
    LOG_PIPE=""
  fi
fi
if [ -z "$LOG_PIPE" ]; then
  exec >"$LOG_FILE" 2>&1
fi

CLEANED_UP=0
cleanup_logging() {
  if [ "$CLEANED_UP" -eq 1 ]; then
    return
  fi
  CLEANED_UP=1
  exec 1>&3 2>&3
  if [ -n "$LOG_PIPE" ]; then
    if [ -n "$TEE_PID" ]; then
      wait "$TEE_PID" 2>/dev/null || true
    fi
    rm -f "$LOG_PIPE"
  fi
}

handle_failure() {
  cleanup_logging
  echo "❌ Build failed." >&3
  if [ -f "$LOG_FILE" ]; then
    echo "Last 40 log lines:" >&3
    tail -n 40 "$LOG_FILE" >&3
  fi
}

trap 'handle_failure' ERR
trap 'cleanup_logging' EXIT INT TERM

log() { echo "[$(date +%H:%M:%S)] $1" | tee -a "$LOG_FILE" >&3; }

log "🛠️ [Build Start] $(date)"
log "🚀 Using existing repository at $ROOT"

configure_libclang() {
  if [ -z "${LIBCLANG_PATH:-}" ]; then
    for candidate in \
      "/Library/Developer/CommandLineTools/usr/lib" \
      "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib"; do
      if [ -f "$candidate/libclang.dylib" ]; then
        LIBCLANG_PATH="$candidate"
        break
      fi
    done
  fi
  if [ -n "${LIBCLANG_PATH:-}" ] && [ -d "$LIBCLANG_PATH" ]; then
    export LIBCLANG_PATH
    case ":${DYLD_LIBRARY_PATH:-}:" in
      *:"${LIBCLANG_PATH}:"*) ;;
      *)
        export DYLD_LIBRARY_PATH="${LIBCLANG_PATH}${DYLD_LIBRARY_PATH:+:${DYLD_LIBRARY_PATH}}"
        ;;
    esac
    log "✅ Using libclang from $LIBCLANG_PATH"
  else
    log "⚠️ libclang.dylib not found; bindgen builds may fail"
  fi
}

prepare_sel4_target_spec() {
  if [ -n "${SEL4_TARGET_SPEC_SANITIZED:-}" ] && [ -f "$SEL4_TARGET_SPEC_SANITIZED" ]; then
    return
  fi
  local src="$ROOT/workspace/cohesix_root/sel4-aarch64.json"
  if [ ! -f "$src" ]; then
    log "❌ Missing target specification at $src"
    exit 1
  fi

  local target_dir
  target_dir="${COHESIX_TRACE_TMP:-${TMPDIR:-}}"
  if [ -z "$target_dir" ] || [ ! -d "$target_dir" ]; then
    target_dir="$(create_temp_dir cohesix-target)"
  fi
  mkdir -p "$target_dir"
  local sanitized="$target_dir/sel4-aarch64.json"

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$src" "$sanitized" <<'PY'
import json
import sys

src_path, dst_path = sys.argv[1:3]
with open(src_path, 'r', encoding='utf-8') as src_file:
    data = json.load(src_file)

value = data.get("target-pointer-width")
if isinstance(value, str):
    try:
        data["target-pointer-width"] = int(value, 0)
    except ValueError:
        data["target-pointer-width"] = 64

with open(dst_path, 'w', encoding='utf-8') as dst_file:
    json.dump(data, dst_file, indent=2)
    dst_file.write("\n")
PY
  else
    python_skip_log
    sed 's/"target-pointer-width"[[:space:]]*:[[:space:]]*"\{0,1\}64"/"target-pointer-width": 64/' "$src" >"$sanitized"
  fi

  export SEL4_TARGET_SPEC_SRC="$src"
  export SEL4_TARGET_SPEC_SANITIZED="$sanitized"
  log "✅ Prepared nightly target spec at $SEL4_TARGET_SPEC_SANITIZED"
}

# Phase-specific minimal builds
PHASE=""
for arg in "$@"; do
  case $arg in
    --phase=*)
      PHASE="${arg#*=}"
      ;;
    --enable-python)
      ENABLE_PYTHON=1
      ;;
  esac
done


SEL4_LIB_DIR="${SEL4_LIB_DIR:-$ROOT/third_party/seL4/lib}"
export SEL4_ARCH="${SEL4_ARCH:-aarch64}"

validate_sel4_artifacts() {
  sel4_root="$ROOT/third_party/seL4"
  if [ ! -d "$sel4_root" ]; then
    log "❌ Missing seL4 workspace at $sel4_root"
    log "➡️ Run '$ROOT/third_party/seL4/fetch_sel4.sh --non-interactive' before invoking cohesix_fetch_build.sh."
    exit 1
  fi

  if [ ! -d "$SEL4_LIB_DIR" ]; then
    log "❌ Expected seL4 library directory at $SEL4_LIB_DIR"
    log "➡️ Run '$ROOT/third_party/seL4/fetch_sel4.sh --non-interactive' to populate lib/, include/, and workspace artefacts."
    exit 1
  fi

  if [ ! -f "$SEL4_LIB_DIR/libsel4.a" ]; then
    log "❌ libsel4.a not found under $SEL4_LIB_DIR"
    log "➡️ Populate third_party/seL4 by running '$ROOT/third_party/seL4/fetch_sel4.sh --non-interactive' prior to rerunning."
    exit 1
  fi

  if [ ! -d "$sel4_root/include" ]; then
    log "❌ seL4 headers missing under $sel4_root/include"
    log "➡️ Re-run '$ROOT/third_party/seL4/fetch_sel4.sh --non-interactive' to install required headers."
    exit 1
  fi

  arch_header="$sel4_root/include/libsel4/sel4_arch/sel4/sel4_arch/${SEL4_ARCH}/simple_types.h"
  if [ ! -f "$arch_header" ]; then
    log "❌ Required header missing: $arch_header"
    log "➡️ Ensure Cohesix fetch has installed the upstream seL4 headers for ${SEL4_ARCH}."
    exit 1
  fi
  if ! grep -q 'SEL4_WORD_IS_UINT64' "$arch_header" 2>/dev/null; then
    log "❌ Unexpected contents in $arch_header (expected upstream macros)."
    exit 1
  fi

  log "✅ Offline seL4 artefacts detected under third_party/seL4"
}

validate_sel4_artifacts

prepare_sel4_target_spec

configure_musl_toolchain() {
  if [ -z "$MUSL_CC" ]; then
    echo "❌ musl cross compiler not detected" >&2
    exit 1
  fi
  if ! resolved_cc=$(resolve_tool_path "$MUSL_CC"); then
    echo "❌ Unable to resolve musl cross compiler for target aarch64-unknown-linux-musl" >&2
    exit 1
  fi
  export CC_aarch64_unknown_linux_musl="$resolved_cc"
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_CC="$resolved_cc"
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$resolved_cc"
  if [ "${MUSL_CC_FALLBACK}" -eq 1 ]; then
    log "⚠️ aarch64-linux-musl-gcc unavailable; reusing ${resolved_cc} for aarch64-unknown-linux-musl C builds"
  else
    log "✅ Using musl cross compiler at ${resolved_cc}"
  fi

  if [ -n "$MUSL_AR" ]; then
    if resolved_ar=$(resolve_tool_path "$MUSL_AR"); then
      export AR_aarch64_unknown_linux_musl="$resolved_ar"
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_AR="$resolved_ar"
    fi
  fi

  if [ -n "$MUSL_RANLIB" ]; then
    if resolved_ranlib=$(resolve_tool_path "$MUSL_RANLIB"); then
      export RANLIB_aarch64_unknown_linux_musl="$resolved_ranlib"
    fi
  fi
}

configure_musl_toolchain

if ! select_linker; then
  echo "❌ Unable to locate a suitable linker (expected ld.lld, ld64.lld, aarch64-linux-gnu-ld, or aarch64-linux-gnu-gcc)." >&2
  echo "➡️ Install LLVM's lld or export LD_LLD=/path/to/ld.lld before rerunning." >&2
  exit 1
fi

export COHESIX_LINKER
export COHESIX_LINKER_FLAVOR

if [ "${COHESIX_LINKER_FLAVOR}" = "ld.lld" ]; then
  log "🔗 Using ld.lld at ${COHESIX_LINKER}"
else
  log "⚠️ ld.lld unavailable; using fallback linker ${COHESIX_LINKER} (flavor: ${COHESIX_LINKER_FLAVOR})"
fi

COHESIX_RUST_LINKER_FLAGS="-C linker=${COHESIX_LINKER} -C linker-flavor=${COHESIX_LINKER_FLAVOR}"
COHESIX_RUST_GC_FLAGS="-C link-arg=--gc-sections -C link-arg=--eh-frame-hdr"

if [ -n "$PHASE" ]; then
  if [ "$PHASE" = "4" ]; then
    log "🔁 Phase 4: Reusing previous build outputs; continuing with staging and packaging"
    cd "$ROOT"
  else
    configure_libclang
    cd "$ROOT/workspace"
    CROSS_RUSTFLAGS="${COHESIX_RUST_LINKER_FLAGS} ${COHESIX_RUST_GC_FLAGS} -C link-arg=-L${SEL4_LIB_DIR}"
    if [ "$PHASE" = "1" ]; then
      log "🔨 Phase 1: Building host crates for musl userland"
      cargo build --release --workspace \
        --exclude 'cohesix_root' \
        --exclude 'sel4-sys-extern-wrapper' \
        --target aarch64-unknown-linux-musl
      log "⏭️ Cross-target tests compiled only (aarch64-unknown-linux-musl)"
      cargo test --release --workspace \
        --exclude 'cohesix_root' \
        --exclude 'sel4-sys-extern-wrapper' \
        --target aarch64-unknown-linux-musl \
        --no-run
      log "✅ Phase 1 build succeeded (tests compiled for cross-target)"
    elif [ "$PHASE" = "2" ]; then
      log "🔨 Phase 2: Building sel4-sys-extern-wrapper"
      export CFLAGS="-I${ROOT}/workspace/sel4-sys-extern-wrapper/out"
      export LDFLAGS="-L${SEL4_LIB_DIR}"
      export RUSTFLAGS="-C panic=abort -L${SEL4_LIB_DIR} ${CROSS_RUSTFLAGS}"
      # Ensure Rust source is available for build-std
      rustup component add rust-src --toolchain nightly || true
      cargo +nightly build -p sel4-sys-extern-wrapper --release \
        --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" \
        -Z build-std=core,alloc,compiler_builtins \
        -Z build-std-features=compiler-builtins-mem
      WRAPPER_RLIB=$(find target/sel4-aarch64/release/deps -maxdepth 1 -name 'libsel4_sys_extern_wrapper*.rlib' -print -quit 2>/dev/null)
      if [ -n "$WRAPPER_RLIB" ]; then
        log "✅ Phase 2 build succeeded: $(basename "$WRAPPER_RLIB")"
      else
        echo "❌ wrapper artifact missing" >&2
        exit 1
      fi
    elif [ "$PHASE" = "3" ]; then
      log "🔨 Phase 3: Building cohesix_root under nightly"
      export LDFLAGS="-L${SEL4_LIB_DIR}"
      export RUSTFLAGS="-C panic=abort -L${SEL4_LIB_DIR} ${CROSS_RUSTFLAGS}"
      # Ensure Rust source is available for build-std
      rustup component add rust-src --toolchain nightly || true
      cargo +nightly build -p cohesix_root --release \
        --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" \
        -Z build-std=core,alloc,compiler_builtins \
        -Z build-std-features=compiler-builtins-mem
      ROOT_ARTIFACT=$(find target/sel4-aarch64/release/deps -maxdepth 1 -name 'libcohesix_root*.rlib' -print -quit 2>/dev/null)
      if [ -z "$ROOT_ARTIFACT" ]; then
        BIN_ARTIFACT=$(find target/sel4-aarch64/release -maxdepth 1 -type f -name 'cohesix_root' -print -quit 2>/dev/null)
      else
        BIN_ARTIFACT=""
      fi
      if [ -n "$ROOT_ARTIFACT" ] || [ -n "$BIN_ARTIFACT" ]; then
        log "✅ Phase 3 build succeeded: $(basename "${ROOT_ARTIFACT:-$BIN_ARTIFACT}")"
      else
        echo "❌ cohesix_root artifact missing" >&2
        exit 1
      fi
    else
      echo "❌ Invalid phase: $PHASE" >&2
      exit 1
    fi
    exit 0
  fi
fi
STAGE_DIR="$ROOT/out"
GO_HELPERS_DIR="$ROOT/out/go_helpers"
mkdir -p "$STAGE_DIR"
mkdir -p "$GO_HELPERS_DIR"
cd "$STAGE_DIR"
mkdir -p bin usr/bin usr/cli usr/share/man/man1 usr/share/man/man8 \
         etc srv mnt/data tmp dev proc roles home/cohesix boot init
cp "$ROOT/workspace/cohesix/src/kernel/init.rc" "$STAGE_DIR/srv/init.rc"
chmod +x "$STAGE_DIR/srv/init.rc"
log "✅ Created Cohesix FS structure"
# 🗂 Prepare /srv namespace for tests (clean and set role)
log "🗂 Preparing staged /srv namespace under $STAGE_DIR for tests..."
echo "DroneWorker" > "$STAGE_DIR/srv/cohrole"
# Always create a robust config/config.yaml and stage it
log "📂 Ensuring configuration file exists..."
CONFIG_PATH="$ROOT/config/config.yaml"
mkdir -p "$(dirname "$CONFIG_PATH")"
cat > "$CONFIG_PATH" <<EOF
# CLASSIFICATION: COMMUNITY
# Filename: config.yaml
# Author: Lukas Bower
# Date Modified: $(date +%Y-%m-%d)
role: QueenPrimary
network:
  enabled: true
  interfaces:
    - eth0
logging:
  level: info
EOF
log "✅ config.yaml created at $CONFIG_PATH"

export SEL4_LIB_DIR="${SEL4_LIB_DIR:-$ROOT/third_party/seL4/lib}"
SEL4_INCLUDE_PATH="$ROOT/third_party/seL4/include"
if [ -d "$SEL4_INCLUDE_PATH" ]; then
  export SEL4_INCLUDE="${SEL4_INCLUDE:-$(resolve_path "$SEL4_INCLUDE_PATH")}"
fi
# Delay use of seL4-specific RUSTFLAGS until cross compilation phase
CROSS_RUSTFLAGS="${COHESIX_RUST_LINKER_FLAGS} ${COHESIX_RUST_GC_FLAGS} -C link-arg=-L${SEL4_LIB_DIR}"
if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
  . "$ROOT/scripts/load_arch_config.sh"
else
  echo "❌ Missing: $ROOT/scripts/load_arch_config.sh" >&2
  exit 1
fi

COH_ARCH="$COHESIX_ARCH"
log "Architecture: $COH_ARCH (seL4+ELF only, no UEFI/PE32 build)"


# Toolchain sanity checks
if ! command -v rustup >/dev/null 2>&1; then
  echo "❌ rustup not found. Install Rust toolchains before running" >&2
  exit 1
fi
if ! rustup component list --toolchain nightly | grep -q 'rust-src (installed)'; then
  echo "🔧 Installing missing rust-src component for nightly" >&2
  rustup component add rust-src --toolchain nightly
fi
if ! rustup target list --installed --toolchain nightly | grep -q "^aarch64-unknown-none$"; then
  echo "🔧 Installing missing Rust target aarch64-unknown-none" >&2
  rustup target add --toolchain nightly aarch64-unknown-none
fi
cross_gcc_msg=${CROSS_GCC:-}
if [ -z "$cross_gcc_msg" ]; then
  echo "❌ aarch64 cross GCC missing (expected aarch64-linux-gnu-gcc or aarch64-unknown-linux-gnu-gcc)" >&2
  exit 1
fi
PROTOC_BIN="${PROTOC:-}"
if [ -n "$PROTOC_BIN" ]; then
  if [ ! -x "$PROTOC_BIN" ]; then
    echo "❌ PROTOC environment variable points to non-executable: $PROTOC_BIN" >&2
    exit 1
  fi
else
  if command -v protoc >/dev/null 2>&1; then
    PROTOC_BIN="$(command -v protoc)"
  else
    echo "❌ protoc not found. Install protobuf compiler (e.g. 'brew install protobuf') or export PROTOC=/path/to/protoc" >&2
    echo "➡️ Proto compilation required for GUI control-plane gRPC contracts (Solution Architecture §7, Backlog E4-F10)." >&2
    exit 1
  fi
fi
export PROTOC="$PROTOC_BIN"
log "✅ Using protoc at $PROTOC_BIN"
"$COHESIX_LINKER" --version >&3 || true

configure_libclang

log "\ud83d\udcc5 Fetching Cargo dependencies..."
cd "$ROOT/workspace"
if [ -z "${CARGO_NET_OFFLINE+x}" ]; then
  export CARGO_NET_OFFLINE=true
fi
if ! CARGO_BUILD_TARGET="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" cargo +nightly fetch; then
  if [ "${CARGO_NET_OFFLINE}" = "true" ]; then
    log "⚠️ Offline fetch failed; retrying with network access"
    CARGO_NET_OFFLINE=false CARGO_BUILD_TARGET="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" cargo +nightly fetch
  else
    false
  fi
fi
log "\u2705 Cargo dependencies fetched"

# Kernel must run in production mode; disable seL4 self-tests
export CONFIG_BUILD_KERNEL_TESTS=n
KERNEL_TEST_FLAG=OFF

# CUDA detection and environment setup
log "🚀 Starting CUDA check..."
CUDA_HOME="${CUDA_HOME:-}"
if [ -z "$CUDA_HOME" ]; then
  if command -v nvcc >/dev/null 2>&1; then
    NVCC_PATH="$(command -v nvcc)"
    CUDA_HOME="$(dirname "$(dirname "$NVCC_PATH")")"
  elif [ -d /usr/local/cuda ]; then
    CUDA_HOME="/usr/local/cuda"
  else
    CUDA_FALLBACK_SCAN=""
    for candidate in /usr/local/cuda-*arm64 /usr/local/cuda-*; do
      case "$candidate" in
        *\**|*\?*) continue ;;
      esac
      if [ -d "$candidate" ]; then
        if [ -z "$CUDA_HOME" ]; then
          CUDA_HOME="$candidate"
        fi
        if [ -n "$CUDA_FALLBACK_SCAN" ]; then
          CUDA_FALLBACK_SCAN="$CUDA_FALLBACK_SCAN $candidate"
        else
          CUDA_FALLBACK_SCAN="$candidate"
        fi
      fi
    done
    # Manual override for environments where cuda.h is in /usr/include but no nvcc exists
    if [ "$CUDA_HOME" = "/usr" ] && [ -f "/usr/include/cuda.h" ]; then
      export CUDA_INCLUDE_DIR="/usr/include"
      CUDA_FALLBACK_LIB=""
      if [ "$HOST_OS" = "Darwin" ]; then
        for candidate in \
          "/usr/local/cuda/lib" \
          "/usr/local/lib" \
          "/opt/homebrew/lib"; do
          if [ -d "$candidate" ]; then
            CUDA_FALLBACK_LIB="$candidate"
            break
          fi
        done
      else
        case "$HOST_ARCH" in
          aarch64|arm64)
            CUDA_FALLBACK_LIB="/usr/lib/aarch64-linux-gnu"
            ;;
          x86_64|amd64)
            CUDA_FALLBACK_LIB="/usr/lib/x86_64-linux-gnu"
            ;;
          *)
            CUDA_FALLBACK_LIB=""
            ;;
        esac
      fi
      if [ -n "$CUDA_FALLBACK_LIB" ] && [ -d "$CUDA_FALLBACK_LIB" ]; then
        if [ -z "${CUDA_LIBRARY_PATH:-}" ]; then
          export CUDA_LIBRARY_PATH="$CUDA_FALLBACK_LIB"
        fi
        add_library_path_list "$CUDA_FALLBACK_LIB"
      fi
      log "✅ Manually set CUDA paths for cust_raw: CUDA_HOME=$CUDA_HOME"
    fi
    if [ -z "$CUDA_HOME" ] || [ ! -d "$CUDA_HOME" ]; then
      CUDA_HOME="/usr"
    fi
  fi
fi

# Log CUDA fallback paths
if [ -z "${CUDA_FALLBACK_SCAN:-}" ]; then
  log "CUDA fallback paths tried: none found"
else
  log "CUDA fallback paths tried: ${CUDA_FALLBACK_SCAN}"
fi

export CUDA_HOME
if [ -d "$CUDA_HOME/bin" ]; then
  export PATH="$CUDA_HOME/bin:$PATH"
fi
if [ -d "$CUDA_HOME/lib64" ]; then
  prepend_ld_library_path "$CUDA_HOME/lib64"
elif [ -d "$CUDA_HOME/lib" ]; then
  prepend_ld_library_path "$CUDA_HOME/lib"
fi
# Add robust library path fallback for common distros
if [ "$HOST_OS" = "Linux" ] && [ -d "/usr/lib/x86_64-linux-gnu" ]; then
  prepend_ld_library_path "/usr/lib/x86_64-linux-gnu"
fi
if [ "$HOST_OS" = "Darwin" ]; then
  for candidate in "/usr/local/lib" "/opt/homebrew/lib"; do
    if [ -d "$candidate" ]; then
      prepend_ld_library_path "$candidate"
    fi
  done
fi
export CUDA_LIBRARY_PATH="${CUDA_LIBRARY_PATH:-${LD_LIBRARY_PATH:-}}"

if [ -f "$CUDA_HOME/include/cuda.h" ]; then
  log "✅ Found cuda.h in $CUDA_HOME/include"
elif [ "$HOST_OS" = "Darwin" ]; then
  log "⚠️ cuda.h not found locally; macOS builds rely on remote CUDA via Secure9P"
else
  echo "❌ cuda.h not found in $CUDA_HOME/include. Check CUDA installation." >&2
  exit 1
fi

if [ -n "$CUDA_HOME" ] && [ -f "$CUDA_HOME/bin/nvcc" ]; then
  log "CUDA detected at $CUDA_HOME"
  if nvcc --version >/tmp/nvcc_check.log 2>&1; then
    log "nvcc OK: $(grep -m1 release /tmp/nvcc_check.log)"
  else
    log "⚠️ nvcc failed: $(cat /tmp/nvcc_check.log)"
  fi
  if command -v nvidia-smi >/dev/null 2>&1; then
    if nvidia-smi >/tmp/nvidia_smi.log 2>&1; then
      log "nvidia-smi OK: $(grep -m1 'Driver Version' /tmp/nvidia_smi.log)"
    else
      log "⚠️ nvidia-smi failed: $(cat /tmp/nvidia_smi.log)"
    fi
  else
    log "⚠️ nvidia-smi not found"
  fi
  log "✅ CUDA OK"
else
  log "⚠️ CUDA toolkit not detected. nvcc not found or invalid CUDA_HOME: $CUDA_HOME"
fi

if [ "$COH_ARCH" = "aarch64" ] && command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed --toolchain nightly | grep -q '^aarch64-unknown-none$'; then
    rustup target add --toolchain nightly aarch64-unknown-none
    log "✅ Rust target aarch64-unknown-none installed"
  fi
fi

if [ "$COH_ARCH" != "x86_64" ]; then
  CROSS_X86="x86_64-linux-gnu-"
else
  CROSS_X86=""
fi

CMAKE_VER=$(cmake --version 2>/dev/null | head -n1 | awk '{print $3}')
if [ -z "$CMAKE_VER" ] || ! version_ge "$CMAKE_VER" "3.20"; then
  log "cmake ${CMAKE_VER:-not installed} too old; installing newer release binary"
  TMP_CMAKE="$(create_temp_dir cohesix_cmake)"
  CMAKE_V=3.28.1
  ARCH=$(uname -m)
  case "$HOST_OS" in
    Darwin)
      CMAKE_PKG="cmake-${CMAKE_V}-macos-universal.tar.gz"
      CMAKE_CONTENT_DIR="cmake-${CMAKE_V}-macos-universal/CMake.app/Contents"
      ;;
    *)
      case "$ARCH" in
        aarch64|arm64)
          CMAKE_PKG="cmake-${CMAKE_V}-linux-aarch64.tar.gz"
          ;;
        *)
          CMAKE_PKG="cmake-${CMAKE_V}-linux-x86_64.tar.gz"
          ;;
      esac
      CMAKE_CONTENT_DIR="cmake-${CMAKE_V}-linux-*"
      ;;
  esac
  wget -q "https://github.com/Kitware/CMake/releases/download/v${CMAKE_V}/${CMAKE_PKG}" -O "$TMP_CMAKE/$CMAKE_PKG"
  tar -xf "$TMP_CMAKE/$CMAKE_PKG" -C "$TMP_CMAKE"
  if [ "$HOST_OS" = "Darwin" ]; then
    if [ -d "$TMP_CMAKE/$CMAKE_CONTENT_DIR/bin" ]; then
      $SUDO cp -R "$TMP_CMAKE/$CMAKE_CONTENT_DIR/bin" /usr/local/
    fi
    if [ -d "$TMP_CMAKE/$CMAKE_CONTENT_DIR/share" ]; then
      $SUDO cp -R "$TMP_CMAKE/$CMAKE_CONTENT_DIR/share" /usr/local/
    fi
  else
    $SUDO cp -r "$TMP_CMAKE"/${CMAKE_CONTENT_DIR}/{bin,share} /usr/local/
  fi
  rm -rf "$TMP_CMAKE"
  hash -r
fi

# Ensure init.conf exists with defaults
INIT_CONF="$STAGE_DIR/etc/init.conf"
if [ ! -f "$INIT_CONF" ]; then
  cat > "$INIT_CONF" <<EOF
# CLASSIFICATION: COMMUNITY
# Filename: init.conf v0.1
# Author: Lukas Bower
# Date Modified: $(date +%Y-%m-%d)
ROLE=DroneWorker
GPU=1
TRACE=on
EOF
  log "✅ Created default init.conf"
else
  log "✅ Existing init.conf found"
fi

# Ensure plan9.ns is staged early, fail fast if missing
ensure_plan9_ns() {
  ns_path="$ROOT/config/plan9.ns"
  if [ ! -f "$ns_path" ]; then
    echo "❌ Missing namespace file: $ns_path" >&2
    return 1
  fi
  if cp "$ns_path" "$STAGE_DIR/etc/plan9.ns"; then
    cp "$ns_path" "$ROOT/out/etc/plan9.ns"
    log "✅ plan9.ns staged"
  else
    echo "❌ plan9.ns staging failed" >&2
    return 1
  fi
}
ensure_plan9_ns

# Stage rc script if available
if [ -f "userland/miniroot/bin/rc" ]; then
  cp "userland/miniroot/bin/rc" "$STAGE_DIR/etc/rc"
  chmod +x "$STAGE_DIR/etc/rc"
  log "✅ Staged /etc/rc"
fi

# Detect platform and GPU availability
COH_PLATFORM="$(uname -m)"
case "$COH_PLATFORM" in
  x86_64|amd64)
    COH_ARCH="x86_64"
    ;;
  aarch64|arm64)
    COH_ARCH="aarch64"
    ;;
  *)
    COH_ARCH="$COH_PLATFORM"
    ;;
esac

COH_GPU=0
if command -v nvidia-smi >/dev/null 2>&1; then
  if nvidia-smi > /tmp/nvidia_smi.log 2>&1; then
    COH_GPU=1
    log "nvidia-smi output:" && cat /tmp/nvidia_smi.log
  else
    log "nvidia-smi present but failed: $(cat /tmp/nvidia_smi.log)"
  fi
elif [ -c /dev/nvidia0 ]; then
  COH_GPU=1
elif command -v lspci >/dev/null 2>&1 && lspci | grep -qi nvidia; then
  COH_GPU=1
fi

export COH_ARCH COH_GPU
export COH_PLATFORM="$COH_ARCH"
log "Detected platform: $COH_ARCH, GPU=$COH_GPU"

# Set cross compiler for aarch64 if available
if [ "$COH_ARCH" = "aarch64" ]; then
  if [ -n "$CROSS_GCC" ]; then
    export CC_aarch64_unknown_linux_gnu="$CROSS_GCC"
    log "✅ Using GNU cross compiler at $CROSS_GCC"
  elif [ -x "/opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc" ]; then
    export CC_aarch64_unknown_linux_gnu="/opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc"
    log "✅ Using GNU cross compiler at /opt/aarch64-linux-gnu/bin/aarch64-linux-gnu-gcc"
  else
    log "⚠️ aarch64 cross GCC not found in PATH or /opt/aarch64-linux-gnu/bin"
  fi
fi

validate_git_submodules() {
  if [ ! -f "$ROOT/.gitmodules" ]; then
    log "ℹ️ No git submodules configured; skipping validation"
    return
  fi

  if ! command -v git >/dev/null 2>&1; then
    log "❌ git not found; unable to validate submodule checkout"
    log "➡️ Install git and run 'git submodule update --init --recursive' before rerunning."
    exit 1
  fi

  submodule_lines=$(git config --file "$ROOT/.gitmodules" --get-regexp '^submodule\\..*\\.path$' 2>/dev/null || true)
  if [ -z "$submodule_lines" ]; then
    log "ℹ️ .gitmodules present but no submodule paths defined"
    return
  fi

  missing_paths=""
  while IFS=' ' read -r _ path; do
    [ -z "$path" ] && continue
    module_dir="$ROOT/$path"
    if [ ! -d "$module_dir" ] || [ ! -e "$module_dir/.git" ]; then
      if [ -n "$missing_paths" ]; then
        missing_paths="$missing_paths $path"
      else
        missing_paths="$path"
      fi
    fi
  done <<EOF
$submodule_lines
EOF

  if [ -n "$missing_paths" ]; then
    log "❌ Missing git submodules: $missing_paths"
    log "➡️ Run 'git submodule update --init --recursive' from $ROOT to populate required dependencies before rerunning."
    exit 1
  fi

  log "✅ All git submodule directories present"
}

validate_git_submodules

export PATH="$HOME/.local/bin:$PATH"
if [ "$ENABLE_PYTHON" -eq 1 ]; then
  if command -v python3 >/dev/null 2>&1; then
    log "🐍 Python tooling enabled (--enable-python)"
    if python3 -m pip --version >/dev/null 2>&1; then
      :
    elif python3 -m ensurepip --upgrade >/dev/null 2>&1; then
      log "✅ ensurepip provisioned pip"
    else
      log "⚠️ python3 detected but pip/ensurepip unavailable; skipping Python dependency installation"
    fi

    if python3 -m pip --version >/dev/null 2>&1; then
      if ! python3 -m pip install --upgrade --user pip setuptools wheel; then
        log "⚠️ Failed to upgrade pip tooling; continuing without Python dependency installation"
      fi
      if ! python3 -m pip install --user ply lxml; then
        log "⚠️ Failed to install base Python packages ply/lxml"
      fi
      if [ -f requirements.txt ]; then
        if ! python3 -m pip install --user -r requirements.txt; then
          log "⚠️ Failed to install requirements.txt dependencies"
        fi
      fi
      if [ -f tests/requirements.txt ]; then
        if ! python3 -m pip install --user -r tests/requirements.txt; then
          log "⚠️ Failed to install tests/requirements.txt dependencies"
        fi
      fi
    fi
  else
    log "⚠️ --enable-python provided but python3 not found; skipping Python dependency installation"
  fi
else
  log "⏭️ Skipping Python tooling (run with --enable-python to install Python dependencies)"
fi

# --- GUI orchestrator -----------------------------------------------------
GUI_DIR="$ROOT/go/cmd/gui-orchestrator"
if [ -d "$GUI_DIR" ]; then
    log "👁️  Building GUI orchestrator"

    GO_CMD="${GO_CMD:-}"
    if [ -z "$GO_CMD" ]; then
        if command -v go >/dev/null 2>&1; then
            GO_CMD="$(command -v go)"
        elif [ -n "${GOROOT:-}" ] && resolved=$(resolve_tool_path "$GOROOT/bin/go"); then
            GO_CMD="$resolved"
        elif resolved=$(resolve_tool_path "$HOME/go/bin/go"); then
            GO_CMD="$resolved"
        elif resolved=$(resolve_tool_path "/usr/local/go/bin/go"); then
            GO_CMD="$resolved"
        elif resolved=$(resolve_tool_path "/opt/homebrew/bin/go"); then
            GO_CMD="$resolved"
        fi
    fi

    if [ -z "$GO_CMD" ] || [ ! -x "$GO_CMD" ]; then
        echo "❌ Go toolchain not found. Install Go (e.g. via 'brew install go') or set GO_CMD/GOROOT/GOBIN so the go binary is discoverable." | tee -a "$SUMMARY_ERRORS" >&3
        echo "➡️ See Solution Architecture §7 and Backlog E4-F10 for GUI control-plane prerequisites." | tee -a "$SUMMARY_ERRORS" >&3
        exit 1
    fi

    case "$COH_ARCH" in
        aarch64) GOARCH=arm64  ;;
        x86_64)  GOARCH=amd64 ;;
        *)       GOARCH=$COH_ARCH ;;
    esac

    pushd "$GUI_DIR" >/dev/null

    # One tidy is enough; harmless if already tidy
    "$GO_CMD" mod tidy

    log "  running go test"
    if ! "$GO_CMD" test ./...; then
        echo "❌ GUI orchestrator tests failed" | tee -a "$SUMMARY_TEST_FAILS" >&3
        exit 1
    fi

    OUT_BIN="$GO_HELPERS_DIR/gui-orchestrator"
    log "  compiling (GOOS=linux GOARCH=$GOARCH)"
    if GOOS=linux GOARCH=$GOARCH "$GO_CMD" build -tags unix -o "$OUT_BIN" .; then
        chmod +x "$OUT_BIN"
        log "✅ GUI orchestrator built → $OUT_BIN"
    else
        echo "❌ GUI orchestrator build failed" | tee -a "$SUMMARY_ERRORS" >&3
        exit 1
    fi

    popd >/dev/null
else
    log "⚠️  GUI orchestrator source not found – skipping"
fi
# -------------------------------------------------------------------------

log "🔧 Checking C compiler..."
if ! command -v gcc >/dev/null 2>&1; then
  echo "❌ gcc not found. Install with: sudo apt install build-essential" >&2
  exit 1
fi
CC_TEST_TMP="$(mktemp)"
CC_TEST_C="${CC_TEST_TMP}.c"
mv "$CC_TEST_TMP" "$CC_TEST_C"
cat <<'EOF' > "$CC_TEST_C"
#include <stdio.h>
int main(void){ printf("hello\n"); return 0; }
EOF
CC_TEST_BIN="${CC_TEST_C%.c}"
if gcc "$CC_TEST_C" -o "$CC_TEST_BIN" >/dev/null 2>&1 && "$CC_TEST_BIN" >/dev/null; then
  log "✅ C compiler operational"
  rm -f "$CC_TEST_C" "$CC_TEST_BIN"
else
  echo "❌ C compiler or linker failed" >&2
  rm -f "$CC_TEST_C" "$CC_TEST_BIN"
  exit 1
fi

log "🧱 Building C components..."
if [ -f "$ROOT/CMakeLists.txt" ]; then
  cd "$ROOT"
  mkdir -p build
  cd "$ROOT/build"
  cmake "$ROOT" && make -j"$(detect_cpu_count)"
else
  echo "⚠️ No CMakeLists.txt found at $ROOT, skipping C build"
fi

log "🔧 Building BusyBox..."
cd "$ROOT"
"$ROOT/scripts/build_busybox.sh" "$COH_ARCH"
BUSYBOX_BIN="$ROOT/out/busybox/$COH_ARCH/bin/busybox"
if [ -x "$BUSYBOX_BIN" ]; then
  cp "$BUSYBOX_BIN" "$STAGE_DIR/bin/busybox"
  log "✅ BusyBox built"

  log "✅ Staged BusyBox applets to /bin"
  if [ -f "$ROOT/userland/miniroot/bin/init" ]; then
    cp "$ROOT/userland/miniroot/bin/init" "$STAGE_DIR/bin/init"
    chmod +x "$STAGE_DIR/bin/init"
  fi
  if [ -f "$ROOT/userland/miniroot/bin/rc" ]; then
    cp "$ROOT/userland/miniroot/bin/rc" "$STAGE_DIR/bin/rc"
    chmod +x "$STAGE_DIR/bin/rc"
  fi
else
  echo "❌ BusyBox build failed" >&2
  exit 1
fi

log "🔧 Building Rust workspace binaries..."

if ! grep -q "\[workspace\]" "$ROOT/workspace/Cargo.toml"; then
  echo "ERROR: top-level Cargo.toml is not a workspace"
  exit 1
fi

cd "$ROOT/workspace"

SKIP_RUST_BUILD=0
if [ "$PHASE" = "4" ]; then
  SKIP_RUST_BUILD=1
fi

if [ "$SKIP_RUST_BUILD" -eq 0 ]; then
  log "🔨 Running Rust build section"
  cargo clean
  mkdir -p target/release/deps target/debug/deps

  # Phase 1: host crates under musl
  log "🔨 Phase 1: Building & testing host crates (musl userland)"
  rustup target add aarch64-unknown-linux-musl || true
  cargo build --release --workspace \
    --exclude 'cohesix_root' \
    --exclude 'sel4-sys-extern-wrapper' \
    --target=aarch64-unknown-linux-musl
  log "⏭️ Cross-target tests compiled only (aarch64-unknown-linux-musl)"
  cargo test --release --workspace \
    --exclude 'cohesix_root' \
    --exclude 'sel4-sys-extern-wrapper' \
    --target=aarch64-unknown-linux-musl \
    --no-run
  log "✅ Phase 1 build succeeded (tests compiled for cross-target)"
  log "✅ Phase 1 build succeeded"

  # Common stub-header setup
  SEL4_LIB_DIR="${ROOT}/third_party/seL4/lib"
  : "${SEL4_LIB_DIR:?SEL4_LIB_DIR must be set}"
  export LIBRARY_PATH="$SEL4_LIB_DIR:${LIBRARY_PATH:-}"

  export LDFLAGS="-L${SEL4_LIB_DIR}"

  # Phase 2: sel4-sys-extern-wrapper under nightly
  log "🔨 Phase 2: Building sel4-sys-extern-wrapper"
  export CFLAGS="-I${ROOT}/workspace/sel4-sys-extern-wrapper/out"
  export LDFLAGS="-L${SEL4_LIB_DIR}"
  export RUSTFLAGS="-C panic=abort -L${SEL4_LIB_DIR} ${CROSS_RUSTFLAGS}"
  rustup component add rust-src --toolchain nightly || true
  cargo +nightly build -p sel4-sys-extern-wrapper --release \
    --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" \
    -Z build-std=core,alloc,compiler_builtins \
    -Z build-std-features=compiler-builtins-mem
  WRAPPER_RLIB=$(find target/sel4-aarch64/release/deps -maxdepth 1 -name 'libsel4_sys_extern_wrapper*.rlib' -print -quit 2>/dev/null)
  if [ -n "$WRAPPER_RLIB" ]; then
    log "✅ sel4-sys-extern-wrapper built: $(basename "$WRAPPER_RLIB")"
  else
    echo "❌ wrapper build failed: artifact missing" >&2
    exit 1
  fi

  # Phase 3: cohesix_root under nightly
  log "🔨 Phase 3: Building cohesix_root"
  export LDFLAGS="-L${SEL4_LIB_DIR}"
  export RUSTFLAGS="-C panic=abort -L${SEL4_LIB_DIR} ${CROSS_RUSTFLAGS}"
  rustup component add rust-src --toolchain nightly || true
  cargo +nightly build \
    -p cohesix_root \
    --release \
    --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" \
    -Z build-std=core,alloc,compiler_builtins \
    -Z build-std-features=compiler-builtins-mem
  ROOT_ARTIFACT=$(find target/sel4-aarch64/release/deps -maxdepth 1 -name 'libcohesix_root*.rlib' -print -quit 2>/dev/null)
  if [ -z "$ROOT_ARTIFACT" ]; then
    BIN_ARTIFACT=$(find target/sel4-aarch64/release -maxdepth 1 -type f -name 'cohesix_root' -print -quit 2>/dev/null)
  else
    BIN_ARTIFACT=""
  fi
  if [ -n "$ROOT_ARTIFACT" ] || [ -n "$BIN_ARTIFACT" ]; then
    log "✅ cohesix_root built: $(basename "${ROOT_ARTIFACT:-$BIN_ARTIFACT}")"
  else
    echo "❌ cohesix_root build failed: artifact missing" >&2
    exit 1
  fi

  log "✅ All Rust components built with proper split targets"
else
  log "⏭️ Phase 4: Skipping Rust rebuild; reusing existing artifacts"
fi

CROSS_RELEASE_DIR="$ROOT/workspace/target/${COHESIX_ARCH}-unknown-linux-musl/release"
if [ -d "$CROSS_RELEASE_DIR" ]; then
  TARGET_DIR="$CROSS_RELEASE_DIR"
else
  TARGET_DIR="$ROOT/workspace/target/release"
fi
# Stage all main binaries
for bin in cohcc cohbuild cli_cap cohtrace cohrun_cli cohagent cohrole cohrun cohup srvctl indexserver devwatcher physics-server exportfs import mount srv scenario_compiler cloud cohesix cohfuzz; do
  BIN_PATH="$TARGET_DIR/$bin"
  if [ -f "$BIN_PATH" ]; then
    cp "$BIN_PATH" "$STAGE_DIR/bin/$bin"
    cp "$BIN_PATH" "$ROOT/out/bin/$bin"
  else
    echo "❌ $bin not found in $TARGET_DIR" >&2
    exit 1
  fi
  [ -f "$STAGE_DIR/bin/$bin" ] || { echo "❌ $bin missing after staging" >&2; exit 1; }
done

# Ensure physics-server and srv exist after staging
[ -f "$STAGE_DIR/bin/physics-server" ] || { echo "❌ physics-server missing after staging" >&2; exit 1; }
[ -f "$STAGE_DIR/bin/srv" ] || { echo "❌ srv missing after staging" >&2; exit 1; }

log "📖 Building mandoc and staging man pages..."
bash "$ROOT/scripts/build_mandoc.sh"
MANDOC_BIN="$ROOT/prebuilt/mandoc/mandoc.$COH_ARCH"
if [ -f "$MANDOC_BIN" ]; then
  cp "$MANDOC_BIN" "$STAGE_DIR/bin/man"
  log "✅ Built mandoc" 
  if [ -d "$ROOT/workspace/docs/man" ]; then
    mkdir -p "$STAGE_DIR/usr/share/man/man1" "$STAGE_DIR/usr/share/man/man8"
    cp "$ROOT/workspace/docs/man/"*.1 "$STAGE_DIR/usr/share/man/man1/" 2>/dev/null || true
    cp "$ROOT/workspace/docs/man/"*.8 "$STAGE_DIR/usr/share/man/man8/" 2>/dev/null || true
    log "✅ Updated man pages"
  fi
  log "✅ Staged mandoc to /usr/bin"
  cat > "$STAGE_DIR/etc/README.txt" <<'EOF'
Cohesix OS Quick Start

Tools: cohcli, cohrun, cohtrace, cohcc, cohcap, cohesix-shell, mandoc

Usage:
  cohcli status --verbose
  cohrun kiosk_start
  cohtrace list

Logs: /log/
Traces: /log/trace/
Manual pages: mandoc -Tascii /usr/share/man/man1/<tool>.1
EOF
  log "✅ Created /etc/README.txt"
else
  echo "❌ mandoc build failed" >&2
  exit 1
fi

# Stage Plan9 rc tests
mkdir -p "$STAGE_DIR/bin/tests"
cp -- "$ROOT/tests/Cohesix/"*.rc "$STAGE_DIR/bin/tests/" 2>/dev/null || true
chmod +x "$STAGE_DIR/bin/tests/"*.rc
log "✅ Staged Plan9 rc tests to /bin/tests"

echo "✅ All builds complete."

echo "[🧪] Checking boot prerequisites..."
if [ ! -x "$STAGE_DIR/bin/init" ]; then
  echo "❌ init binary missing in $STAGE_DIR/bin" >&2
  exit 1
fi
if [ ! -f "$STAGE_DIR/etc/plan9.ns" ]; then
  echo "❌ plan9.ns missing at $STAGE_DIR/etc/plan9.ns" >&2
  exit 1
fi

log "🏗️  Staging complete filesystem..."
echo "BUILD AND STAGING COMPLETE"
BIN_COUNT=$(find "$STAGE_DIR/bin" -type f -perm -111 | wc -l)
ROLE_COUNT=$(find "$STAGE_DIR/roles" -name '*.yaml' | wc -l)
log "FS BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles staged" >&3

# Ensure all staged binaries are executable
chmod +x "$STAGE_DIR/bin"/*

# 1) Create boot directory
mkdir -p "$ROOT/boot"


# 2) Stage cohesix_root and elfloader
ROOT_ELF_SRC="$ROOT/workspace/target/sel4-aarch64/release/cohesix_root"
ROOT_ELF_DST="$ROOT/third_party/seL4/artefacts/cohesix_root.elf"
mkdir -p "$ROOT/out/bin"
if [ -f "$ROOT_ELF_SRC" ]; then
  cp -- "$ROOT_ELF_SRC" "$ROOT/out/bin/cohesix_root.elf"
  cp -- "$ROOT_ELF_SRC" "$ROOT_ELF_DST"
elif [ -f "$ROOT_ELF_DST" ]; then
  log "♻️  Reusing previously staged cohesix_root.elf"
  cp -- "$ROOT_ELF_DST" "$ROOT/out/bin/cohesix_root.elf"
else
  echo "❌ Missing cohesix_root artefact. Run cohesix_fetch_build.sh --phase=3 before invoking phase 4" >&2
  exit 1
fi

cp -- "$ROOT/third_party/seL4/artefacts/elfloader" \
      "$ROOT/boot/elfloader"

# 3) Verify artefacts exist
cd "$ROOT/third_party/seL4/artefacts"
[ -f kernel.elf ] || { echo "❌ Missing kernel.elf" >&2; exit 1; }
[ -f cohesix_root.elf ] || { echo "❌ Missing cohesix_root.elf" >&2; exit 1; }
[ -f kernel.dtb ] || { echo "❌ Missing kernel.dtb" >&2; exit 1; }

# 4) Pack into a newc cpio archive with the kernel DTB as the second image
printf '%s\n' kernel.elf kernel.dtb cohesix_root.elf | \
  cpio -o -H newc > "$ROOT/boot/cohesix.cpio"

CPIO_IMAGE="$ROOT/boot/cohesix.cpio"

# Verify archive order
log "📦 CPIO first entries:"
cpio_listing=$(cpio -it < "$ROOT/boot/cohesix.cpio" | head -n 3)
printf '%s\n' "$cpio_listing" >&3
cpio_entry_1=$(printf '%s\n' "$cpio_listing" | sed -n '1p')
cpio_entry_2=$(printf '%s\n' "$cpio_listing" | sed -n '2p')
cpio_entry_3=$(printf '%s\n' "$cpio_listing" | sed -n '3p')
if [ "$cpio_entry_1" != "kernel.elf" ] || \
   [ "$cpio_entry_2" != "kernel.dtb" ] || \
   [ "$cpio_entry_3" != "cohesix_root.elf" ]; then
  echo "❌ Unexpected CPIO order (expected kernel.elf kernel.dtb cohesix_root.elf): $cpio_listing" >&2
  exit 1
fi

# Replace the embedded archive inside the elfloader's .rodata section
patch_elfloader_archive() {
  local target="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$target" "$CPIO_IMAGE" <<'PY'
import pathlib
import subprocess
import sys

elf_path = pathlib.Path(sys.argv[1])
cpio_path = pathlib.Path(sys.argv[2])

nm_output = subprocess.check_output([
    "aarch64-linux-gnu-nm", "-n", str(elf_path)
], text=True)
symbols = {}
for line in nm_output.splitlines():
    parts = line.split()
    if len(parts) >= 3:
        addr_str, _typ, name = parts[:3]
        if name in {"_archive_start", "_archive_end"}:
            symbols[name] = int(addr_str, 16)

start = symbols.get("_archive_start")
end = symbols.get("_archive_end")
if start is None or end is None:
    sys.exit("missing archive symbols on {}".format(elf_path))
if end <= start:
    sys.exit("archive end precedes start on {}".format(elf_path))

readelf_output = subprocess.check_output([
    "aarch64-linux-gnu-readelf", "-S", str(elf_path)
], text=True)
rodata_addr = rodata_off = rodata_size = None
lines = readelf_output.splitlines()
for idx, line in enumerate(lines):
    parts = line.split()
    if len(parts) >= 6 and parts[2] == '.rodata':
        rodata_addr = int(parts[4], 16)
        rodata_off = int(parts[5], 16)
        if idx + 1 < len(lines):
            size_parts = lines[idx + 1].split()
            if size_parts:
                rodata_size = int(size_parts[0], 16)
        break
if rodata_addr is None:
    sys.exit("failed to locate .rodata on {}".format(elf_path))

start_off = rodata_off + (start - rodata_addr)
end_off = rodata_off + (end - rodata_addr)
if start_off < rodata_off or end_off > rodata_off + rodata_size:
    sys.exit("archive slice outside .rodata for {}".format(elf_path))

archive_capacity = end_off - start_off
cpio_data = pathlib.Path(cpio_path).read_bytes()
if len(cpio_data) > archive_capacity:
    sys.exit(
        f"new archive ({len(cpio_data)}) exceeds available space ({archive_capacity})"
    )

blob = bytearray(elf_path.read_bytes())
blob[start_off:start_off + len(cpio_data)] = cpio_data
pad_start = start_off + len(cpio_data)
if pad_start < end_off:
    blob[pad_start:end_off] = b"\x00" * (end_off - pad_start)
elf_path.write_bytes(blob)
PY
  else
    python_skip_log
    echo "❌ python3 is required to patch the elfloader archive" >&2
    exit 1
  fi
}

patch_elfloader_archive "$ROOT/third_party/seL4/artefacts/elfloader"
patch_elfloader_archive "$ROOT/boot/elfloader"

# Use plain grep here so the pipe drains fully; `grep -q` would trigger
# SIGPIPE on `strings` under `set -o pipefail` and wrongly flag a failure.
if ! strings -a "$ROOT/boot/elfloader" | grep 'ROOTSERVER ONLINE' >/dev/null; then
  echo "❌ Patched elfloader archive does not contain cohesix_root payload" >&2
  exit 1
fi

echo "✅ ELFLoader archive replaced"

# 5) Return to repository root for downstream tooling
cd "$ROOT"

#
# -----------------------------------------------------------
# QEMU bare metal boot test (aarch64)
# -----------------------------------------------------------

log "🧪 Booting elfloader + kernel in QEMU..."

# Timestamped log files
QEMU_LOG="$LOG_DIR/qemu_debug_$(date +%Y%m%d_%H%M%S).log"
QEMU_SERIAL_LOG="$LOG_DIR/qemu_serial_$(date +%Y%m%d_%H%M%S).log"

# Base QEMU flags
QEMU_FLAG_LIST="-nographic -serial mon:stdio"

if [ "${DEBUG_QEMU:-0}" = "1" ]; then
  echo "🔍 QEMU debug mode enabled: GDB stub on :1234, tracing CPU and MMU events"
  # Connect using: gdb -ex 'target remote :1234' <vmlinux>
  QEMU_FLAG_LIST="$QEMU_FLAG_LIST -S -s -d cpu_reset,int,mmu,page,unimp"
fi

# Launch QEMU
qemu-system-aarch64 \
  -M virt,gic-version=2 \
  -cpu cortex-a57 \
  -m 1024M \
  -kernel "$ROOT/boot/elfloader" \
  -initrd "$CPIO_IMAGE" \
  -dtb "$ROOT/third_party/seL4/artefacts/kernel.dtb" \
  $QEMU_FLAG_LIST \
  -D "$QEMU_LOG" 2>&1 | tee "$QEMU_SERIAL_LOG"

# Report where logs went
log "✅ QEMU debug log saved to $QEMU_LOG"
log "✅ QEMU serial log saved to $QEMU_SERIAL_LOG"

# Pre-handoff reserved region dump
echo "Reserved regions:" | tee -a "$TRACE_LOG"
grep -i "reserved" "$QEMU_SERIAL_LOG" | tee -a "$TRACE_LOG"

# Append to trace log for CI
echo "QEMU debug log: $QEMU_LOG" >> "$TRACE_LOG"
echo "QEMU serial log: $QEMU_SERIAL_LOG" >> "$TRACE_LOG"

# Generate manifest of staged binaries
MANIFEST="$STAGE_DIR/manifest.json"
echo '{"binaries":[' > "$MANIFEST"
first=1
for bin in $(find "$STAGE_DIR/bin" -type f -perm -111); do
  hash=$(shasum -a 256 "$bin" | awk '{print $1}')
  ver=$(git rev-parse --short HEAD)
  if [ $first -eq 0 ]; then echo ',' >> "$MANIFEST"; fi
  first=0
  printf '{"file":"%s","hash":"%s","version":"%s"}' "${bin#$STAGE_DIR/}" "$hash" "$ver" >> "$MANIFEST"
done
echo ']}' >> "$MANIFEST"

log "✅ [Build Complete] $(date)"

grep -Ei 'error|fail|panic|permission denied|warning' "$LOG_FILE" > "$SUMMARY_ERRORS" || true
grep -A 5 -E '^failures:|thread .* panicked at' "$LOG_FILE" > "$SUMMARY_TEST_FAILS" || true

echo "⚠️  Summary of Errors and Warnings:" | tee -a "$LOG_FILE" >&3
tail -n 10 "$SUMMARY_ERRORS" || echo "✅ No critical issues found" | tee -a "$LOG_FILE" >&3

echo "🪵 Full log saved to $LOG_FILE" >&3

# Final verification builds
export SEL4_INCLUDE
export SEL4_LIB_DIR
RUSTFLAGS="-C panic=abort -L $SEL4_LIB_DIR $CROSS_RUSTFLAGS" \
  cargo build -p sel4-sys-extern-wrapper --release --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}"
RUSTFLAGS="-C panic=abort -L $SEL4_LIB_DIR $CROSS_RUSTFLAGS" \
  cargo build -p cohesix_root --release --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}"
RUSTFLAGS="-C panic=abort -L $SEL4_LIB_DIR $CROSS_RUSTFLAGS" \
  cargo test --release --target="${SEL4_TARGET_SPEC_SANITIZED:-$SEL4_TARGET_SPEC_SRC}" --workspace
