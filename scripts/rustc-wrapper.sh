#!/bin/sh
# Author: Lukas Bower
# Ensures rustc output directories exist before invocation to avoid
# APFS/iCloud reaping races when generating dep-info files.
set -eu

next_dep=0
next_out=0
for arg in "$@"; do
    if [ "$next_dep" -eq 1 ]; then
        dir=$(dirname "$arg")
        mkdir -p "$dir"
        next_dep=0
        continue
    fi
    if [ "$next_out" -eq 1 ]; then
        dir=$(dirname "$arg")
        mkdir -p "$dir"
        next_out=0
        continue
    fi
    case "$arg" in
        --dep-info)
            next_dep=1
            ;;
        -o)
            next_out=1
            ;;
    esac
done

exec "$@"
