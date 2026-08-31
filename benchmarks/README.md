# Benchmarks

These programs answer two questions. What does each collector cost on the same
work, and what does managed memory cost against plain C?

```sh
./run.sh              # the full workload, a minute or two
./run.sh --quick      # the small workload, which the gate runs
./run.sh --only churn # one benchmark
```

Set `LARK` to the compiler to use. The default is `lark` on the `PATH`.

## What each one measures

| Benchmark | What it stresses |
|---|---|
| `trees` | Allocation and tracing of a large live graph. |
| `churn` | Many short lived objects. This is what a generational collector is for. |
| `walk` | One long list, read many times. A moving collector puts it back in order. |
| `barrier` | Stores of a young pointer into an old object. This is the price of rule R-2. |
| `overhead` | The same work managed and with `malloc`. The gap is what `gc` costs. |

`overhead.c` holds the plain C half. It links no runtime and no collector, so
it is the baseline that the managed runs compare against.

## How to read the output

The table gives milliseconds, collections, and peak heap, one row per benchmark
and one column per collector.

The last table is the important one. Every collector runs the same work, so
every collector must report the same checksum. A row that disagrees is a defect
in a collector, not a slow result. The gate runs `--quick` for that reason: it
compares no timings, because a shared machine gives no stable number, but it
does check that every benchmark still builds and that the collectors agree.

## Reading a number honestly

A benchmark measures one machine on one day. Compare two collectors in the same
run, not two runs on different machines. The shape of the result is what
matters: `churn` favors a generational collector, `walk` favors one that moves,
and `barrier` charges the generational collector for what `churn` pays it.

Every benchmark builds with `opt = "2"` and `debug = false`, which
`benchmarks/lark.toml` sets. The default build is a debug build, and measuring
one of those measures nothing.

## Adding one

1. Write `<name>.lark`. Import `bench`, and end with `bench::bench_report`.
2. Take two workload sizes through `bench::bench_scale`, a full one and a small
   one. The gate runs the small one.
3. Return a checksum that depends on all the work. A collector that disagrees
   with the others then fails loudly.
4. Add the name to `benchmarks` in `run.sh`.
