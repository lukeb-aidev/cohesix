#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build Linux ARM64 host tools and release archives on an explicitly selected remote builder.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

usage() {
  cat <<'USAGE'
Usage:
  scripts/linux_host_tools_sync.sh build-tools [options]
  scripts/linux_host_tools_sync.sh archive-bundle [options]

Common options:
  --host <host>                 Remote Linux ARM64 builder hostname or address
  --user <name>                 Remote SSH username
  --key <path>                  Optional SSH private key; omit for normal SSH agent/config auth

build-tools options:
  --remote-build-dir <path>     Absolute remote source/target/staging root
  --remote-cargo <path>         Absolute remote cargo executable
  --remote-cargo-home <path>    Absolute remote Cargo registry/cache directory
  --local-out <path>            Local destination for the exact host-tool set
  --manifest-out <path>         Local output path for build provenance JSON
  --max-glibc-version <x.y>     Maximum permitted GLIBC symbol version
  --no-clean                    Preserve the prior remote Cargo target cache

archive-bundle options:
  --remote-release-dir <path>   Absolute remote directory that retains the release tarball
  --bundle-dir <path>           Local assembled release directory to archive remotely
  --local-tarball <path>        Local destination for the remotely created tarball
  --force                       Replace an existing remote release tarball

No host, user, authentication, NVMe, cargo, build, release, or output location
is embedded in this script. Every environment-specific value is supplied by an
argument. The remote host must already contain the required build dependencies;
this release path never mutates apt sources or installs system packages.
USAGE
}

fail() {
  printf '[linux-builder] error: %s\n' "$*" >&2
  exit 1
}

