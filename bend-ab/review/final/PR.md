**Title:** F32.read and F32.show give the C lane's answer on JS

#1055 is one case of a wider difference between the JS and C lanes. JS read a float with `Math.fround(Number(s))`, which rounds twice: when the double lands exactly on an f32 midpoint, the second rounding can go the wrong way. C's `strtof` rounds once. Two more differences in the same functions:

- `F32.read` skipped any Unicode space before the number (`\s`), where `strtof` only skips ASCII whitespace.
- `F32.show` rounded a decimal tie up (`toExponential`), where C's `printf` rounds it to even.

```
                                          C           JS on main
F32.read("16777217.00000000001")          1266679809  1266679808
F32.read("340282356779733661637539395458142568447")
                                          2139095039  2139095040   (#1055)
F32.read("-7.0064923216240854e-46")       2147483649  2147483648
F32.read("\u{a0}1")                       None        1065353216
F32.show(23577.5625)                      23577.562   23577.563
```

`F32.read` now compares the decimal with the midpoint exactly (BigInt) when the double lands on one, and only skips ASCII whitespace. `F32.show` checks for a tie once, at the precision it stops at, and rounds it to even. Only the JS runtime changes.

Test plan
- [x] `tests/run/float_text_lanes.bend` prints the same line on the interpreter, JS and C; on main the interpreter and JS differ on 6 of its 7 values
- [x] the other float tests still pass on all three
- [x] `F32.show` matches C on all 74,525,948 f32 values that sit on a decimal tie, plus 2,088,960 random ones
- [x] `F32.read` matches `strtof` on 246,287 edge-case strings, on bun and node
- [x] 1M `F32.show` calls on bun: 2.62s on main, 2.00s with this change; reads unchanged

Closes #1055.
