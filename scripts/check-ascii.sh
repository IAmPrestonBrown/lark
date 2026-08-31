#!/bin/sh
# Rule C-1.1: every tracked file holds only ASCII bytes.
# A file listed in .ascii-exempt is skipped.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

fail=0
for f in $(sh scripts/lib-files.sh); do
    if [ -f .ascii-exempt ] && grep -qxF "$f" .ascii-exempt; then
        continue
    fi
    if LC_ALL=C grep -qP '[^\x00-\x7F]' "$f" 2>/dev/null; then
        echo "non-ascii: $f"
        LC_ALL=C grep -nP '[^\x00-\x7F]' "$f" | head -5 | sed 's/^/    /'
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    echo "FAIL check-ascii (rule C-1.1)"
    exit 1
fi
echo "ok   check-ascii"
