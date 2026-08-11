#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Install Cohesix macOS ARM64 host toolchain dependencies and pinned Rust toolchain.
# Copyright 2026 Lukas Bower

set -euo pipefail

# Keep setup non-interactive and prevent Homebrew from cleaning up or
# autoremoving unrelated user packages while satisfying this repository's
# explicitly scoped prerequisites.
unset HOMEBREW_ASK
export HOMEBREW_NO_ASK=1
export HOMEBREW_NO_AUTOREMOVE=1
export HOMEBREW_NO_INSTALL_CLEANUP=1
export HOMEBREW_NO_ENV_HINTS=1

BREW_PACKAGES=(
    git
    cmake
    ninja
    llvm@17
    python@3.13
    qemu
    coreutils
    cpio
    jq
    protobuf
    repo
    make
    openssl@3
    pkgconf
)
RUST_TOOLCHAIN_VERSION="1.97.1"
RUST_TARGET="aarch64-unknown-none"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TOOLCHAIN_ROOT="${REPO_ROOT}/out/toolchain"
COMPILER_RELEASE="15.2.Rel1"
COMPILER_VERSION="15.2.1"
COMPILER_TARGET="aarch64-none-elf"
COMPILER_PREFIX="${COMPILER_TARGET}-"
COMPILER_ARCHIVE_NAME="arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz"
COMPILER_ARCHIVE_URL="https://armkeil.blob.core.windows.net/developer/files/downloads/gnu/15.2.rel1/binrel/${COMPILER_ARCHIVE_NAME}"
COMPILER_ARCHIVE_SHA256="37084c99bc05fda43a6c48900c638ae4fd6d93e2287ceb3e9bcda55437f1aadd"
COMPILER_ARCHIVE_SIZE="80863820"
COMPILER_ARCHIVE="${TOOLCHAIN_ROOT}/downloads/${COMPILER_ARCHIVE_NAME}"
COMPILER_INSTALL="${TOOLCHAIN_ROOT}/arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf"
COMPILER_BIN="${COMPILER_INSTALL}/bin"
COMPILER_PROVENANCE="${COMPILER_INSTALL}/cohesix-compiler-provenance.json"
COMPILER_GCC_SHA256="e3aca7127a4407f63b9a3f157021bda8476706370f98ded3fd4a20f898261066"
COMPILER_GXX_SHA256="dce7d3014f33b95e68dbc64ac885048e9de682b1263edea886ece769a4cf2b55"
COMPILER_CPP_SHA256="cd16a3d2f8f9972782c8eddb816675e8a92fe935075628d0340d018d0ee5ad4a"
COMPILER_AS_SHA256="cf01313f7f00d24dedb273b7e3d3753290cf0dd4f4be389094e47de2c90d1e48"
COMPILER_LD_SHA256="16d591c8f44bbadb1c0f96990753b34efe74e0c92a594d1a8b600170e7d99d10"
COMPILER_OBJCOPY_SHA256="0784ea59a9a45dea36c6129c0d5c010db8966508fccbd6b415a0aa6c78e23ddf"
COMPILER_AR_SHA256="38f32b4a1b196da8ef1c34412f68792bb2a3cdc8c20d5c98d70fa44f13c9c1bb"
COMPILER_RANLIB_SHA256="939545db6eac3b78e5e145dcf428db22229aa6eec04cd86f95ca90f9a8d00e99"
PROFILE_VENV="${TOOLCHAIN_ROOT}/sel4-profile-venv"
PYTHON_BOOTSTRAP_LOCK="${REPO_ROOT}/configs/sel4/python-bootstrap.lock"
PYTHON_BOOTSTRAP_LOCK_SHA256="a582a400e97b0b830952482a8923c4097024c9960fe2197315c341ca00a9548a"
PYTHON_REQUIREMENTS_LOCK="${REPO_ROOT}/configs/sel4/python-build-requirements.lock"
PYTHON_REQUIREMENTS_LOCK_SHA256="5ef6b3b1e5edc912a0041417b7386ad17a57fab6ecd3b64c797ecbb3e37ea929"
PROFILE_CONTRACT="${REPO_ROOT}/configs/sel4/profiles.toml"
UBOOT_SOURCE_URL="https://ftp.denx.de/pub/u-boot/u-boot-2026.01.tar.bz2"
UBOOT_SOURCE_ARCHIVE="${TOOLCHAIN_ROOT}/downloads/u-boot-2026.01.tar.bz2"
UBOOT_SOURCE_SHA256="b60d5865cefdbc75da8da4156c56c458e00de75a49b80c1a2e58a96e30ad0d54"
UBOOT_SOURCE_SIZE="34172789"
UBOOT_SOURCE_VERSION="2026.01"
UBOOT_SOURCE_COMMIT="127a42c7257a6ffbbd1575ed1cbaa8f5408a44b3"
UBOOT_SNAPSHOT="${REPO_ROOT}/out/toolchain/u-boot-tools-source"
UBOOT_BUILD="${REPO_ROOT}/out/toolchain/u-boot-tools-build"
MKIMAGE="${UBOOT_BUILD}/tools/mkimage"
MKIMAGE_PROVENANCE="${UBOOT_BUILD}/cohesix-mkimage-provenance.json"
MKIMAGE_VERSION="mkimage version 2026.01"

