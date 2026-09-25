**Title:** F32.read and F32.show give the C lane's answer on JS

JS read a float by rounding the decimal to a double and then to an f32, so a decimal just past an f32 midpoint could land on the wrong side (16777217.00000000001 read as 16777216; the case in #1055 read as inf). It also accepted Unicode spaces that `strtof` rejects, and `F32.show` rounded decimal ties up where C's `printf` rounds to even (23577.5625 printed as 23577.563).

`F32.read` now compares the decimal with the midpoint exactly when the double lands on one, and only skips ASCII whitespace. `F32.show` checks for a tie once, at the precision it stops at. Only the JS runtime changes; C, Metal and CUDA are untouched.

Test plan
- [x] `tests/run/float_text_lanes.bend` prints the same line on the interpreter, JS and C (on main, JS and the interpreter differ on 4 of 5 values)
- [x] the other 99 float tests still pass on all three
- [x] `F32.show` matches C on all 74,525,948 f32 decimal ties and 2,088,960 random floats
- [x] `F32.read` matches `strtof` on 246,287 edge-case strings
- [x] 1M shows on bun: 2.56s on main, 2.06s with this change; reads unchanged

Closes #1055, closes #[issue].
