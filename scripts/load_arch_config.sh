# CLASSIFICATION: COMMUNITY
# Filename: load_arch_config.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-07-25
#!/usr/bin/env bash
# Load persistent architecture configuration.
# If --prompt is given and config is missing, ask user and save.
set -euo pipefail
CONFIG_FILE="${COHESIX_CONFIG:-$HOME/.cohesix_config}"
if [ -f "$CONFIG_FILE" ]; then
    source "$CONFIG_FILE"
else
    if [ "${1:-}" = "--prompt" ]; then
        echo "Select target architecture:" >&2
        select a in x86_64 aarch64; do
            case "$a" in
                x86_64|aarch64) COHESIX_ARCH="$a"; break;;
                *) echo "Invalid choice" >&2;;
            esac
        done
        if [ -z "${COHESIX_ARCH:-}" ]; then
            echo "❌ Architecture not set" >&2
            exit 1
        fi
        cat > "$CONFIG_FILE" <<EOF_CFG
# CLASSIFICATION: COMMUNITY
# Filename: .cohesix_config v0.1
# Author: Lukas Bower
# Date Modified: 2026-07-25
COHESIX_ARCH=$COHESIX_ARCH
EOF_CFG
        echo "✅ Architecture '$COHESIX_ARCH' saved to $CONFIG_FILE" >&2
    else
        echo "❌ COHESIX configuration not found at $CONFIG_FILE" >&2
        echo "Run scripts/setup_build_env.sh to configure." >&2
        exit 1
    fi
fi
export COHESIX_ARCH
