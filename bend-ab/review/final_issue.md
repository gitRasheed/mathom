**Title:** JS and C disagree on F32.read and F32.show

**What you did**

```
bend main.bend -o main -o main.js && ./main && bun main.js
```

**What happened**

```
C           JS
1266679809  1266679808   F32.read("16777217.00000000001")
2139095039  2139095040   F32.read("340282356779733661637539395458142568447")
1           0            F32.read("7.0064923216240854e-46")
None        1065353216   F32.read("\u{a0}1")
23577.562   23577.563    F32.show(23577.5625)
```

Three causes, all in the JS runtime in `bend2/comp.ts`:

1. `f32_read` does `Math.fround(Number(s))`, which rounds twice. When the double lands exactly on an f32 midpoint, the second rounding picks the wrong side. C uses `strtof`, which rounds once. #1055 is one case of this.
2. The regex's `\s` and `Number()` accept Unicode spaces (NBSP, EM SPACE, BOM). `strtof` only skips ASCII whitespace.
3. `f32_show` uses `toExponential`, which rounds ties up. C's `printf` rounds ties to even. Out of 74,525,948 floats that sit on a decimal tie, 8,388,608 print differently.

`WONTFIX.txt` says the C lane is the reference for F32, and none of these are listed there.

**The file**

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
    Unit <- IO.print(bits(F32.read("340282356779733661637539395458142568447")))
    Unit <- IO.print(bits(F32.read("7.0064923216240854e-46")))
    Unit <- IO.print(bits(F32.read("\u{a0}1")))
    IO.print(F32.show(23577.5625))
```

**bend version:** 2.0.27 (3360764)
**uname -sm:** Linux x86_64 (same JS output on bun 1.3.11 and node 22)
