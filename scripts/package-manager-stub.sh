// CLASSIFICATION: COMMUNITY
// Filename: package-manager-stub.sh v0.3
// Date Modified: 2025-06-17
// Author: Lukas Bower

#!/usr/bin/env bash
###############################################################################
# package-manager-stub.sh – Cohesix prototype package tool
#
# Extremely minimal wrapper to install or remove pre‑built Cohesix packages
# from the local artefact store (`out/pkgs`).  Intended only as a placeholder
# until a full pkg‑db is implemented.
#
# Supported commands:
#   install <pkg.tar.gz>   – Extracts into /opt/cohesix/<pkg>
#   remove  <name>         – Deletes /opt/cohesix/<name>
#   list                   – Shows installed packages
#
# Example:
#   ./scripts/package-manager-stub.sh install out/pkgs/coh_hello-1.0.0-aarch64.tar.gz
#
#   ./scripts/package-manager-stub.sh list
#
# Exit codes:
#   0  Success
#   1  Invalid usage
#   2  Operation failed
###############################################################################
set -euo pipefail

PREFIX="/opt/cohesix"

msg() { printf "\e[32m[pkg]\e[0m %s\n" "$*"; }
err() { printf "\e[31m[err]\e[0m %s\n" "$*" >&2; exit 2; }

usage() {
  grep -E '^#' "$0" | sed -E 's/^#[ ]?//'
  exit 1
}

########## Parse CLI ##########################################################
[[ $# -eq 0 ]] && usage
CMD="$1"; shift

case "$CMD" in
  install)
    [[ $# -eq 1 ]] || usage
    TARBALL="$1"
    [[ -f $TARBALL ]] || err "Package not found: $TARBALL"
    PKG_NAME="$(basename "$TARBALL" .tar.gz)"
    DEST="$PREFIX/$PKG_NAME"
    msg "Installing $PKG_NAME → $DEST"
    sudo mkdir -p "$DEST"
    sudo tar -xzf "$TARBALL" -C "$DEST" --strip-components 1
    msg "✅ Installed $PKG_NAME"
    ;;
  remove)
    [[ $# -eq 1 ]] || usage
    PKG_NAME="$1"
    DEST="$PREFIX/$PKG_NAME"
    [[ -d $DEST ]] || err "Package not installed: $PKG_NAME"
    msg "Removing $PKG_NAME"
    sudo rm -rf "$DEST"
    msg "✅ Removed $PKG_NAME"
    ;;
  list)
    msg "Installed packages under $PREFIX:"
    ls -1 "$PREFIX" 2>/dev/null || true
    ;;
  *)
    usage ;;
esac