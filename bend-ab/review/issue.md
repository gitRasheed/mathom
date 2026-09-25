# Issue (fits .github/ISSUE_TEMPLATE/bug.yml)

**Title:** The JS F32.read rounds a decimal twice and takes a Unicode space, and the JS F32.show rounds a decimal tie up: the C lane reads and prints the same program differently

## What you did

```
bend main.bend -o main -o main.js && ./main && bun main.js && bend main.bend
```

## What happened

The C binary, the JS build and `bend main.bend` (which runs an IO main through the JS path) print, side by side:

```
C          JS / bend main.bend
1266679809 1266679808            F32.read("16777217.00000000001")
3212836865 3212836864            F32.read("-1.0000000596046447753906250001")
2139095039 2139095040            F32.read("340282356779733661637539395458142568447")
1          0                     F32.read("7.0064923216240854e-46")
None       1065353216            F32.read("\u{a0}1")
23577.562  23577.563             F32.show(23577.5625)
-2983783.2 -2983783.3            F32.show(-2983783.25)
```

Three causes, all in the JS runtime text of `bend2/comp.ts`:

1. `f32_read` (comp.ts:546) computes `Math.fround(Number(s))`: the decimal is rounded to a double, then the double to an f32. When the double lands exactly on an f32 midpoint, the second rounding goes to even and loses the side the decimal was on. The C `f32_read` (comp.ts:477) calls `strtof`, which rounds once. `16777217.00000000001` is above the midpoint 16777217, so strtof reads 16777218; its double is exactly 16777217, so fround reads 16777216. The same double rounding takes the largest decimal under the overflow threshold (2^128 - 2^103) to inf (#1055 is that instance) and the first decimal past 2^-150 to 0 instead of the smallest subnormal.
2. The same regex anchors on `\s*`, which in JS takes NBSP (U+00A0), EM SPACE (U+2003), the BOM (U+FEFF) and every other Unicode space, and `Number()` takes them too. `strtof` skips `isspace` only (` \t\n\v\f\r`), so the C lane answers None.
3. `f32_show` (comp.ts:523) picks its digits with `toExponential`, which rounds a decimal tie away from zero. The C `f32_text` (comp.ts:442) prints with `%.*e`, which rounds a tie to even. When both candidates read back to the same f32 (a tie at 7 to 9 significant digits), the two lanes print different digits. Over every f32 whose value is such a tie (74,525,948 bit patterns, both signs), the lanes differ on 8,388,608 (2^23) of them, every one an 8-digit tie between 2^-12 and 4194303.25; a random float is one rarely (104 of 200,000), which is why the pins never caught it. `tests/run/float_text.bend` notes the emulation holds "past exact ties".

WONTFIX names the C lane as the reference for F32. None of the three is listed there, and #801 fixed the reverse direction (the C read accepting what JS rejects).

## The file

```python
import Base

def bits(m: Maybe<&2, F32>) -> String:
  match m:
    case None{}:
      "None"
    case Some{v}:
      U32.show(F32.bits(v))

def main() -> IO(Unit):
  do IO<Unit>:
    Unit <- IO.print(bits(F32.read("16777217.00000000001")))
    Unit <- IO.print(bits(F32.read("-1.0000000596046447753906250001")))
    Unit <- IO.print(bits(F32.read("340282356779733661637539395458142568447")))
    Unit <- IO.print(bits(F32.read("7.0064923216240854e-46")))
    Unit <- IO.print(bits(F32.read("\u{a0}1")))
    Unit <- IO.print(F32.show(23577.5625))
    IO.print(F32.show(F32.neg(2983783.25)))
```

## bend --version

bend 2.0.27 (HigherOrderCO/Bend at 3360764)

## uname -sm

Linux x86_64 (the JS side is the same under bun 1.3.11 and node 22.22.2; the C side is glibc's strtof and printf, which round as macOS's do)

## clang --version (the first line)

Ubuntu clang version 18.1.3 (1ubuntu1)
