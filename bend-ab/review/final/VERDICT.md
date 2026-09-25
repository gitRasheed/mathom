# Final review: JS F32.read / F32.show vs the C lane

Reviewed against a fresh worktree of 3360764 with `review/fix.patch` (v2)
applied, then with the shorter version in this directory (v3, `fix.patch`).
Nothing long was re-run; the 74.5M tie sweep and the 2M sample are taken as
given for v2. Every number below for v3 was measured here.

## Verdict: READY WITH NITS

The v2 code is correct. I found no input where it disagrees with strtof or
with the C `f32_text`, by reading or by spot checks (116 hand-picked read
edges, 3,129 hand-picked show values, all matching C on bun and node). The
design is sound (argument below). What stands between the draft and a post
is text, not code, plus one size measurement. The shorter v3 in this
directory is recommended but optional; if adopted it needs the listed
re-verification.

### Blocking before posting (both versions)

1. **comp.ts ttok.** `gates/repo.ts` caps comp.ts at 64,000 ttok and #1046
   reported its count. ttok cannot fetch its tokenizer here (proxy 403).
   Estimate from #1046's own numbers (95317d9: 184,320 bytes = 62,602 ttok;
   its +617 bytes of code cost +265): main 3360764 is 184,936 bytes, about
   62,870; v2 adds 1,989 bytes, about +680 to +850, landing near 63,550 to
   63,720; v3 adds 1,659 bytes, about +560 to +710, near 63,430 to 63,580.
   Both fit, with a few hundred tokens of headroom. Measure with
   `ttok < bend2/comp.ts` on main and on the branch before posting and put
   the two numbers in the PR body. If it cannot be measured, say "about" and
   the byte delta; do not post a number that looks measured.
2. **Drop the separate issue.** #1055 already reports cause 1 and is the
   case a maintainer will test first. An issue opened and closed by the same
   author within the hour is process noise for a three-function fix; the
   JS-vs-C table belongs in the PR body (it is there in `pr.md`). File the PR
   with "Closes #1055." only. If an issue is wanted anyway, `issue.md` here
   is the short form; then the PR's closes line names it.
3. **PR text**: use `pr.md`. The draft's "closes #[issue]" placeholder, the
   missing ttok line and the missing GPU note are the concrete gaps (see
   section 5).

### Optional (recommended)

