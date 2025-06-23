# CLASSIFICATION: COMMUNITY
# Filename: build_busybox.sh v0.4
# Date Modified: 2026-01-26
# Author: Lukas Bower

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
BUSYBOX_VERSION="1.36.1"
LOCAL_SRC="$ROOT/third_party/busybox"
WORK_DIR="$ROOT/build/busybox"
OUT_DIR="$ROOT/out/busybox"
OUT_BIN="$ROOT/out/bin"
SUPPORTED_ARCHS=("x86_64" "aarch64")

msg()  { printf "\e[32m==>\e[0m %s\n" "$*"; }
die()  { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

ARCHES=("$@")
[[ ${#ARCHES[@]} -eq 0 || ${ARCHES[0]} == "all" ]] && ARCHES=("${SUPPORTED_ARCHS[@]}")
for a in "${ARCHES[@]}"; do
  [[ " ${SUPPORTED_ARCHS[*]} " == *" $a "* ]] || die "Unsupported arch: $a"
done

mkdir -p "$WORK_DIR" "$OUT_DIR" "$OUT_BIN"
SRC_DIR="$LOCAL_SRC"

for ARCH in "${ARCHES[@]}"; do
  BUILD_DIR="$WORK_DIR/build-$ARCH"
  INSTALL_DIR="$OUT_DIR/$ARCH"
  mkdir -p "$BUILD_DIR" "$INSTALL_DIR"

  msg "Building BusyBox for $ARCH"
  rm -rf "$BUILD_DIR" && cp -r "$SRC_DIR" "$BUILD_DIR"
  pushd "$BUILD_DIR" > /dev/null

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

  make mrproper >/dev/null || true
  make defconfig >/dev/null
  scripts/config --enable FEATURE_INSTALLER \
                 --enable APPLET_SYMLINKS \
                --disable SELINUX \
                --disable FEATURE_MOUNT_LABEL \
                --disable TC \
                --disable FEATURE_TC_INGRESS \
                --enable STATIC \
                 --enable ASH \
                 --enable SH_IS_ASH \
                 --enable LS \
                 --enable CP \
                 --enable MV \
                 --enable ECHO \
                 --enable MOUNT \
                 --enable CAT \
                 --enable PS \
                 --enable KILL >/dev/null 2>&1 || true
  sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config
  make olddefconfig >/dev/null
  make -j"$(nproc)" >/dev/null
  make CONFIG_PREFIX="$INSTALL_DIR" install >/dev/null
  strip "$INSTALL_DIR/bin/busybox"
  mkdir -p "$OUT_BIN"
  cp "$INSTALL_DIR/bin/busybox" "$OUT_BIN/busybox"
  popd > /dev/null
  msg "✅ BusyBox built → $INSTALL_DIR/bin/busybox"
done

msg "All requested BusyBox builds complete."
