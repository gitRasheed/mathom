# bend-ab

An A/B harness between mathom's Rust core and ports of it to
[Bend](https://github.com/HigherOrderCO/Bend). Mathom's code is the oracle:
it runs over generated inputs, and the Bend ports must print the same lines
on every Bend lane. Where a Bend lane disagrees with Rust or with another
Bend lane, the difference is minimized to a repro in `repros/` and written
up in `FINDINGS.md`.

The Bend lanes:

| lane | how it runs | what it exercises |
|---|---|---|
| C | `bend main.bend -o main`, run natively at `--threads` 1, 4 and 16 | the C emitter, the flat heap, the fork-join scheduler |
| JS | `bend main.bend -o main.js`, run under bun | the JS emitter and its runtime |
| checker | a pure `main` normalized by `bend main.bend` | the theory itself (Base's definitions, not the intrinsics) |

## Results

| suite | mathom source | cases | C | JS | checker |
|---|---|---|---|---|---|
| runs | `scanner-ntfs/src/runs.rs` (`decode_runs`, `backed_clusters`) | 3,000 run lists, 64-bit edges, every error path | 3,000/3,000 | 3,000/3,000 | 400/400 |
| category | `core/src/category.rs` | 3,000 names: Unicode, NUL, emoji, 8 vs 9 UTF-8 byte extensions | 3,000/3,000 | 3,000/3,000 | 400/400 |
| tree | `core/src/tree.rs`, `search.rs`, `stats.rs` | seeded trees of 648 to 67,373 nodes; aggregates, type breakdown, top 20, 17 queries x 2 hide modes, search and overlay | exact, 1 to 64 threads, 18 runs | exact to 14k nodes; stack overflow at 67k (WONTFIX #798) | n/a |
| treemap | `core/src/treemap.rs` in F32 | 2,318 and 27,329 rects, bit-exact against an f32 mirror | exact at 1, 4, 16 threads | exact | n/a |
| ops | U32/Nat primitives, `U32.read`, `Nat.read` | 400 edge pairs x 25 ops, 19 parse inputs | 419/419 vs JS | 419/419 vs C | 400/400 on the 18 U32 ops (Nat ops are unary in the theory, too slow at these magnitudes) |

Every mismatch seen against Rust during porting was a porting mistake (two
fuel budgets set too low); none was Bend's. The Bend bugs, eight in all,
are in `FINDINGS.md`. They were found by fuzzing the primitives mathom's
logic depends on (F32 parsing and printing, `Char`, literals) and by running
Bend's own 1,464 tests on this Linux x86-64 box, which Bend's Mac-only gate
never covers. Two patches in `patches/` fix four of them, validated by
fuzzing and by Bend's tests.

| Bend's own suite here | |
|---|---|
| tests | 1,464 |
| failing | 5: three need ALSA headers (environment), two are FINDINGS 8 (JS timer order) |
| with `patches/` applied | the two timer tests pass; no regressions in the 198 IO and F32 tests |

Performance on the 67k-node tree workload (generate, aggregate, 34
searches and overlays, stats): Rust 0.30 s, Bend C 10.6 s at any thread
count. The Bend port is naive (strings are cons lists, lowercased per node
per query), so read the gap as an upper bound. Parallel speedup on this
irregular tree is 1.0x to 1.24x; `pow2`, which is balanced, scales 4.0x.

## Mathom findings

Building the oracle surfaced two behaviours in mathom itself:

| where | behaviour |
|---|---|
| `search.rs` `parse_min_size` | `>100` means at least 100 bytes, not more than 100: `>` and `>=` are the same filter. `>0` sets `min_size` to 0, which makes the query empty, so searching `>0` returns nothing. |
| `search.rs` `search`, `stats.rs` `largest_files` | The top-N heap evicts the smallest `(size, id)`, so among equal sizes at the cut-off it keeps the largest ids, then displays ties by smallest id first. With many equal sizes (0 bytes, or saturated totals) the kept set and the display order use opposite tie-breaks. |

## Layout

| path | contents |
|---|---|
| `oracle/` | Rust binary on mathom's crates: generates inputs, writes the Bend case module and the expected output |
| `ports/runs/runs.bend` | `runs.rs`; u64/i64 as a `W64{hi, lo}` pair of U32 |
| `ports/category/cat.bend` | `category.rs`; UTF-8 byte length computed from code points |
| `ports/tree/tree.bend` | tree generation (parallel), numbering, sizes, aggregates, stats, query parser, search, overlay |
| `ports/tree/treemap.bend` | `treemap.rs`, one Job-driven walker with parallel halves |
| `ports/ops/ops.bend` | primitive semantics fuzz |
| `repros/` | minimal Bend files for each finding |
| `scripts/run.sh` | the driver |
| `scripts/repros.sh` | runs every repro on every lane |
| `scripts/run_suite.py` | runs Bend's own `tests/` locally the way `gates/test.ts` does |
| `scripts/suite_report.py` | judges `run_suite.py` output with the gate's own exclusions |
| `patches/` | 0001 fixes FINDINGS 2, 3, 4 (the JS `F32.read` and `F32.show`); 0002 fixes FINDINGS 8 (timer order, both loops) |

## Running

Needs a Bend checkout, bun, clang and cargo.

```sh
export BEND_ROOT=/path/to/Bend
scripts/run.sh runs 3000
scripts/run.sh category 3000
scripts/run.sh interp runs 400 50
scripts/run.sh tree 3 14
scripts/run.sh treemap 3 13
scripts/run.sh ops
scripts/repros.sh
python3 scripts/run_suite.py "$BEND_ROOT" /tmp/bend-suite 3   # Bend's own tests, ~90 min on 4 cores
python3 scripts/suite_report.py /tmp/bend-suite/results.jsonl
```

Generated inputs land in `ports/*/gen*/` and builds in `ports/*/out/`,
both ignored by git.

## Porting notes

What made the port harder than the Rust, for anyone contributing Bend code
or docs:

| constraint | workaround used here |
|---|---|
| No 64-bit integers | `W64{hi, lo}` with explicit carries, saturation and signed overflow |
| A def can only call defs above it; no mutual recursion | one recursive def per loop, driven by a `Job` or state value; helpers above it decide the next state |
| `match` only on parameters and pattern fields | every computed branch gets its own def taking a `Bool` or `Cmp` |
| Termination is structural | `Nat` fuel on loops whose step is not structural, sized by hand |
| A pair of `Data` is not `Data` | small record types with accessor defs to reuse a recursive result |
| No `if` | `Bool.pick` (both arms evaluated) or a dispatch def |
