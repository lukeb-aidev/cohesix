# CLASSIFICATION: COMMUNITY
# Filename: preview_man.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

manfile="docs/man/$1.1"
if [ ! -f "$manfile" ]; then
    manfile="docs/man/$1.8"
fi
if [ ! -f "$manfile" ]; then
    echo "manpage not found: $1" >&2
    exit 1
fi
mandoc "$manfile" | less
