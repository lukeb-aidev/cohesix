#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: setup_build_env.sh v0.8
# Author: Lukas Bower
# Date Modified: 2030-07-07
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ARCH="$(uname -m)"
OS_NAME="$(uname -s)"

if [[ "$OS_NAME" != "Darwin" ]]; then
    printf '\e[31m[ERR]\e[0m This setup only supports macOS (Darwin). Detected %s.\n' "$OS_NAME" >&2
    exit 1
fi

if [[ "$ARCH" != "arm64" && "$ARCH" != "aarch64" ]]; then
    printf '\e[31m[ERR]\e[0m This setup only supports Apple Silicon (arm64/aarch64). Detected %s.\n' "$ARCH" >&2
    exit 1
fi
# Load or prompt for persistent architecture configuration
normalize_arch() {
    case "$1" in
        arm64) echo "aarch64" ;;
        amd64) echo "x86_64" ;;
        *) echo "$1" ;;
    esac
}

if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
    # shellcheck source=./scripts/load_arch_config.sh
    source "$ROOT/scripts/load_arch_config.sh" --prompt
else
    echo "⚠️  load_arch_config.sh not found. Skipping architecture config."
    COHESIX_ARCH="$(normalize_arch "$(uname -m)")"
    export COHESIX_ARCH
    echo "🔧 Fallback: setting COHESIX_ARCH to $COHESIX_ARCH"
    CONFIG_FILE="$HOME/.cohesix_config"
    echo "COHESIX_ARCH=$COHESIX_ARCH" > "$CONFIG_FILE"
    echo "✅ Wrote fallback config to $CONFIG_FILE"
fi

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

install_cohesix_zshrc_snippet() {
    local repo_path="$1"
    local workspace_dir="$2"
    local zshrc="$HOME/.zshrc"
    local begin_marker="# >>> Cohesix auto-venv >>>"
    local end_marker="# <<< Cohesix auto-venv <<<"
    local tmp_file="$workspace_dir/.zshrc.cohesix.tmp"

    mkdir -p "$workspace_dir"
    if [ ! -f "$zshrc" ]; then
        : > "$zshrc"
    fi

    local begin_escaped end_escaped
    begin_escaped="$(printf '%s\n' "$begin_marker" | sed 's/[\\/&]/\\&/g')"
    end_escaped="$(printf '%s\n' "$end_marker" | sed 's/[\\/&]/\\&/g')"

    sed "/$begin_escaped/,/$end_escaped/d" "$zshrc" > "$tmp_file"

    cat <<'EOF' >> "$tmp_file"

# >>> Cohesix auto-venv >>>
[[ $- != *i* ]] && return

COHESIX_AUTO_ENV_ROOT="__COHESIX_REPO__"

if [ -f "$COHESIX_AUTO_ENV_ROOT/.env" ]; then
    set -a
    # shellcheck disable=SC1090
    source "$COHESIX_AUTO_ENV_ROOT/.env"
    set +a
    echo "🔹 Loaded Cohesix environment variables."
fi

if [ -d "$COHESIX_AUTO_ENV_ROOT/.venv" ]; then
    COHESIX_AUTO_VENV_ROOT="$COHESIX_AUTO_ENV_ROOT"
    # shellcheck disable=SC1090
    if source "$COHESIX_AUTO_ENV_ROOT/.venv/bin/activate"; then
        echo "🔹 Activating Cohesix Python virtual environment..."
    else
        echo "⚠️  Cohesix virtual environment activation failed." >&2
    fi
fi
# <<< Cohesix auto-venv <<<
EOF

    local replaced_file="$workspace_dir/.zshrc.cohesix.replaced"
    awk -v rp="$repo_path" '{gsub("__COHESIX_REPO__", rp)}1' "$tmp_file" > "$replaced_file"
    mv "$replaced_file" "$zshrc"
    rm -f "$tmp_file"
}

ensure_homebrew_shellenv() {
    if command -v brew >/dev/null 2>&1; then
        eval "$(brew shellenv)"
        return
    fi

    if [ -x "/opt/homebrew/bin/brew" ]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
        return
    fi

    if [ -x "/usr/local/bin/brew" ]; then
        eval "$(/usr/local/bin/brew shellenv)"
        return
    fi

    msg "Installing Homebrew package manager …"
    NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    if [ -x "/opt/homebrew/bin/brew" ]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
    elif [ -x "/usr/local/bin/brew" ]; then
        eval "$(/usr/local/bin/brew shellenv)"
    else
        die "Homebrew installation did not provide a usable brew binary."
    fi
}

ensure_brew_tap() {
    local tap_name="$1"
    if brew tap | grep -Fxq "$tap_name"; then
        msg "Homebrew tap $tap_name already configured."
        return
    fi

    msg "Adding Homebrew tap $tap_name …"
    brew tap "$tap_name"
}

ensure_brew_packages() {
    local manager_script="$ROOT/scripts/manage_homebrew_packages.sh"
    if [ ! -x "$manager_script" ]; then
        die "Missing package manager helper at $manager_script"
    fi
    "$manager_script" install "$@"
}

ensure_python_bin() {
    local python_bin=""
    for candidate in python3.12 python3.11 python3.10 python3; do
        if command -v "$candidate" >/dev/null 2>&1; then
            local bin_path
            bin_path="$(command -v "$candidate")"
            if [[ "$OS_NAME" == "Darwin" && "$bin_path" == "/usr/bin/python3" ]]; then
                continue
            fi
            python_bin="$bin_path"
            break
        fi
    done

    if [[ -z "$python_bin" ]]; then
        die "Unable to locate a usable python3 interpreter."
    fi

    printf '%s\n' "$python_bin"
}

