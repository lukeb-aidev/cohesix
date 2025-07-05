# CLASSIFICATION: COMMUNITY
# Filename: build_busybox.sh v0.7
# Date Modified: 2026-11-17
# Author: Lukas Bower

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")"/.. && pwd)"
BUSYBOX_VERSION="1.36.1"
LOCAL_SRC="$ROOT/third_party/busybox"
WORK_DIR="$ROOT/build/busybox"
OUT_DIR="$ROOT/out/busybox"
OUT_BIN="$ROOT/out/bin"
ISO_BIN="$ROOT/out/bin"
SUPPORTED_ARCHS=("x86_64" "aarch64")

msg()  { printf "\e[32m==>\e[0m %s\n" "$*"; }
die()  { printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

ARCHES=("$@")
[[ ${#ARCHES[@]} -eq 0 || ${ARCHES[0]} == "all" ]] && ARCHES=("${SUPPORTED_ARCHS[@]}")
for a in "${ARCHES[@]}"; do
  [[ " ${SUPPORTED_ARCHS[*]} " == *" $a "* ]] || die "Unsupported arch: $a"
done

mkdir -p "$WORK_DIR" "$OUT_DIR" "$OUT_BIN" "$ISO_BIN"
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
      unset CROSS_COMPILE
      export CC="gcc"
      ;;
    aarch64)
      export CROSS_COMPILE="aarch64-linux-gnu-"
      export CC="${CROSS_COMPILE}gcc"
      ;;
  esac

  make mrproper || true
  make defconfig
  sed -i 's/# CONFIG_STATIC is not set/CONFIG_STATIC=y/' .config
  echo "BusyBox config summary:"
  grep -E '^(CONFIG_STATIC|CONFIG_ASH|CONFIG_SH_IS_ASH|CONFIG_LS|CONFIG_CP|CONFIG_MV|CONFIG_ECHO|CONFIG_MOUNT|CONFIG_CAT|CONFIG_PS|CONFIG_KILL)' .config

  make -j"$(nproc)"
  make CONFIG_PREFIX="$INSTALL_DIR" install
  strip "$INSTALL_DIR/bin/busybox" || true
  cp "$INSTALL_DIR/bin/busybox" "$OUT_BIN/busybox"
  cp "$INSTALL_DIR/bin/busybox" "$ISO_BIN/busybox"
  popd > /dev/null
  msg "✅ BusyBox built → $INSTALL_DIR/bin/busybox (also staged to ISO)"
done

msg "All requested BusyBox builds complete."
