#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

PROJECT_ROOT="$(git rev-parse --show-toplevel)"
source "$PROJECT_ROOT/scripts/cohesix-build-run.sh"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

SEL4_BUILD_DIR="$TMP_DIR"
export SEL4_BUILD_DIR

mkdir -p "$SEL4_BUILD_DIR/kernel/gen_config"
cat > "$SEL4_BUILD_DIR/kernel/gen_config/kernel_config.h" <<'CFG'
#define CONFIG_ARM_GIC_V3 1
CFG

if [[ "$(detect_gic_version)" != "3" ]]; then
    echo "[detect-gic-version] ERROR: expected version 3 from kernel_config.h" >&2
    exit 1
fi

cat > "$SEL4_BUILD_DIR/kernel/gen_config/kernel_config.h" <<'CFG'
CONFIG_ARM_GIC_V2=y
CFG

if [[ "$(detect_gic_version)" != "2" ]]; then
    echo "[detect-gic-version] ERROR: expected version 2 from Kconfig style" >&2
    exit 1
fi

rm -f "$SEL4_BUILD_DIR/kernel/gen_config/kernel_config.h"
mkdir -p "$SEL4_BUILD_DIR/kernel/include"
cat > "$SEL4_BUILD_DIR/kernel/include/autoconf.h" <<'CFG'
CONFIG_ARM_GIC_V3 = y
CFG

if [[ "$(detect_gic_version)" != "3" ]]; then
    echo "[detect-gic-version] ERROR: expected version 3 from autoconf.h" >&2
    exit 1
fi

mkdir -p "$SEL4_BUILD_DIR/kernel/gen_config/kernel"
cat > "$SEL4_BUILD_DIR/kernel/gen_config/kernel/gen_config.h" <<'CFG'
/* disabled: CONFIG_ARM_GIC_V3_SUPPORT */
#define CONFIG_PLAT_QEMU_ARM_VIRT 1
CFG

if [[ "$(detect_gic_version)" != "2" ]]; then
    echo "[detect-gic-version] ERROR: expected version 2 when GICv3 support disabled" >&2
    exit 1
fi

cat > "$SEL4_BUILD_DIR/kernel/gen_config/kernel/gen_config.h" <<'CFG'
#define CONFIG_ARM_GIC_V3_SUPPORT 1
CFG

if [[ "$(detect_gic_version)" != "3" ]]; then
    echo "[detect-gic-version] ERROR: expected version 3 when GICv3 support enabled" >&2
    exit 1
fi

echo "[detect-gic-version] PASS: detected multiple GIC configuration encodings"
