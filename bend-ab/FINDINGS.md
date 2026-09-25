# Bend findings

Found while porting mathom's pure core to Bend and checking every Bend lane
against mathom's Rust output. Each entry is written to fit Bend's bug form
(`.github/ISSUE_TEMPLATE/bug.yml`): what you did, what happened, the file,
`bend version`, `uname -sm`. Repro files are in `repros/`;
`scripts/repros.sh` runs them all.

Environment for every entry:

| | |
|---|---|
| bend | 2.0.27 (HigherOrderCO/Bend @ 3360764, 2026-09-25) |
| uname -sm | Linux x86_64 |
| clang | 18.1.3 |
| bun / node | 1.3.11 / 22.22.2 |

Bend's own gate runs its tests on Apple M4 minis only, so none of this
platform was covered upstream. The entries below are not platform-specific
unless stated.

Before filing, each entry was checked against `WONTFIX.txt`. The NaN payload
item (#797, #872), the JS deep-recursion item (#798, #802) and the Nat cap
(#779) are already listed there and are not repeated here, except where the
observed limit is far below the documented one.

## 1. F32 literals are double-rounded (all lanes)

`repros/f32_literal_rounding.bend`

The front end turns an F32 literal into bits with `Math.fround(Number(s))`
(`bend2/bend.ts:2321`). That rounds the decimal to a double, then the double
to f32. A decimal just above an f32 midpoint becomes exactly the midpoint as
a double, and ties-to-even then rounds it down.

| literal | Bend (C, JS, checker) | correctly rounded (Rust `parse::<f32>`, C `strtof`) |
|---|---|---|
| `16777217.00000000001` | 1266679808 (16777216) | 1266679809 (16777218) |
| `1.0000000596046447753906250001` | 1065353216 (1.0) | 1065353217 (1.0000001) |

Impact: the program text does not denote the float it spells. Every lane
agrees, so the gate cannot see it. Fix: parse to f32 with a correctly
rounded algorithm, or detect the exact-midpoint double and break the tie by
comparing the decimal to the midpoint.

## 2. `F32.read` disagrees between C and JS on rounding

`repros/f32_read_double_rounding.bend`

The JS twin (`f32_read`, `bend2/comp.ts:549`) has the same double rounding
as entry 1. The C twin uses `strtof` and rounds correctly. Same program,
different value:

| input | C | JS |
|---|---|---|
| `"16777217.00000000001"` | 1266679809 | 1266679808 |
| `"1.0000000596046447753906250001"` | 1065353217 | 1065353216 |

The checker lane cannot evaluate `F32.read` (it is an opaque law, so a pure
main leaves it stuck), so nothing arbitrates. The C answer is the IEEE one.

## 3. `F32.read` accepts Unicode whitespace on JS only

`repros/f32_read_unicode_space.bend`

The JS regex's `\s*` and `Number()` both accept NBSP (U+00A0), EM SPACE
(U+2003) and BOM (U+FEFF). C's `strtof` skips ASCII whitespace only.

| input | C | JS |
|---|---|---|
| `"\u{a0}1"` | None | Some 1 |
| `"\u{2003}2.5"` | None | Some 2.5 |
| `"\u{feff}3"` | None | Some 3 |

Fix: anchor the JS regex on `[ \t\n\v\f\r]*` and reject before `Number()`.

## 4. `F32.show` prints different digits on C and JS

`repros/f32_show_ties.bend`

When two shortest decimal strings both round-trip, the lanes pick
differently. C's `printf("%.*e")` rounds the decimal tie to even; JS's
`toExponential` rounds it up. In 200,000 fuzzed floats, 104 printed
differently (every other F32 op matched bit for bit, NaNs aside).

| value | C | JS (and `bend file.bend`) |
|---|---|---|
| 23577.5625 | `23577.562` | `23577.563` |
| 2335232.25 | `2335232.2` | `2335232.3` |
| -2983783.25 | `-2983783.2` | `-2983783.3` |

Since `bend file.bend` runs an IO main through the JS path, the same file
prints different text under `bend file.bend` and under its compiled binary.
Fix: make the JS twin round ties to even (or port the C algorithm).

## 5. A non-scalar `Char` kills the JS lane only

`repros/char_non_scalar.bend`

The theory's `Char` is `Chr{code: U32}`, and the checker and the C lane
accept any code. The JS lane's `char_new` throws on a surrogate or anything
past U+10FFFF, ending the whole run.

| | checker | C | JS |
|---|---|---|---|
| `String.length(SCon{Chr{55296}, SNil{}})` | 1n | 1 | dies |
| `IO.print` of `Chr{55296}` | n/a | `ED A0 80` (WTF-8) | dies: `bend: 55296 is not a Unicode scalar value` |
| `IO.print` of `Chr{1114112}` | n/a | `F4 90 80 80` (not UTF-8) | dies |

Either the type should rule these out (a `Char` law with a proof of
scalar-ness), or every lane should agree on one behaviour. As it stands, a
checked program that type-checks and runs on C crashes on JS. Mathom meets
this directly: NTFS file names are UTF-16 and may hold lone surrogates.

## 6. A 2,000-element list literal overflows the JS lane

`repros/js_list_literal_2000.bend`, `repros/js_list_literal_4000.bend`

A list literal compiles to one nested object expression
(`{$: "Con", head: .., tail: {$: "Con", ..}}`), which the engine evaluates
recursively.

| elements | C | bun | bun with the gate's 32 MB stack | node |
|---|---|---|---|---|
| 1,000 | ok | ok | ok | ok |
| 2,000 | ok | ok | ok | stack overflow |
| 4,000 | ok | stack overflow | stack overflow | stack overflow |
| 8,000 | the checker itself overflows | | | |

WONTFIX lists deep recursion on the JS lane (#798, #802) at "List.length over
200k elements". This is 50 to 100 times lower, comes from literal emission
rather than recursion in the program, and is cheap to fix independently
(emit a flat array and fold it into `Con` cells at load time). Any
table-driven test or lookup table written as a literal hits it.

## 7. The checker lane cannot print a 10k-character String

`repros/normalizer_long_string.bend`

A pure main is normalized by the checker and printed. `String.repeat("a",
10000n)` overflows the machine stack there (6,000 works). Clean error, low
impact, same family as #798. It limits how much a test can compare on the
checker lane.

## Not bugs, but worth knowing

| area | observation |
|---|---|
| Parallel speedup | `pow2` scales 4.0x on 4 threads. An irregular directory tree (the shape mathom handles) scales 1.0x to 1.24x, because a forked task is placed once and never moved. The guide documents this ("keep the workload balanced"); a real-world tree is never balanced. |
| C build time | 20 to 28 s to build a module holding 3,000 list literals (about 270 KB of source). |
| Error message | Calling a def declared below the caller reports "an unfilled law is a dead claim: live code cannot use it". The real cause is declaration order. |
| Destructuring | `(a, b) = f(x)` is rejected (a match on a computed value), and a tuple of `Data` is not `Data`, so it cannot be bound with `+`. Threading two results out of a recursive call needs a named record type and accessor defs. |
| Termination and kinds | About 15 classic attacks were all rejected with clear messages: argument swaps, self-calls hidden in lambdas, `do` continuations and binds, passing a def to its own template, negative datatypes, and function fields smuggled into `Data`. No soundness hole found. |
