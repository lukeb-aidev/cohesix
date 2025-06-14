# CLASSIFICATION: COMMUNITY
#!/bin/sh
# Filename: generate_busybox_man.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-06-08
set -e
OUTDIR=docs/man
mkdir -p "$OUTDIR"
for cmd in ls sed sh ps top mount umount ifconfig ping; do
    if command -v "$cmd" >/dev/null 2>&1; then
        page="$OUTDIR/$cmd.1"
        if [ ! -f "$page" ]; then
            HELP=$("$cmd" --help 2>&1 | head -n 20)
            {
                echo '.TH "'"$cmd"'" 1'
                echo '.SH NAME'
                echo "$cmd"
                echo '.SH SYNOPSIS'
                echo "$cmd [OPTIONS]"
                echo '.SH DESCRIPTION'
                echo "$HELP" | sed 's/\./\\&/g'
            } > "$page"
        fi
    fi
done
