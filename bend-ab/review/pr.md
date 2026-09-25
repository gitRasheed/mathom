# PR

**Title:** The JS F32.read rounds once and skips ASCII space only, and the JS F32.show rounds a tie to even, as the C lane does

**Body:**

The JS runtime's F32.read computed `Math.fround(Number(s))`: the decimal
rounded to a double, then the double to an f32, and when the double was
exactly an f32 midpoint the second rounding went to even and lost the side
the decimal was on. The C lane's strtof rounds once. So "16777217.00000000001"
read as 16777216 on JS and 16777218 on C, the largest decimal under the
overflow threshold 2^128 - 2^103 read as inf on JS and as 3.4028235e38 on C
(#1055), and the first decimal past 2^-150 read as 0 instead of the smallest
subnormal. Its regex also anchored on `\s*`, which takes NBSP, EM SPACE and
the BOM, where strtof skips isspace only, so "\u{a0}1" read as 1 on JS and
None on C. And F32.show picked its digits with toExponential, which rounds a
decimal tie away from zero, where the C f32_text's `%.*e` rounds it to even:
23577.5625 printed as 23577.563 on JS and 23577.562 on C. `bend file.bend`
runs an IO main through the JS path, so a program printed one text there and
another from its own binary.

f32_read keeps Number then fround, which is right whenever the double is not
an f32 midpoint (every midpoint is a double, so a decimal and its double are
on the same side of each one); when it is, f32_round compares the decimal
with the midpoint exactly (BigInt, through f64_exact and dec_cmp) and takes
the neighbour the decimal is nearer to, an exact tie staying with fround's
even choice, as strtof's. The magnitude is clamped at 2^128 so the overflow
threshold has FLT_MAX and inf as its neighbours. The regex takes the six
isspace characters. f32_show rounds through f32_exp, which is toExponential
except that an odd last digit that sits exactly half a unit above the value
(checked exactly with the same two helpers) steps down to the even one.
The BigInt work runs only on a midpoint read and on an odd-digit show.

tests/run/float_text_lanes.bend pins the three: the four reads above and
their bits, the NBSP read as None beside the six ASCII spaces read as 1,
and the two shown ties. On main its check (the interpreted run) and its JS
lane print the double-rounded bits, 1065353216 for the NBSP and the rounded-
up digits, and its C lane prints the pinned text; on this branch all three
print it. tests/run/float_text.bend's header no longer says the emulation
holds only past exact ties.

Verification, Linux x86-64, bun 1.3.11 and node 22.22.2 against glibc 2.39:

- F32.read: 246,287 texts by boundary class (every strtof grammar and
  isspace spelling, the Unicode spaces, the exact midpoint of f32 pairs
  across every binade, subnormals, FLT_MAX and the overflow and underflow
  thresholds, each nudged above and below past double precision and past
  20 significant digits, huge and tiny exponents, 400-digit strings, and
  200,000 random decimals) through the C lane's f32_read and the JS text
  extracted from comp.ts: 12,226 differ on main, 0 on this branch, bun and
  node alike.
- F32.show: every f32 whose exact value is a decimal tie at nine or fewer
  significant digits (74,525,948 bit patterns, both signs, the only values
  on which half-up and half-even can print differently) plus 2,088,960
  random mantissas stratified over every exponent and sign, through the C
  f32_text and the JS f32_show: 8,388,608 (2^23) differ on main, every one
  an 8-digit tie between 2^-12 and 4194303.25, 0 on this branch, bun and
  node alike.
- A local replica of gates/test.ts (check, interpreted, JS and C) over the
  100 tests that use F32 or a float literal and the 122 tests/io tests: the
  100 pass, the new test included; the io set fails as on main here (the
  three audio_only C builds want libasound, and spawn_sleep's JS wake is late
  on this Linux box), nothing else.
- comp.ts [ttok: fill in from `ttok < bend2/comp.ts` on the branch and on main].

Closes #1055, closes #[the issue].
