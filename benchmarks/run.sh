#!/bin/sh
# Runs every benchmark against every collector, and prints a table.
#
# Each run builds the same source with a different `gc.strategy`, which rule
# F-1 allows on the command line. Each collector writes to its own output
# directory, so one run does not overwrite the artifacts of another.
#
#   ./run.sh              the full workload
#   ./run.sh --quick      the small workload, which the gate uses
#   ./run.sh --only NAME  one benchmark
#
# Set LARK to the compiler to use. The default is `lark` on the PATH.

set -eu

LARK="${LARK:-lark}"
CC="${CC:-cc}"
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

collectors="precise-marksweep arena semispace generational"
benchmarks="trees churn walk barrier overhead"

quick=""
only=""
while [ $# -gt 0 ]; do
    case "$1" in
        --quick) quick="--quick" ;;
        --only) shift; only="$1" ;;
        *) echo "usage: $0 [--quick] [--only NAME]" >&2; exit 2 ;;
    esac
    shift
done

if [ -n "$only" ]; then
    benchmarks="$only"
fi

rows="$(mktemp)"
trap 'rm -f "$rows"' EXIT

for collector in $collectors; do
    out="build/$collector/"
    for name in $benchmarks; do
        printf '  %-18s %s\n' "$collector" "$name" >&2
        "$LARK" build "--gc.strategy=$collector" "--build.out=$out" "$name.lark" >/dev/null
        "$out$name" $quick >> "$rows"
    done
done

# The plain C half of the overhead pair. It links no runtime and no collector,
# so it runs once and stands as the baseline.
case " $benchmarks " in
    *" overhead "*)
        printf '  %-18s %s\n' "malloc" "overhead" >&2
        "$CC" -std=c11 -O2 -Wall -Wextra -o build/overhead_c overhead.c
        ./build/overhead_c $quick >> "$rows"
        ;;
esac

python3 table.py < "$rows"