log() {
  printf '[linux-builder] %s\n' "$*"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi
[[ $# -ge 1 ]] || { usage >&2; exit 1; }
MODE="$1"
shift

HOST=""
REMOTE_USER=""
KEY_PATH=""
REMOTE_BUILD_DIR=""
REMOTE_CARGO=""
REMOTE_CARGO_HOME=""
LOCAL_OUT=""
MANIFEST_OUT=""
MAX_GLIBC_VERSION=""
CLEAN=1
REMOTE_RELEASE_DIR=""
BUNDLE_DIR=""
LOCAL_TARBALL=""
FORCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      [[ $# -ge 2 ]] || fail "--host requires a value"
      HOST="$2"
      shift 2
      ;;
    --user)
      [[ $# -ge 2 ]] || fail "--user requires a value"
      REMOTE_USER="$2"
      shift 2
      ;;
    --key)
      [[ $# -ge 2 ]] || fail "--key requires a path"
      KEY_PATH="$2"
      shift 2
      ;;
    --remote-build-dir)
      [[ $# -ge 2 ]] || fail "--remote-build-dir requires a path"
      REMOTE_BUILD_DIR="$2"
      shift 2
      ;;
    --remote-cargo)
      [[ $# -ge 2 ]] || fail "--remote-cargo requires a path"
      REMOTE_CARGO="$2"
      shift 2
      ;;
    --remote-cargo-home)
      [[ $# -ge 2 ]] || fail "--remote-cargo-home requires a path"
      REMOTE_CARGO_HOME="$2"
      shift 2
      ;;
    --local-out)
      [[ $# -ge 2 ]] || fail "--local-out requires a path"
      LOCAL_OUT="$2"
      shift 2
      ;;
    --manifest-out)
      [[ $# -ge 2 ]] || fail "--manifest-out requires a path"
      MANIFEST_OUT="$2"
      shift 2
      ;;
    --max-glibc-version)
      [[ $# -ge 2 ]] || fail "--max-glibc-version requires a value"
      MAX_GLIBC_VERSION="$2"
      shift 2
      ;;
    --no-clean)
      CLEAN=0
      shift
      ;;
    --remote-release-dir)
      [[ $# -ge 2 ]] || fail "--remote-release-dir requires a path"
      REMOTE_RELEASE_DIR="$2"
      shift 2
      ;;
    --bundle-dir)
      [[ $# -ge 2 ]] || fail "--bundle-dir requires a path"
      BUNDLE_DIR="$2"
      shift 2
      ;;
    --local-tarball)
      [[ $# -ge 2 ]] || fail "--local-tarball requires a path"
      LOCAL_TARBALL="$2"
      shift 2
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[[ "$MODE" == "build-tools" || "$MODE" == "archive-bundle" ]] || \
  fail "mode must be build-tools or archive-bundle"
[[ -n "$HOST" ]] || fail "--host is required"
[[ -n "$REMOTE_USER" ]] || fail "--user is required"
[[ "$HOST" != -* && "$HOST" =~ ^[A-Za-z0-9._:-]+$ ]] || \
  fail "--host contains unsupported characters"
[[ "$REMOTE_USER" =~ ^[A-Za-z_][A-Za-z0-9._-]*$ ]] || \
  fail "--user contains unsupported characters"
if [[ -n "$KEY_PATH" ]]; then
  [[ -f "$KEY_PATH" && ! -L "$KEY_PATH" ]] || \
    fail "SSH key is missing or is a symlink: $KEY_PATH"
fi

validate_remote_dir() {
  local option="$1"
  local path="$2"
  [[ "$path" =~ ^/[A-Za-z0-9._/-]+$ ]] || \
    fail "${option} must be an absolute whitespace-free path"
  [[ "$path" != *"//"* && "$path" != */../* && "$path" != */.. ]] || \
    fail "${option} must be normalized and traversal-free"
  case "$path" in
    /|/bin|/boot|/dev|/etc|/home|/mnt|/opt|/root|/run|/srv|/tmp|/usr|/var)
      fail "${option} is too broad for managed release output: $path"
      ;;
  esac
}

validate_local_mutation_path() {
  local option="$1"
  local path="$2"
  [[ -n "$path" ]] || fail "${option} is required"
  python3 - "$ROOT_DIR" "$option" "$path" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
option = sys.argv[2]
path = Path(sys.argv[3]).expanduser().resolve(strict=False)
for forbidden in (Path("/"), Path.home().resolve(), root, root / "out", root / "releases"):
    if path == forbidden:
        raise SystemExit(f"{option} is too broad for managed output: {path}")
if path.exists() and path.is_symlink():
    raise SystemExit(f"{option} must not be a symlink: {path}")
PY
}

SSH_OPTS=(
  -o BatchMode=yes
  -o StrictHostKeyChecking=accept-new
)
if [[ -n "$KEY_PATH" ]]; then
  SSH_OPTS+=(-i "$KEY_PATH")
fi
SSH_DESTINATION="${REMOTE_USER}@${HOST}"

run_ssh() {
  ssh "${SSH_OPTS[@]}" "$SSH_DESTINATION" "$@"
}

sha256_file() {
  python3 - "$1" <<'PY'
from pathlib import Path
import hashlib
import sys

digest = hashlib.sha256()
with Path(sys.argv[1]).open("rb") as handle:
    while chunk := handle.read(1024 * 1024):
        digest.update(chunk)
print(digest.hexdigest())
PY
}

build_tools() {
  if [[ -n "$REMOTE_RELEASE_DIR" || -n "$BUNDLE_DIR" || -n "$LOCAL_TARBALL" || "$FORCE" -ne 0 ]]; then
    fail "archive-bundle options are not valid with build-tools"
  fi
  [[ -n "$REMOTE_BUILD_DIR" ]] || fail "--remote-build-dir is required"
  [[ -n "$REMOTE_CARGO" ]] || fail "--remote-cargo is required"
  [[ -n "$REMOTE_CARGO_HOME" ]] || fail "--remote-cargo-home is required"
  [[ -n "$LOCAL_OUT" ]] || fail "--local-out is required"
  [[ -n "$MANIFEST_OUT" ]] || fail "--manifest-out is required"
  [[ "$REMOTE_CARGO" =~ ^/[A-Za-z0-9._/-]+$ ]] || \
    fail "--remote-cargo must be an absolute whitespace-free path"
  [[ "$MAX_GLIBC_VERSION" =~ ^[0-9]+\.[0-9]+$ ]] || \
    fail "--max-glibc-version must have x.y form"
  validate_remote_dir "--remote-build-dir" "$REMOTE_BUILD_DIR"
  validate_remote_dir "--remote-cargo-home" "$REMOTE_CARGO_HOME"
  validate_local_mutation_path "--local-out" "$LOCAL_OUT"
  validate_local_mutation_path "--manifest-out" "$MANIFEST_OUT"

  local source_status
  source_status="$(git -C "$ROOT_DIR" status --porcelain=v1 --untracked-files=all)"
  [[ -z "$source_status" ]] || \
    fail "release host tools require a clean source checkout"

  local temp_dir
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "${temp_dir}"' RETURN
  local source_tarball="${temp_dir}/cohesix-host-tools-source.tar.gz"
  local remote_source_tarball="${REMOTE_BUILD_DIR}/cohesix-host-tools-source.tar.gz"
  local remote_tools_tarball="${REMOTE_BUILD_DIR}/host-tools-linux.tar.gz"
  local remote_build_info="${REMOTE_BUILD_DIR}/host-tools-build-info.env"
  local source_commit
  source_commit="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)"

  log "Packaging the exact clean host-tool source set"
  (
    cd "$ROOT_DIR"
    {
      printf '%s\0' \
        Cargo.toml \
        Cargo.lock \
        rust-toolchain.toml \
        .cargo/config.toml \
        configs/generated/cohesix_python_qemu_smp_production.json \
        scripts/rustc-wrapper.sh
      git ls-files -z --cached apps crates tools tests resources
    } | COPYFILE_DISABLE=1 tar --no-xattrs --null -T - -czf "$source_tarball"
  )
  local source_sha256
  source_sha256="$(sha256_file "$source_tarball")"

  run_ssh "mkdir -p '${REMOTE_BUILD_DIR}'"
  scp "${SSH_OPTS[@]}" "$source_tarball" \
    "${SSH_DESTINATION}:${remote_source_tarball}"

  log "Building the exact Linux ARM64 host-tool set on ${HOST}"
  ssh "${SSH_OPTS[@]}" "$SSH_DESTINATION" bash -s -- \
    "$REMOTE_BUILD_DIR" "$REMOTE_CARGO" "$REMOTE_CARGO_HOME" \
    "$MAX_GLIBC_VERSION" "$source_sha256" "$source_commit" "$CLEAN" <<'REMOTE_BUILD'
set -euo pipefail

build_root="$1"
cargo_bin="$2"
cargo_home="$3"
max_glibc="$4"
expected_source_sha="$5"
source_commit="$6"
clean="$7"
source_tarball="${build_root}/cohesix-host-tools-source.tar.gz"
source_dir="${build_root}/source"
target_dir="${build_root}/target"
stage_dir="${build_root}/host-tools-linux"
tools_tarball="${build_root}/host-tools-linux.tar.gz"
build_info="${build_root}/host-tools-build-info.env"
rustc_bin="${cargo_bin%/*}/rustc"

[[ "$(uname -s)" == "Linux" ]] || { echo "remote builder is not Linux" >&2; exit 1; }
case "$(uname -m)" in
  aarch64|arm64) ;;
  *) echo "remote builder is not ARM64: $(uname -m)" >&2; exit 1 ;;
esac
[[ -x "$cargo_bin" ]] || { echo "remote cargo is not executable: $cargo_bin" >&2; exit 1; }
[[ -x "$rustc_bin" ]] || { echo "remote rustc is not executable beside cargo: $rustc_bin" >&2; exit 1; }
for command in awk file grep install python3 sed sha256sum sort strings tar; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "remote builder is missing required command: $command" >&2
    exit 1
  }
done

actual_source_sha="$(sha256sum "$source_tarball" | awk '{print $1}')"
[[ "$actual_source_sha" == "$expected_source_sha" ]] || {
  echo "remote source archive digest mismatch" >&2
  exit 1
}

if [[ "$clean" -eq 1 ]]; then
  rm -rf "$source_dir" "$target_dir" "$stage_dir"
fi
mkdir -p "$source_dir" "$target_dir" "$stage_dir"
mkdir -p "$cargo_home"
rm -rf "${source_dir:?}"/* "${source_dir}"/.[!.]* "${source_dir}"/..?* 2>/dev/null || true
tar -xzf "$source_tarball" -C "$source_dir"

expected_toolchain="$(awk -F'"' \
  '/^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' \
  "$source_dir/rust-toolchain.toml")"
[[ -n "$expected_toolchain" ]] || {
  echo "tracked rust-toolchain.toml omits toolchain.channel" >&2
  exit 1
}
actual_cargo_version="$("$cargo_bin" --version | awk '{print $2}')"
actual_rustc_version="$("$rustc_bin" --version | awk '{print $2}')"
[[ "$actual_cargo_version" == "$expected_toolchain" ]] || {
  echo "remote cargo ${actual_cargo_version} does not match pinned ${expected_toolchain}" >&2
  exit 1
}
[[ "$actual_rustc_version" == "$expected_toolchain" ]] || {
  echo "remote rustc ${actual_rustc_version} does not match pinned ${expected_toolchain}" >&2
  exit 1
}

export PATH="${cargo_bin%/*}:${PATH}"
export CARGO_HOME="$cargo_home"
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$target_dir"
cd "$source_dir"

"$cargo_bin" build --release -p gpu-bridge-host
"$cargo_bin" build --release -p cas-tool
"$cargo_bin" build --release -p hive-gateway
"$cargo_bin" build --release -p host-ticket-agent
"$cargo_bin" build --release -p host-sidecar-bridge --features tcp
"$cargo_bin" build --release -p cohsh --features tcp
"$cargo_bin" build --release -p coh --features fuse,nvml
RUSTFLAGS='-C debuginfo=0' "$cargo_bin" build --release -p swarmui

bins=(cohsh coh gpu-bridge-host host-sidecar-bridge cas-tool swarmui hive-gateway host-ticket-agent)
rm -rf "$stage_dir"
mkdir -p "$stage_dir"
for bin in "${bins[@]}"; do
  source_path="${target_dir}/release/${bin}"
  [[ -x "$source_path" ]] || { echo "missing expected binary: $source_path" >&2; exit 1; }
  description="$(file -b "$source_path")"
  echo "$description" | grep -Eiq 'ELF.*(ARM aarch64|aarch64)' || {
    echo "wrong Linux ARM64 binary kind for ${bin}: ${description}" >&2
    exit 1
  }
  version="$(strings "$source_path" | grep -o 'GLIBC_[0-9\.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1 || true)"
  if [[ -n "$version" ]]; then
    worst="$(printf '%s\n%s\n' "$max_glibc" "$version" | sort -V | tail -n 1)"
    [[ "$worst" == "$max_glibc" ]] || {
      echo "${bin} requires GLIBC_${version}; maximum is GLIBC_${max_glibc}" >&2
      exit 1
    }
  fi
  install -m 0755 "$source_path" "${stage_dir}/${bin}"
done

tar -C "$build_root" -czf "$tools_tarball" host-tools-linux
os_id="$(. /etc/os-release && printf '%s' "${ID:-unknown}")"
os_version="$(. /etc/os-release && printf '%s' "${VERSION_ID:-unknown}")"
{
  printf 'schema=cohesix-linux-host-tools-build/v1\n'
  printf 'source_commit=%s\n' "$source_commit"
  printf 'source_archive_sha256=%s\n' "$actual_source_sha"
  printf 'architecture=%s\n' "$(uname -m)"
  printf 'os_id=%s\n' "$os_id"
  printf 'os_version=%s\n' "$os_version"
  printf 'toolchain_channel=%s\n' "$expected_toolchain"
  printf 'cargo_version=%s\n' "$("$cargo_bin" --version)"
  printf 'rustc_version=%s\n' "$("$rustc_bin" --version)"
  printf 'rustc_host=%s\n' "$("$rustc_bin" -vV | awk '/^host:/ {print $2}')"
  printf 'max_glibc_version=%s\n' "$max_glibc"
  printf 'built_at_utc=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
} >"$build_info"
REMOTE_BUILD

  scp "${SSH_OPTS[@]}" "${SSH_DESTINATION}:${remote_tools_tarball}" \
    "${temp_dir}/host-tools-linux.tar.gz"
  scp "${SSH_OPTS[@]}" "${SSH_DESTINATION}:${remote_build_info}" \
    "${temp_dir}/host-tools-build-info.env"

  mkdir -p "${temp_dir}/extract"
  tar -xzf "${temp_dir}/host-tools-linux.tar.gz" -C "${temp_dir}/extract"
  local extracted="${temp_dir}/extract/host-tools-linux"
  [[ -d "$extracted" && ! -L "$extracted" ]] || \
    fail "remote host-tool archive omitted its exact staging directory"

  python3 - "$extracted" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
expected = {
    "cas-tool",
    "coh",
    "cohsh",
    "gpu-bridge-host",
    "hive-gateway",
    "host-sidecar-bridge",
    "host-ticket-agent",
    "swarmui",
}
actual = {path.name for path in root.iterdir() if path.is_file() and not path.is_symlink()}
if actual != expected:
    raise SystemExit(
        f"remote host-tool set drift: missing={sorted(expected - actual)} "
        f"unexpected={sorted(actual - expected)}"
    )
if any(not path.is_file() or path.is_symlink() for path in root.iterdir()):
    raise SystemExit("remote host-tool archive contains non-regular entries")
PY

  local out_parent
  out_parent="$(dirname "$LOCAL_OUT")"
  mkdir -p "$out_parent"
  local replacement="${out_parent}/.$(basename "$LOCAL_OUT").new.$$"
  local backup="${out_parent}/.$(basename "$LOCAL_OUT").old.$$"
  rm -rf "$replacement" "$backup"
  mv "$extracted" "$replacement"
  if [[ -e "$LOCAL_OUT" ]]; then
    mv "$LOCAL_OUT" "$backup"
  fi
  mv "$replacement" "$LOCAL_OUT"
  rm -rf "$backup"

  mkdir -p "$(dirname "$MANIFEST_OUT")"
  python3 - "$LOCAL_OUT" "${temp_dir}/host-tools-build-info.env" "$MANIFEST_OUT" <<'PY'
from pathlib import Path
import hashlib
import json
import subprocess
import sys

tools = Path(sys.argv[1])
info_path = Path(sys.argv[2])
manifest_path = Path(sys.argv[3])
info: dict[str, str] = {}
for line in info_path.read_text(encoding="utf-8").splitlines():
    key, separator, value = line.partition("=")
    if not separator or not key or key in info:
        raise SystemExit(f"invalid remote build-info line: {line!r}")
    info[key] = value
if info.get("schema") != "cohesix-linux-host-tools-build/v1":
    raise SystemExit("remote build-info schema is invalid")

artifacts = []
for path in sorted(tools.iterdir(), key=lambda item: item.name):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    description = subprocess.run(
        ["file", "-b", str(path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if "ELF" not in description or not (
        "ARM aarch64" in description or "aarch64" in description.lower()
    ):
        raise SystemExit(f"wrong downloaded binary kind for {path.name}: {description}")
    artifacts.append(
        {
            "filename": path.name,
            "sha256": digest,
            "size_bytes": path.stat().st_size,
            "file_description": description,
        }
    )

payload = {
    "schema": info.pop("schema"),
    "builder": info,
    "source_tree_clean": True,
    "artifacts": artifacts,
}
manifest_path.write_text(
    json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
PY

  log "Linux host tools: ${LOCAL_OUT}"
  log "Linux build provenance: ${MANIFEST_OUT}"
}

archive_bundle() {
  if [[ -n "$REMOTE_BUILD_DIR" || -n "$REMOTE_CARGO" || -n "$REMOTE_CARGO_HOME" || -n "$LOCAL_OUT" || -n "$MANIFEST_OUT" || -n "$MAX_GLIBC_VERSION" || "$CLEAN" -ne 1 ]]; then
    fail "build-tools options are not valid with archive-bundle"
  fi
  [[ -n "$REMOTE_RELEASE_DIR" ]] || fail "--remote-release-dir is required"
  [[ -n "$BUNDLE_DIR" ]] || fail "--bundle-dir is required"
  [[ -n "$LOCAL_TARBALL" ]] || fail "--local-tarball is required"
  validate_remote_dir "--remote-release-dir" "$REMOTE_RELEASE_DIR"
  validate_local_mutation_path "--local-tarball" "$LOCAL_TARBALL"
  [[ -d "$BUNDLE_DIR" && ! -L "$BUNDLE_DIR" ]] || \
    fail "--bundle-dir must select a regular directory"

  local bundle_name
  bundle_name="$(basename "$BUNDLE_DIR")"
  [[ "$bundle_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || \
    fail "bundle directory name contains unsupported characters"
  local bundle_parent
  bundle_parent="$(cd "$(dirname "$BUNDLE_DIR")" && pwd)"
  local remote_bundle="${REMOTE_RELEASE_DIR}/${bundle_name}"
  local remote_appledouble="${REMOTE_RELEASE_DIR}/._${bundle_name}"
  local remote_tarball="${REMOTE_RELEASE_DIR}/${bundle_name}.tar.gz"

  log "Uploading the assembled Linux bundle for remote release archiving"
  if [[ "$FORCE" -eq 1 ]]; then
    run_ssh "mkdir -p '${REMOTE_RELEASE_DIR}' && rm -rf '${remote_bundle}' && rm -f '${remote_appledouble}' '${remote_tarball}'"
  else
    run_ssh "mkdir -p '${REMOTE_RELEASE_DIR}' && if [ -e '${remote_tarball}' ]; then echo 'remote release tarball already exists: ${remote_tarball}' >&2; exit 1; fi && rm -rf '${remote_bundle}' && rm -f '${remote_appledouble}'"
  fi
  COPYFILE_DISABLE=1 tar --no-xattrs -C "$bundle_parent" -cf - "$bundle_name" | \
    run_ssh "tar -C '${REMOTE_RELEASE_DIR}' -xf -"
  run_ssh "tar -C '${REMOTE_RELEASE_DIR}' -czf '${remote_tarball}' '${bundle_name}'"
  run_ssh "rm -rf '${remote_bundle}' && rm -f '${remote_appledouble}'"

  local temp_dir
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "${temp_dir}"' RETURN
  scp "${SSH_OPTS[@]}" "${SSH_DESTINATION}:${remote_tarball}" \
    "${temp_dir}/${bundle_name}.tar.gz"

  python3 - "$BUNDLE_DIR" "${temp_dir}/${bundle_name}.tar.gz" <<'PY'
from pathlib import Path, PurePosixPath
import hashlib
import sys
import tarfile

bundle = Path(sys.argv[1])
archive = Path(sys.argv[2])
expected = {
    path.relative_to(bundle).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
    for path in bundle.rglob("*")
    if path.is_file() and not path.is_symlink()
}
actual: dict[str, str] = {}
with tarfile.open(archive, "r:gz") as handle:
    for member in handle.getmembers():
        parts = PurePosixPath(member.name).parts
        if not parts or parts[0] != bundle.name or ".." in parts:
            raise SystemExit(f"remote archive contains an invalid path: {member.name}")
        if member.issym() or member.islnk():
            raise SystemExit(f"remote archive contains a link: {member.name}")
        if member.isdir():
            continue
        if not member.isfile():
            raise SystemExit(f"remote archive contains a special entry: {member.name}")
        relative = PurePosixPath(*parts[1:]).as_posix()
        if relative in actual:
            raise SystemExit(f"remote archive contains a duplicate file: {member.name}")
        stream = handle.extractfile(member)
        if stream is None:
            raise SystemExit(f"remote archive member cannot be read: {member.name}")
        actual[relative] = hashlib.sha256(stream.read()).hexdigest()
if actual != expected:
    raise SystemExit(
        "remote release archive differs from the assembled bundle: "
        f"missing={sorted(expected.keys() - actual.keys())} "
        f"unexpected={sorted(actual.keys() - expected.keys())}"
    )
for relative, digest in expected.items():
    if actual[relative] != digest:
        raise SystemExit(f"remote archive digest mismatch: {relative}")
PY

  mkdir -p "$(dirname "$LOCAL_TARBALL")"
  mv "${temp_dir}/${bundle_name}.tar.gz" "$LOCAL_TARBALL"
  log "Remote release tarball retained at ${HOST}:${remote_tarball}"
  log "Verified local release tarball: ${LOCAL_TARBALL}"
}

case "$MODE" in
  build-tools)
    build_tools
    ;;
  archive-bundle)
    archive_bundle
    ;;
esac
