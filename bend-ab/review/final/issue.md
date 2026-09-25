Only if an issue is filed at all (the review recommends the PR alone, closing #1055).

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
2147483649  2147483648   F32.read("-7.0064923216240854e-46")
None        1065353216   F32.read("\u{a0}1")
23577.562   23577.563    F32.show(23577.5625)
```

All three causes are in the JS runtime in `bend2/comp.ts`. `f32_read` does `Math.fround(Number(s))`, two roundings, so a decimal whose double is exactly an f32 midpoint lands on the wrong side (#1055 is the overflow threshold). Its regex's `\s` and `Number()` take Unicode spaces that `strtof` refuses. `f32_show` uses `toExponential`, which rounds a decimal tie up where `printf` rounds to even; 2^23 f32 values print differently, all 8-digit ties. `WONTFIX.txt` names the C lane as the reference for F32.

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
    Unit <- IO.print(bits(F32.read("-7.0064923216240854e-46")))
    Unit <- IO.print(bits(F32.read("\u{a0}1")))
    IO.print(F32.show(23577.5625))
```

**bend --version:** bend 2.0.27 (3360764)
**uname -sm:** Linux x86_64 (same JS output on bun 1.3.11 and node 22)
**clang --version:** clang version 18.1.3
