#!/usr/bin/env python3
"""Turns benchmark rows into a table.

Each row is one run: collector, benchmark, milliseconds, allocations,
collections, heap bytes, checksum. The table puts one benchmark per row and one
collector per column, so a reader compares the collectors at a glance.

The checksum column is the point of the second table. Every collector runs the
same work, so every collector must report the same checksum. A row that
disagrees is a defect in a collector, not a slow result.
"""

import sys

ORDER = ["precise-marksweep", "arena", "semispace", "generational", "malloc"]
SHORT = {
    "precise-marksweep": "marksweep",
    "arena": "arena",
    "semispace": "semispace",
    "generational": "generational",
    "malloc": "malloc/free",
}


def read(stream):
    """Reads the rows, keyed by benchmark and then by collector."""
    results = {}
    order = []
    for line in stream:
        line = line.strip()
        if not line:
            continue
        parts = line.split("\t")
        if len(parts) != 7:
            print(f"bad row: {line}", file=sys.stderr)
            return None
        collector, name, ms, allocations, cycles, heap, checksum = parts
        if name not in results:
            results[name] = {}
            order.append(name)
        results[name][collector] = {
            "ms": float(ms),
            "allocations": int(allocations),
            "cycles": int(cycles),
            "heap": int(heap),
            "checksum": int(checksum),
        }
    return results, order


def columns(results):
    """Returns the collectors that appear, in a fixed order."""
    seen = set()
    for entry in results.values():
        seen.update(entry)
    return [name for name in ORDER if name in seen]


def print_table(title, results, order, names, pick, width=14):
    print()
    print(title)
    head = "".join(f"{SHORT[name]:>{width}}" for name in names)
    print(f"{'benchmark':<10}{head}")
    for name in order:
        cells = ""
        for collector in names:
            entry = results[name].get(collector)
            cells += f"{pick(entry):>{width}}" if entry else f"{'-':>{width}}"
        print(f"{name:<10}{cells}")


def main():
    parsed = read(sys.stdin)
    if parsed is None:
        return 1
    results, order = parsed
    names = columns(results)

    print_table("milliseconds, lower is better", results, order, names,
                lambda entry: f"{entry['ms']:.1f}")
    print_table("collections", results, order, names,
                lambda entry: str(entry["cycles"]))
    print_table("peak heap, KB", results, order, names,
                lambda entry: f"{entry['heap'] / 1024:.0f}")

    # Every collector runs the same work, so one checksum per benchmark.
    print()
    print("checksums, one value per row means the collectors agree")
    bad = 0
    for name in order:
        values = {entry["checksum"] for entry in results[name].values()}
        state = "ok" if len(values) == 1 else "DISAGREE"
        if len(values) != 1:
            bad += 1
        print(f"{name:<10}{state:>10}  {sorted(values)}")
    if bad:
        print(f"\n{bad} benchmark(s) disagree between collectors", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
