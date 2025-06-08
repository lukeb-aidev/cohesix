#!/bin/sh
# Scan for manpages and verify rendering via mandoc
LOG=/var/log/manpage_check.log
: > "$LOG"
BASE=$(dirname "$0")/..
MANDOC="$BASE/bin/mandoc"
for dir in /usr/share/man "$BASE/docs/man" /etc/docs; do
    [ -d "$dir" ] || continue
    find "$dir" -type f \( -name '*.1' -o -name '*.5' -o -name '*.8' \) | while read -r f; do
        if "$MANDOC" -a "$f" >/tmp/man.out 2>&1; then
            head -n 20 /tmp/man.out >> "$LOG"
            echo "OK $f" >> "$LOG"
        else
            echo "FAIL $f" >> "$LOG"
        fi
    done
done
