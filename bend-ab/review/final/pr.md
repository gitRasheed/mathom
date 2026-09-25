**Title:** F32.read and F32.show answer as the C lane does on JS

JS read a float with `Math.fround(Number(s))`, two roundings: a decimal just past an f32 midpoint whose double is that midpoint landed on the wrong side (`16777217.00000000001` read as 16777216, the largest decimal under the overflow threshold as inf, #1055, the midpoint 2^-150 as 0). The regex's `\s` also took Unicode spaces that `strtof` refuses (NBSP then 1 read as 1). `F32.show` used `toExponential`, which rounds a decimal tie up where C's `printf` rounds to even: 23577.5625 printed as 23577.563, and 2^23 f32 values print differently, all 8-digit ties.

`f32_read` keeps `Number` and `fround` on the fast path; only when the double is exactly an f32 midpoint (the overflow threshold included) does it compare the decimal with the midpoint exactly and pick the side, as `strtof` does, and its regex skips ASCII space only. `f32_show` keeps the `toExponential` loop and, at the precision it stops at, steps an odd last digit down when the value is exactly a tie. One helper, `dec_cmp`, does the exact comparison for both. Only the JS runtime text changes; C, Metal and CUDA are untouched, and `F32.read` and `F32.show` are not available on the device (ERR_FIDS), so there is no GPU case. `bend file.bend` prints a pure F32 main through bend.ts's own printer, which still rounds a tie up; that file is human-written, so this leaves it. The header of `tests/run/float_text.bend`, which excepted exact ties, is updated.

Test plan
- [x] `tests/run/float_text_lanes.bend` prints the same line on the interpreter, JS and C; on main, JS and the interpreter differ from C on 6 of its 7 values
- [x] the other 99 float tests pass on check, JS and C
- [x] `F32.show` matches C's `f32_text` on all 74,525,948 f32 values that are decimal ties at 9 digits or fewer, and on 2,088,960 stratified random floats, on bun and node
- [x] `F32.read` matches `strtof` on 246,287 boundary-class strings, on bun and node
- [x] 1M shows on bun: 2.4 s on main, 2.1 s here; reads unchanged
- [x] comp.ts: [MAIN_TTOK] ttok on main, [BRANCH_TTOK] with this change (+1,659 bytes)

Closes #1055.
