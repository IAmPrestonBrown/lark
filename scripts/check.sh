#!/bin/sh
# The gate. Run this before every push. See docs/conventions.md section 6.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

status=0
run() {
    label=$1
    shift
    if "$@"; then
        :
    else
        echo "FAILED: $label"
        status=1
    fi
}

echo "== text =="
run "check-ascii"       sh scripts/check-ascii.sh
run "check-prose"       python3 scripts/check-prose.py
run "check-attribution" sh scripts/check-attribution.sh

if [ -f Cargo.toml ]; then
    echo "== rust =="
    run "cargo fmt"    cargo fmt --all --check
    run "cargo clippy" cargo clippy --workspace --all-targets --all-features -- -D warnings
    run "cargo test"   cargo test --workspace
    # A broken documentation link points a reader at nothing, and a link to a
    # private item usually means the item belongs in the public API.
    run "cargo doc"    env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
else
    echo "== rust =="
    echo "skip check-rust: no Cargo.toml yet"
fi

if [ -f runtime/Makefile ]; then
    echo "== runtime =="
    # `check-all` runs the whole suite against every collector, because a
    # program links exactly one and each one must satisfy the same seam.
    run "runtime build" make -C runtime check-all
else
    echo "== runtime =="
    echo "skip check-runtime: no runtime/Makefile yet"
fi

if [ -x benchmarks/run.sh ]; then
    echo "== benchmarks =="
    # The small workload, which takes a few seconds. The gate does not compare
    # timings, because a shared machine gives no stable number. It checks that
    # every benchmark still builds against every collector, and that the four
    # collectors return the same checksum for the same work.
    # The driver needs the compiler, and `cargo test` does not always build
    # the binary itself.
    run "benchmark build" cargo build -p lark-cli
    run "benchmarks" env LARK="$root/target/debug/lark" sh benchmarks/run.sh --quick
else
    echo "== benchmarks =="
    echo "skip check-benchmarks: no benchmarks/run.sh yet"
fi

if [ "$status" -ne 0 ]; then
    echo
    echo "GATE FAILED"
    exit 1
fi
echo
echo "GATE PASSED"
