// CLASSIFICATION: COMMUNITY
// Filename: build-busybox.sh v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# build-busybox.sh – Cohesix helper
#
# Builds a *static* BusyBox binary for the requested architecture(s) and places
# the result in   ./out/busybox/<arch>/busybox
#
# Supported architectures:
#   • x86_64      – native build
#   • aarch64     – cross‑compile using arm64 gcc (via `aarch64-linux-gnu-gcc`)
#
# Usage:
#   ./scripts/build-busybox.sh x86_64
#   ./scripts/build-busybox.sh aarch64
#   ./scripts/build-busybox.sh all        # default if no arg given
#
# BusyBox version is pinned to v1.36.1 for reproducibility.
###############################################################################
set -euo pipefail

BUSYBOX_VERSION="1.36.1"
BUSYBOX_URL="https://busybox.net/downloads/busybox-${BUSYBOX_VERSION}.tar.bz2"
WORK_DIR="$(pwd)/build/busybox"
OUT_DIR="$(pwd)/out/busybox"

SUPPORTED_ARCHS=("x86_64" "aarch64")

msg()  { printf "\e[32m==>\e[0m %s\n" "$*"; }
die()  { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

###############################################################################
# 1. Parse CLI args
###############################################################################
ARCHES=("$@")
[[ ${#ARCHES[@]} -eq 0 || ${ARCHES[0]} == "all" ]] && ARCHES=("${SUPPORTED_ARCHS[@]}")

for a in "${ARCHES[@]}"; do
  [[ " ${SUPPORTED_ARCHS[*]} " == *" $a "* ]] || die "Unsupported arch: $a"
done

###############################################################################
# 2. Fetch BusyBox source (cached between builds)
###############################################################################
mkdir -p "$WORK_DIR" "$OUT_DIR"
TARBALL="$WORK_DIR/busybox-${BUSYBOX_VERSION}.tar.bz2"

if [[ ! -f $TARBALL ]]; then
  msg "Downloading BusyBox $BUSYBOX_VERSION …"
  curl -L "$BUSYBOX_URL" -o "$TARBALL"
fi

###############################################################################
# 3. Extract source
###############################################################################
SRC_DIR="$WORK_DIR/src-$BUSYBOX_VERSION"
if [[ ! -d $SRC_DIR ]]; then
  msg "Extracting BusyBox source …"
  tar -xf "$TARBALL" -C "$WORK_DIR"
  mv "$WORK_DIR/busybox-$BUSYBOX_VERSION" "$SRC_DIR"
fi

###############################################################################
# 4. Build per‑architecture
###############################################################################
for ARCH in "${ARCHES[@]}"; do
  BUILD_DIR="$WORK_DIR/build-$ARCH"
  INSTALL_DIR="$OUT_DIR/$ARCH"
  mkdir -p "$BUILD_DIR" "$INSTALL_DIR"

  msg "Building BusyBox for $ARCH …"

  # Clean build directory each run
  rm -rf "$BUILD_DIR" && cp -r "$SRC_DIR" "$BUILD_DIR"
  pushd "$BUILD_DIR" > /dev/null

  # Select toolchain
  case "$ARCH" in
    x86_64)
      export CROSS_COMPILE=""
      export CC="gcc"
      ;;
    aarch64)
      export CROSS_COMPILE="aarch64-linux-gnu-"
      export CC="${CROSS_COMPILE}gcc"
      ;;
  esac

  # Default config
  make defconfig >/dev/null

  # Enable static build, disable SELinux & large features for small size
  scripts/config --disable SELINUX       \
                 --disable FEATURE_MOUNT_LABEL \
                 --enable CONFIG_STATIC  > /dev/null || true

  # Include additional utilities
  scripts/config    -e FINGER \
                    -e LAST \
                    -e FREE \
                    -e TOP \
                    -e DF \
                    -e WHO >/dev/null || true

  # Ensure static & tiny
  sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config
  make olddefconfig >/dev/null

  # Build + install
  make -j"$(nproc)" >/dev/null
  make CONFIG_PREFIX="$INSTALL_DIR" install >/dev/null

  popd > /dev/null
  msg "✅ BusyBox built → $INSTALL_DIR/bin/busybox"
done

msg "All requested BusyBox builds complete."