4. **Take v3** (`fix.patch` here): one helper instead of two, 330 fewer
   bytes, same speed, arguably what nicolas-abril would ask for ("any reason
   not to inline this function?" on #1062): `f64_exact` was only ever
   consumed by `dec_cmp`, so it is now its first four lines. Re-verification
   needed before posting v3, with what I already ran:
   - `harness/v2_sweep.sh` with `funcs_v3.js` (about 15 min): NOT run. Run
     here: every 64th tie (1,164,468 values) and a stratified sample of 512
     per exponent and sign plus the edges (264,249 values): 0 differences
     vs C on bun, and 0 on node for the sample.
   - `read_cases.txt` (246,287): run on bun, 1 hit, the NUL artifact. Run it
     on node too.
   - the 100 float tests on check, JS and C (`run_suite.py`): NOT run. The
     new test and `float_text.bend` pass on check, bun, node and C here.
   - bench (`bench3.js`, 1M calls, bun, median of 5): main show 2,355 ms,
     v2 2,025, v3 2,068; read 110 / 104 / 116 ms. Same within noise.
5. **Test** (in `fix.patch`): header rewritten in the repo's past-tense
   style, and two cases added (7 lines of pin instead of 5): a negative
   subnormal midpoint read, which is the only case that exercises the bottom
   edge of the neighbour computation (`f32_bits(0) + 1`) and the sign
   re-attachment (`Math.sign(v) * h`), and a negative show tie, the only case
   that exercises the sign path of the tie rewrite. On main JS and the check
   lane print 6 of the 7 values differently from C (measured); the ASCII
   space control is the one that passes everywhere, kept so the regex hunk
   cannot over-narrow.

## 1. Design

**Read: fround, then repair an exact midpoint.** Sound. Every f32 midpoint
is a double (25 significant bits; the subnormal ones and the overflow
threshold 2^128 - 2^103 included). Double rounding is monotonic, so if
`Number(s)` is not a midpoint, `s` and `Number(s)` lie on the same side of
every midpoint and `fround` is the correctly rounded f32. If it is one, the
exact BigInt comparison decides, and an exact tie (`c === 0`) falls to
`fround`, which is ties-to-even like strtof. The clamp `Math.min(|f|, 2^128)`
makes the inf pattern's lower neighbour FLT_MAX and the midpoint the
threshold; above 2^128 `f32_from_bits(inf + 1)` is a NaN, the midpoint test
fails, and `f` (inf) is returned, which is right. The fast path is the old
code plus one compare. It relies on `Number()` being correctly rounded past
20 digits, which the ES spec only permits; V8 and JSC both are (checked with
`1.00000005960464477539062500` plus a trailing 1 / 0 / 9999).

**Show: tie check at the final precision only, behind a cheap pre-check.**
Sound. The claim is that the JS loop (round-half-up text, `fround(Number)`
read-back) and the C loop (round-half-even text, `strtof` read-back) stop at
the same q, so only the text at that q can differ. At any q where x is a
decimal tie, the two candidate texts are at the same distance from x, and
the set of decimals that read back to x is symmetric around x except (a) at
a binade boundary x = 2^k, where the interval below is half the one above,
and (b) when a candidate sits exactly on an f32 midpoint, where the parity of
the neighbour decides. For (b) both neighbours x - ulp and x + ulp have the
same parity, so both candidates read back or neither. For (a) a power of two
is a decimal tie at q digits only when 2^(k+1) = 5^m (odd) times a power of
ten, so 2n + 1 = 5^m; within q <= 8 that is m = 11 (x = 2^-12, tie at q =
7, candidates 5e-12 from x, below-interval 7.3e-12: both read back) and m =
12 (x = 2^-13, q = 8, both read back); m <= 10 candidates are too far (2^-11
at q = 6: 5e-11 against 1.5e-11) and m = 13 is q = 9. So the stopping q is
the same, and a tie at the final q is exactly the case the code repairs:
odd last digit (an even one already matches printf) and the value exactly
half a unit below the printed candidate. The pre-check
`/5e/.test(toExponential(q + 1)) && Number(toExponential(q + 1)) === x` has
no false negatives (a tie at q has q + 2 digits ending in 5, exact at q + 1)
and its false positives (a 10-digit decimal within a double ulp of x above
2^53) go to the exact compare, which is why the BigInt step stays.

**What the sweep does not cover.** The tie sweep is exhaustive for the tie
path. The non-tie path relies on `fround(Number(t)) === x` agreeing with
`strtof(t) == v` for every <= 9-digit text t the loop tries, which is a
double rounding: it can differ only if `Number(t)` lands exactly on an f32
midpoint M with t != M. That needs |t - M| < 2^-53 M with t = D * 10^j, D <
10^9, which forces M >= 2^63 (for smaller M the difference is a multiple of
2^j larger than the tolerance). The 2M stratified sample covers 1/2048 of
each binade there. I could not exclude it by argument (heuristically the
expected count of such M over all 2^30 candidates is far below 1), and it is
not introduced by this patch: main's loop has the same read-back. Not
blocking; worth knowing if a report ever comes in for a value above 9e18.

## 2. Correctness, line by line

Read (v2 and v3 identical except the `dec_cmp` call):
- Regex: `[ \t\n\v\f\r]` is C-locale `isspace` exactly; no trailing space,
  no `x`/`X`/`(`, matching the C gate. `inf`, `infinity`, `nan` with any
  case; `infinit`, `nan(1)`, `1e`, `.`, `0x10`, `e5`: None on both.
- `Number(s.replace(/inf\w*/i, "Infinity"))`: `+inf`, `-INFINITY`, `nan`
  (NaN, returned through `!isFinite`), `1e400` (inf), `1e-400` (0),
  `-1e-400` (-0), `1e99999999999999999999` (inf, no BigInt work): all match
  C.
- `f === v` catches every exact double including 0 and -0.
- Neighbour: `g` and `h` adjacent f32s (or 2^128 and FLT_MAX), so
  `(g + h) / 2` is exact; `g = 0` gives `h = 2^-149` and midpoint 2^-150,
  the subnormal case (`7.0064923216240854e-46` reads as bits 1 on both,
  `7.006492321624085e-46` as 0 on both, negative keeps the sign).
- Re-parse regex on `s.trim()`: `1.`, `.5`, `+.5`, `1.e5`, `.5e1`,
  `1e0005`, `001e0002`, `00001.50000` all give the right (digits, k);
  `Number(exp ?? 0)` handles the missing group; `BigInt` takes leading
  zeros.
- Return: `c === 0` (exact midpoint) keeps `fround`'s ties-to-even answer;
  the sign is applied after the magnitude compare, and `Math.sign(v) * 0`
  gives -0 for a negative underflow, as strtof does.
- Cost: BigInt runs only when `v` is a finite midpoint, which bounds the
  effective exponent by the digit count, so the work is linear in the text
  (a 400-digit midpoint spelling takes microseconds; a 10^7-digit one would
  take seconds, as `Number()` itself would).

Show (v3; v2 differs only in the rebuild line and the helper):
- `q` ends in 0..8; 9 digits always round-trip an f32.
- The tie rewrite `(n - 1n)` never loses a digit (an odd n is not a power of
  ten), and `String(Number("23577562e-3"))` is the same double and the same
  text as `String(Number("2.3577562e+4"))`, so the decimal point need not be
  placed. Negative x, x = 2^-12, 2^-13, 2^-149, FLT_MAX, 2^24 + 2, 1e21,
  1e-7, 1.5, 2.5, 0.25, 15, 25, 4194303.25: all match C on bun and node.
- Comment lines are within 80 columns; `b & mask | 1n << 52n` relies on
  `<<` > `&` > `|`, which is what the original also relied on.

Nothing in the C, Metal or CUDA text changes. `F32.read` and `F32.show` are
`#define`d to `err_post(e.mem, ERR_FIDS)` under `#if DEVICE` (comp.ts lines
429-432), so there is no GPU case to cover; the PR should say so, since
pjcavalcanti asked for GPU coverage on #1046.

## 3. Conciseness

v2 has two helpers, `f64_exact` (used twice, both times immediately feeding
`dec_cmp`) and `dec_cmp` (four lets and two conditionals). v3 folds the
first into the second, writes the scaling as two `Math.max` lines, takes
the sign from `x < 0` instead of `man[0].replace(/[^-]/, "")`, rebuilds the
tie text as `digits e exponent` instead of re-placing the decimal point, and
cuts each comment to two lines. 186,595 bytes against v2's 186,925 (main
184,936). Diff: 47 insertions, 8 deletions in comp.ts. Equivalence: pure
refactor of the same arithmetic (`2 * x` in place of `p + 1`; the scaling
by `max(k, 0)` / `max(-k, 0)` is the old conditional written out), confirmed
by the differentials in the header. I do not see a further cut that keeps
the file's style: the regex re-parse, the clamp and the neighbour line each
earn their place, and the pre-check is what keeps show faster than main.