sha256_file() {
    shasum -a 256 "$1" | awk '{print $1}'
}

require_sha256() {
    local path="$1"
    local expected="$2"
    local actual
    actual="$(sha256_file "${path}")"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "SHA-256 mismatch for ${path}; expected ${expected}, got ${actual}." >&2
        exit 1
    fi
}

if ! command -v brew >/dev/null 2>&1; then
    echo "Homebrew is required. Install it from https://brew.sh/ and re-run this script." >&2
    exit 1
fi

echo "Updating Homebrew formulas..."
brew update
export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_INSTALL_UPGRADE=1

echo "Ensuring required Homebrew packages are present..."
for pkg in "${BREW_PACKAGES[@]}"; do
    if ! brew list --formula "$pkg" >/dev/null 2>&1; then
        echo "Installing $pkg"
        brew install "$pkg"
    else
        echo "Package $pkg already installed"
    fi
done

HOMEBREW_PYTHON="$(brew --prefix python@3.13)/bin/python3.13"
if [[ ! -x "${HOMEBREW_PYTHON}" ]]; then
    echo "Pinned Python 3.13 interpreter is unavailable: ${HOMEBREW_PYTHON}" >&2
    exit 1
fi
if [[ "$("${HOMEBREW_PYTHON}" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')" != "3.13" ]]; then
    echo "The seL4 profile environment requires CPython 3.13." >&2
    exit 1
fi
"${HOMEBREW_PYTHON}" - "${PROFILE_CONTRACT}" <<'PY'
import hashlib
from pathlib import Path
import subprocess
import sys
import tomllib

with Path(sys.argv[1]).open("rb") as stream:
    declared = tomllib.load(stream)["toolchain"]["cpio"]
tool = Path(declared["path"])
if not tool.is_file():
    raise SystemExit(f"Pinned GNU cpio is missing: {tool}")
actual_sha256 = hashlib.sha256(tool.read_bytes()).hexdigest()
if actual_sha256 != declared["sha256"]:
    raise SystemExit(
        f"Pinned GNU cpio digest mismatch: expected {declared['sha256']}, "
        f"got {actual_sha256}"
    )
version = subprocess.run(
    (str(tool), "--version"),
    check=True,
    capture_output=True,
    text=True,
).stdout.splitlines()[0]
expected_version = f"cpio (GNU cpio) {declared['version']}"
if version != expected_version:
    raise SystemExit(
        f"Pinned GNU cpio version mismatch: expected {expected_version!r}, "
        f"got {version!r}"
    )
help_text = subprocess.run(
    (str(tool), "--help"),
    check=True,
    capture_output=True,
    text=True,
).stdout
missing = [option for option in declared["required_options"] if option not in help_text]
if missing:
    raise SystemExit(f"Pinned GNU cpio lacks required options: {', '.join(missing)}")
