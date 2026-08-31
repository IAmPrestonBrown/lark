#!/bin/sh
# Runs a command with a time limit.
#
# macOS has no `timeout`, so this script uses a background process and a poll.
# Usage: run-with-limit.sh <seconds> <command> [argument ...]

set -eu
limit=$1
shift

"$@" &
pid=$!

waited=0
while [ "$waited" -lt "$limit" ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
        wait "$pid"
        exit $?
    fi
    sleep 1
    waited=$((waited + 1))
done

kill -9 "$pid" 2>/dev/null || true
echo "the command did not finish in ${limit}s: $*" >&2
exit 124