## 4. The test

Shape matches #1062's (`bits` helper, `main.out` with `++` lines, one pin)
and the existing `float_text.bend`; 27 lines, 998 bytes, far under the
16,000-ttok cap. `tests/run` is right: all six float tests live there and
merged fixes use `tests/run` and `tests/io` as often as `tests/reg`. Each
of the seven values guards one distinct path (midpoint above 2^24; overflow
clamp, #1055; bottom edge plus sign; Unicode space refused; ASCII space
kept; positive tie; negative tie). Header now says what is pinned and what
JS once printed, with the issue number, like #1062's. The one-line edit of
`float_text.bend`'s header is right to keep: the old text documents the
divergence this PR removes.

## 5. Issue and PR text

- Title of the PR draft fits the log style (present-tense truth); `pr.md`
  keeps it with "answer as the C lane does on JS", the form of #1036 and
  #943.
- The draft's claims are all backed except the ttok line, which is absent
  and required by precedent (#1046), and "closes #[issue]".
- Missing and added in `pr.md`: the device note (ERR_FIDS), the bend.ts
  caveat (a pure F32 main under `bend file.bend` still prints a tie up; the
  file is human-written, so it is left alone; a maintainer will notice this
  within a minute of testing), the `float_text.bend` header edit, the test's
  "6 of 7" line, and the byte/ttok delta.
- Issue draft: accurate, but redundant with #1055 plus the PR body (see
  blocking item 2). `issue.md` is the fallback, trimmed.
- Bench numbers: state them as measured here (main 2.4 s, patch 2.1 s per
  1M shows on bun) or as "about 15% faster"; the draft's 2.56/2.06 are also
  real runs, either is fine.

## 6. Files

- `fix.patch`: v3, applies clean on 3360764 (checked on a second fresh
  worktree), comp.ts + the header line + the 7-value test.
- `funcs_v3.js`: the JS text extracted verbatim from the patched comp.ts.
- `pr.md`, `issue.md`: the texts.
- `bench3.js`, `read_edge*.txt`, `show_edge*.txt`, `samp*.txt`,
  `ties64*.txt`: the checks above.