PY
if [[ -L "${REPO_ROOT}/out" || -L "${TOOLCHAIN_ROOT}" ]]; then
    echo "Refusing a symlinked out/toolchain root." >&2
    exit 1
fi
mkdir -p "${TOOLCHAIN_ROOT}"
REPO_ROOT_RESOLVED="$("${HOMEBREW_PYTHON}" -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve())' "${REPO_ROOT}")"
TOOLCHAIN_ROOT_RESOLVED="$("${HOMEBREW_PYTHON}" -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve())' "${TOOLCHAIN_ROOT}")"
if [[ "${TOOLCHAIN_ROOT_RESOLVED}" != "${REPO_ROOT_RESOLVED}/out/toolchain" ]]; then
    echo "Resolved toolchain root escapes the repository: ${TOOLCHAIN_ROOT_RESOLVED}" >&2
    exit 1
fi

guard_replace_dir() {
    local path="$1"
    local resolved
    if [[ -L "${path}" ]]; then
        echo "Refusing to replace symlinked toolchain directory: ${path}" >&2
        exit 1
    fi
    resolved="$("${HOMEBREW_PYTHON}" -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve(strict=False))' "${path}")"
    case "${resolved}" in
        "${TOOLCHAIN_ROOT_RESOLVED}"/*) ;;
        *)
            echo "Refusing to replace path outside the resolved toolchain root: ${resolved}" >&2
            exit 1
            ;;
    esac
}

remove_generated_dir() {
    local path="$1"
    local attempt
    guard_replace_dir "${path}"
    for attempt in 1 2 3; do
        rm -rf -- "${path}" 2>/dev/null || true
        if [[ ! -e "${path}" && ! -L "${path}" ]]; then
            return 0
        fi
    done
    echo "Could not remove generated toolchain directory after ${attempt} attempts: ${path}" >&2
    return 1
}

guard_replace_dir "$(dirname "${COMPILER_ARCHIVE}")"
mkdir -p "$(dirname "${COMPILER_ARCHIVE}")"
if [[ ! -f "${COMPILER_ARCHIVE}" ]]; then
    curl --fail --location --proto '=https' --tlsv1.2 \
        --output "${COMPILER_ARCHIVE}.tmp" "${COMPILER_ARCHIVE_URL}"
    require_sha256 "${COMPILER_ARCHIVE}.tmp" "${COMPILER_ARCHIVE_SHA256}"
    if [[ "$(stat -f '%z' "${COMPILER_ARCHIVE}.tmp")" != "${COMPILER_ARCHIVE_SIZE}" ]]; then
        echo "Pinned compiler archive size mismatch." >&2
        exit 1
    fi
    mv "${COMPILER_ARCHIVE}.tmp" "${COMPILER_ARCHIVE}"
fi
require_sha256 "${COMPILER_ARCHIVE}" "${COMPILER_ARCHIVE_SHA256}"
if [[ "$(stat -f '%z' "${COMPILER_ARCHIVE}")" != "${COMPILER_ARCHIVE_SIZE}" ]]; then
    echo "Pinned compiler archive size mismatch." >&2
    exit 1
fi

echo "Provisioning Arm GNU Toolchain ${COMPILER_RELEASE}..."
guard_replace_dir "${COMPILER_INSTALL}"
remove_generated_dir "${COMPILER_INSTALL}"
tar -xJf "${COMPILER_ARCHIVE}" -C "${TOOLCHAIN_ROOT}"
for compiler in gcc g++ cpp as ld objcopy ar ranlib; do
    if [[ ! -x "${COMPILER_BIN}/${COMPILER_PREFIX}${compiler}" ]]; then
        echo "Pinned compiler executable is missing: ${COMPILER_PREFIX}${compiler}" >&2
        exit 1
    fi
done
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}gcc" "${COMPILER_GCC_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}g++" "${COMPILER_GXX_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}cpp" "${COMPILER_CPP_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}as" "${COMPILER_AS_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}ld" "${COMPILER_LD_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}objcopy" "${COMPILER_OBJCOPY_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}ar" "${COMPILER_AR_SHA256}"
require_sha256 "${COMPILER_BIN}/${COMPILER_PREFIX}ranlib" "${COMPILER_RANLIB_SHA256}"
if [[ "$("${COMPILER_BIN}/${COMPILER_PREFIX}gcc" -dumpfullversion)" != "${COMPILER_VERSION}" || \
      "$("${COMPILER_BIN}/${COMPILER_PREFIX}gcc" -dumpmachine)" != "${COMPILER_TARGET}" ]]; then
    echo "Pinned compiler identity mismatch." >&2
    exit 1
fi
export PATH="${COMPILER_BIN}:${PATH}"
"${HOMEBREW_PYTHON}" - \
    "${COMPILER_PROVENANCE}" \
    "${COMPILER_ARCHIVE_URL}" \
    "${COMPILER_ARCHIVE}" \
    "${COMPILER_ARCHIVE_SHA256}" \
    "${COMPILER_ARCHIVE_SIZE}" \
    "${COMPILER_RELEASE}" \
    "${COMPILER_VERSION}" \
    "${COMPILER_TARGET}" \
    "${COMPILER_BIN}" \
    "${COMPILER_GCC_SHA256}" \
    "${COMPILER_GXX_SHA256}" \
    "${COMPILER_CPP_SHA256}" \
    "${COMPILER_AS_SHA256}" \
    "${COMPILER_LD_SHA256}" \
    "${COMPILER_OBJCOPY_SHA256}" \
    "${COMPILER_AR_SHA256}" \
    "${COMPILER_RANLIB_SHA256}" \
    "$(sha256_file "${BASH_SOURCE[0]}")" \
    "$(sha256_file "${PROFILE_CONTRACT}")" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
program_names = ("gcc", "g++", "cpp", "as", "ld", "objcopy", "ar", "ranlib")
record = {
    "schema": "cohesix-compiler-provenance/v1",
    "source": {
        "provider": "arm-gnu-toolchain-release-tarball",
        "url": sys.argv[2],
        "archive_path": str(Path(sys.argv[3]).resolve()),
        "archive_sha256": sys.argv[4],
        "archive_size": int(sys.argv[5]),
        "release": sys.argv[6],
    },
    "compiler": {
        "version": sys.argv[7],
        "target": sys.argv[8],
        "bin_path": str(Path(sys.argv[9]).resolve()),
        "program_sha256": dict(zip(program_names, sys.argv[10:18], strict=True)),
    },
    "setup_script_sha256": sys.argv[18],
    "profile_contract_sha256": sys.argv[19],
}
temporary = path.with_suffix(path.suffix + ".tmp")
temporary.write_text(
    json.dumps(record, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
temporary.replace(path)
PY

echo "Provisioning the isolated seL4 profile Python environment..."
require_sha256 "${PYTHON_BOOTSTRAP_LOCK}" "${PYTHON_BOOTSTRAP_LOCK_SHA256}"
require_sha256 "${PYTHON_REQUIREMENTS_LOCK}" "${PYTHON_REQUIREMENTS_LOCK_SHA256}"
guard_replace_dir "${PROFILE_VENV}"
remove_generated_dir "${PROFILE_VENV}"
"${HOMEBREW_PYTHON}" -m venv "${PROFILE_VENV}"
PROFILE_PYTHON="${PROFILE_VENV}/bin/python"
"${PROFILE_PYTHON}" -m pip install \
    --disable-pip-version-check --no-input --no-deps --require-hashes \
    --requirement "${PYTHON_BOOTSTRAP_LOCK}"
"${PROFILE_PYTHON}" -m pip install \
    --disable-pip-version-check --no-input --no-deps --no-build-isolation \
    --require-hashes --requirement "${PYTHON_REQUIREMENTS_LOCK}"
"${PROFILE_PYTHON}" - "${PYTHON_BOOTSTRAP_LOCK}" "${PYTHON_REQUIREMENTS_LOCK}" <<'PY'
import importlib.metadata
from pathlib import Path
import re
import sys

canonical = lambda value: re.sub(r"[-_.]+", "-", value).lower()
expected = {}
for lock_path in sys.argv[1:]:
    for line in Path(lock_path).read_text(encoding="utf-8").splitlines():
        match = re.match(r"^([A-Za-z0-9_.-]+)==([^ ]+)", line)
        if match:
            expected[canonical(match.group(1))] = match.group(2)
observed = {
    canonical(distribution.metadata["Name"]): distribution.version
    for distribution in importlib.metadata.distributions()
}
if observed != expected:
    raise SystemExit(
        f"profile Python distribution mismatch: expected {expected!r}, got {observed!r}"
    )
PY
"${PROFILE_PYTHON}" -c \
    'import google.protobuf, jsonschema, libarchive, lxml, pkg_resources, yaml'

echo "Building pinned mkimage from an immutable U-Boot source snapshot..."
if [[ ! -f "${UBOOT_SOURCE_ARCHIVE}" ]]; then
    curl --fail --location --proto '=https' --tlsv1.2 \
        --output "${UBOOT_SOURCE_ARCHIVE}.tmp" "${UBOOT_SOURCE_URL}"
    require_sha256 "${UBOOT_SOURCE_ARCHIVE}.tmp" "${UBOOT_SOURCE_SHA256}"
    if [[ "$(stat -f '%z' "${UBOOT_SOURCE_ARCHIVE}.tmp")" != "${UBOOT_SOURCE_SIZE}" ]]; then
        echo "Pinned U-Boot source archive size mismatch." >&2
        exit 1
    fi
    mv "${UBOOT_SOURCE_ARCHIVE}.tmp" "${UBOOT_SOURCE_ARCHIVE}"
fi
require_sha256 "${UBOOT_SOURCE_ARCHIVE}" "${UBOOT_SOURCE_SHA256}"
guard_replace_dir "${UBOOT_SNAPSHOT}"
guard_replace_dir "${UBOOT_BUILD}"
remove_generated_dir "${UBOOT_SNAPSHOT}"
remove_generated_dir "${UBOOT_BUILD}"
mkdir -p "${UBOOT_SNAPSHOT}" "${UBOOT_BUILD}"
tar -xjf "${UBOOT_SOURCE_ARCHIVE}" --strip-components=1 -C "${UBOOT_SNAPSHOT}"
GNU_MAKE="$(brew --prefix make)/bin/gmake"
OPENSSL_PREFIX="$(brew --prefix openssl@3)"
OPENSSL_PKG_CONFIG_PATH="${OPENSSL_PREFIX}/lib/pkgconfig"
OPENSSL_HOST_LDLIBS="$(PKG_CONFIG_PATH="${OPENSSL_PKG_CONFIG_PATH}" pkg-config --libs libssl libcrypto)"
if [[ ! -x "${GNU_MAKE}" ]]; then
    echo "Homebrew GNU make is unavailable: ${GNU_MAKE}" >&2
    exit 1
fi
SOURCE_DATE_EPOCH="1704067200" \
KBUILD_BUILD_USER="cohesix" \
KBUILD_BUILD_HOST="macos-arm64" \
PKG_CONFIG_PATH="${OPENSSL_PKG_CONFIG_PATH}" \
HOSTCFLAGS="-I${OPENSSL_PREFIX}/include" \
HOSTLDFLAGS="-L${OPENSSL_PREFIX}/lib" \
HOSTLDLIBS="${OPENSSL_HOST_LDLIBS}" \
    "${GNU_MAKE}" -C "${UBOOT_SNAPSHOT}" O="${UBOOT_BUILD}" \
        tools-only_defconfig
SOURCE_DATE_EPOCH="1704067200" \
KBUILD_BUILD_USER="cohesix" \
KBUILD_BUILD_HOST="macos-arm64" \
PKG_CONFIG_PATH="${OPENSSL_PKG_CONFIG_PATH}" \
HOSTCFLAGS="-I${OPENSSL_PREFIX}/include" \
HOSTLDFLAGS="-L${OPENSSL_PREFIX}/lib" \
HOSTLDLIBS="${OPENSSL_HOST_LDLIBS}" \
    "${GNU_MAKE}" -C "${UBOOT_SNAPSHOT}" O="${UBOOT_BUILD}" \
        -j"$(sysctl -n hw.logicalcpu)" tools-only
if [[ ! -x "${MKIMAGE}" ]]; then
    echo "Pinned mkimage build failed: ${MKIMAGE} is missing." >&2
    exit 1
fi
if [[ "$("${MKIMAGE}" -V 2>&1)" != "${MKIMAGE_VERSION}" ]]; then
    echo "Pinned mkimage version mismatch; expected ${MKIMAGE_VERSION}." >&2
    exit 1
fi
"${PROFILE_PYTHON}" - \
    "${MKIMAGE_PROVENANCE}" \
    "${UBOOT_SOURCE_URL}" \
    "${UBOOT_SOURCE_ARCHIVE}" \
    "${UBOOT_SOURCE_SHA256}" \
    "${UBOOT_SOURCE_SIZE}" \
    "${UBOOT_SOURCE_VERSION}" \
    "${UBOOT_SOURCE_COMMIT}" \
    "${MKIMAGE}" \
    "$(sha256_file "${MKIMAGE}")" \
    "${MKIMAGE_VERSION}" \
    "$(sha256_file "${BASH_SOURCE[0]}")" \
    "$(sha256_file "${PROFILE_CONTRACT}")" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
record = {
    "schema": "cohesix-mkimage-provenance/v1",
    "source": {
        "provider": "denx-release-tarball",
        "url": sys.argv[2],
        "archive_path": str(Path(sys.argv[3]).resolve()),
        "archive_sha256": sys.argv[4],
        "archive_size": int(sys.argv[5]),
        "version": sys.argv[6],
        "commit": sys.argv[7],
    },
    "mkimage": {
        "path": str(Path(sys.argv[8]).resolve()),
        "sha256": sys.argv[9],
        "version": sys.argv[10],
    },
    "setup_script_sha256": sys.argv[11],
    "profile_contract_sha256": sys.argv[12],
    "source_date_epoch": 1704067200,
}
temporary = path.with_suffix(path.suffix + ".tmp")
temporary.write_text(
    json.dumps(record, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
temporary.replace(path)
PY

if [[ -d /opt/homebrew/opt/llvm/bin ]]; then
    export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
        -y --profile minimal --default-toolchain "$RUST_TOOLCHAIN_VERSION"
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

echo "Ensuring Rust toolchain $RUST_TOOLCHAIN_VERSION is installed..."
rustup toolchain install "$RUST_TOOLCHAIN_VERSION" --profile minimal
rustup override set "$RUST_TOOLCHAIN_VERSION"

echo "Ensuring rustfmt and clippy are installed for $RUST_TOOLCHAIN_VERSION..."
rustup component add rustfmt clippy --toolchain "$RUST_TOOLCHAIN_VERSION"

echo "Ensuring target $RUST_TARGET is installed for $RUST_TOOLCHAIN_VERSION..."
rustup target add "$RUST_TARGET" --toolchain "$RUST_TOOLCHAIN_VERSION"

echo "Rust version: $(rustc --version)"

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "QEMU installation failed; qemu-system-aarch64 not in PATH." >&2
    exit 1
fi

for command_name in aarch64-none-elf-gcc aarch64-none-elf-g++ protoc repo; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "Toolchain setup failed; $command_name is not in PATH." >&2
        exit 1
    fi
done

echo "QEMU version: $(qemu-system-aarch64 --version | head -n1)"
echo "Cross compiler: $(aarch64-none-elf-gcc --version | head -n1)"
echo "Protobuf compiler: $(protoc --version)"
echo "Profile Python: $("${PROFILE_VENV}/bin/python" --version)"
echo "mkimage: $("${MKIMAGE}" -V 2>&1)"

echo "Toolchain setup complete."
