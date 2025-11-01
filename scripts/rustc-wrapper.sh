#!/bin/sh
# Author: Lukas Bower
# Ensures rustc output directories exist before invocation to avoid
# APFS/iCloud reaping races when generating dep-info files.
set -eu

next_dep=0
next_out=0
next_out_dir=0
next_incremental=0
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
    if [ "$next_out_dir" -eq 1 ]; then
        mkdir -p "$arg"
        next_out_dir=0
        continue
    fi
    if [ "$next_incremental" -eq 1 ]; then
        mkdir -p "$arg"
        next_incremental=0
        continue
    fi
    case "$arg" in
        --dep-info)
            next_dep=1
            ;;
        -o)
            next_out=1
            ;;
        --out-dir)
            next_out_dir=1
            ;;
        --incremental)
            next_incremental=1
            ;;
        --out-dir=*)
            dir=${arg#--out-dir=}
            mkdir -p "$dir"
            ;;
        --incremental=*)
            dir=${arg#--incremental=}
            mkdir -p "$dir"
            ;;
    esac
done

exec "$@"