msg "Detected macOS host (architecture: $ARCH)."

MACOS_VERSION="$(sw_vers -productVersion 2>/dev/null || echo "0")"
MACOS_MAJOR="${MACOS_VERSION%%.*}"
if [[ "$MACOS_MAJOR" =~ ^[0-9]+$ && "$MACOS_MAJOR" -lt 26 ]]; then
    msg "WARNING: macOS $MACOS_VERSION detected. This setup is validated for macOS 26 or newer."
fi

ensure_homebrew_shellenv
ensure_brew_tap "messense/macos-cross-toolchains"

default_shell="${SHELL:-}"
if [[ "$default_shell" == *"zsh" ]]; then
    msg "Configuring Cohesix environment for Zsh users."
    uname_arch="$(uname -m)"
    COHESIX_ARCH="${COHESIX_ARCH:-$(normalize_arch "$uname_arch")}"
    export COHESIX_ARCH

    zsh_env_dir="$HOME/.cohesix"
    zsh_env_file="$zsh_env_dir/env.zsh"
    mkdir -p "$zsh_env_dir"

    brew_bin="$(command -v brew 2>/dev/null || true)"
    brew_prefix=""
    if [[ -n "$brew_bin" ]]; then
        brew_prefix="$("$brew_bin" --prefix 2>/dev/null || true)"
    fi

    cat > "$zsh_env_file" <<EOF_ZSH
# Author: Lukas Bower
# shellcheck shell=zsh
# Generated by scripts/setup_build_env.sh
export COHESIX_ARCH="${COHESIX_ARCH}"

cohesix_prepend_path() {
    local dir="\$1"
    if [ -d "\$dir" ]; then
        case ":\$PATH:" in
            *:"\$dir":*) ;;
            *) PATH="\$dir:\$PATH" ;;
        esac
    fi
}

cohesix_prepend_path "$ROOT/.venv-runtime/bin"
EOF_ZSH

    if [[ -n "$brew_prefix" ]]; then
        cat >> "$zsh_env_file" <<EOF_ZSH
cohesix_prepend_path "$brew_prefix/bin"
cohesix_prepend_path "$brew_prefix/sbin"
EOF_ZSH
    fi

    if [[ -n "$brew_bin" ]]; then
        cat >> "$zsh_env_file" <<EOF_ZSH
if [ -x "$brew_bin" ]; then
    eval "$($brew_bin shellenv)"
fi
EOF_ZSH
    else
        cat >> "$zsh_env_file" <<'EOF_ZSH'
if command -v brew >/dev/null 2>&1; then
    eval "$(brew shellenv)"
fi
EOF_ZSH
    fi

    cat >> "$zsh_env_file" <<'EOF_ZSH'
unset -f cohesix_prepend_path
EOF_ZSH

    zprofile="$HOME/.zprofile"
    if [ ! -f "$zprofile" ]; then
        : > "$zprofile"
    fi
    if ! grep -Fqs "source ~/.cohesix/env.zsh" "$zprofile"; then
        {
            printf '%s\n' "" "source ~/.cohesix/env.zsh"
        } >> "$zprofile"
    fi

    install_cohesix_zshrc_snippet "$ROOT" "$zsh_env_dir"

    msg "Zsh environment snippet installed at $zsh_env_file. Restart your shell or run 'source ~/.zprofile' to apply changes."
fi

brew_pkgs=(
    qemu
    messense/macos-cross-toolchains/aarch64-unknown-linux-gnu
    llvm
    cmake
    ninja
    python@3.12
    pkg-config
    coreutils
    gnu-tar
)
ensure_brew_packages "${brew_pkgs[@]}"

PYTHON_BIN="$(ensure_python_bin)"

if ! command -v aarch64-unknown-linux-gnu-gcc >/dev/null 2>&1; then
    die "aarch64-unknown-linux-gnu-gcc not found on PATH after installation. Ensure the Homebrew LLVM toolchain is linked."
fi

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    die "qemu-system-aarch64 not found on PATH after installation. Confirm Homebrew's qemu package is correctly installed."
fi

VENV_RUNTIME="$ROOT/.venv-runtime"
if [ -d "$VENV_RUNTIME" ]; then
    rm -rf "$VENV_RUNTIME"
fi

"$PYTHON_BIN" -m venv "$VENV_RUNTIME"
VENV_PYTHON="$VENV_RUNTIME/bin/python3"
if [ ! -x "$VENV_PYTHON" ]; then
    VENV_PYTHON="$VENV_RUNTIME/bin/python"
fi

"$VENV_PYTHON" -m pip install --upgrade pip >/dev/null
"$VENV_PYTHON" -m pip install jinja2 ply pyyaml >/dev/null

# Ensure ~/.cohesix_config exists
CONFIG_FILE="$HOME/.cohesix_config"
if [ ! -f "$CONFIG_FILE" ]; then
    echo "🔧 Creating default Cohesix config at $CONFIG_FILE"
    cat > "$CONFIG_FILE" <<EOF
# Cohesix Architecture Configuration
COHESIX_ARCH=$(normalize_arch "$(uname -m)")
EOF
fi

echo "✅ Build environment setup complete."
