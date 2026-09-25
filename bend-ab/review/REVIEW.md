# Adversarial review: JS F32.read / F32.show vs the C lane (patch 0001)

Reviewed against a clean checkout of HEAD 3360764 (bend 2.0.27), Linux
x86-64, clang 18.1.3 (glibc 2.39), bun 1.3.11, node 22.22.2. Everything
below was reproduced from scratch in this directory; nothing was taken from
FINDINGS.md or the earlier fuzz. Scripts are under `harness/`, the worktree
with the recommended fix is `wt/`, the patch is `fix.patch`.

## 1. Verdicts

| bug | verdict | evidence |
|---|---|---|
| #2 F32.read rounds twice on JS | CONFIRMED, deterministic, real | `repro/r1.bend`: C 1266679809 / 1065353217, bun, node and `bend file.bend` 1266679808 / 1065353216. Read differential (`harness/gen_read.py`, 246,287 texts by boundary class): 12,189 texts read to different bits on the clean JS lane, 0 on the corrected one. |
| #3 F32.read takes Unicode space on JS | CONFIRMED, deterministic, real | NBSP, EM SPACE, BOM, U+2028/9, U+3000, U+0085, U+1680, U+202F, U+205F: C None, JS Some (36 of the 246,287 texts). The six `isspace` characters read as 1 on both lanes (`" \t\u{b}\u{c}\r\n1"` gives 1065353216 on C and JS). |
| #4 F32.show rounds a tie up on JS | CONFIRMED, deterministic, real | Exhaustive over every f32 whose exact value is a decimal tie at 9 or fewer significant digits (`harness/tiecands.c`: 74,525,948 bit patterns, both signs): the clean JS lane prints 8,388,608 (= 2^23) of them differently from C's `f32_text`. Every one is an 8-significant-digit tie, between 2^-12 (`0.00024414062` vs `0.00024414063`) and 4194303.25 (`4194303.2` vs `4194303.3`); none in exponent form. |

Root causes, from the code (line numbers in `bend2/comp.ts` at 3360764):

- JS `f32_read`, lines 546-550: `Math.fround(Number(s))` after a regex anchored on `\s*`. C `f32_read`, lines 477-486: `strtof` plus a whole-length gate and a refusal of `x`, `X`, `(`.
- JS `f32_show`, lines 523-536: `x.toExponential(p - 1)` in the 1..9 digit loop; the ES spec makes `toExponential` pick the larger candidate on a tie. C `f32_text`, lines 442-470: `snprintf("%.*e")`, which glibc and macOS's gdtoa round to even on an exact tie, then the same 1..9 loop with `strtof` as the read-back.
- The checker's own `f32_show` in `bend2/bend.ts:1289-1294` uses `toExponential` too. It prints a *pure* main's F32 on the check lane and is human-only, so a pure `main() -> F32: 23577.5625` prints `23577.563` under `bend file.bend` and `23577.562` from both builds, before and after the patch (verified in `pure/`). The PR must say this is out of scope; the test therefore uses an IO main, which `bend file.bend` runs through the JS path of comp.ts (verified: the clean check lane prints the JS text for 7 of the test's 8 lines, the fixed one prints the C text).

Is any of it already decided? No. WONTFIX lists only NaN payload bits for F32 (#797, #872) and names the C lane as the reference. README's Limitations say only that F32 is axiomatic. The one prior acknowledgement is the header of `tests/run/float_text.bend`: "F32.show is printf's %g (a JS emulation past exact ties)": the author knew the emulation diverged at exact ties and let it stand; it is a caveat in a test, not a design statement. Upstream history (`git log -p -- bend2/comp.ts`, 438 commits available) touches these functions twice since the JS text was written on Sep 12 (5364281), both on the C side to match JS: e5d8733 (the NUL gate) and 38ea9f2 (hex and `nan(...)`), closing #801. That is direct precedent that read agreement between the lanes is wanted, in whichever direction is correct.

Upstream state on GitHub (read through the MCP search and WebFetch; the REST API and patch-diff host are blocked from this box):

- **Issue #1055 (open, 2026-09-25, jasisz, Darwin arm64): "JS F32.read double-rounds a finite decimal to infinity while C keeps it finite."** Its input is `340282356779733661637539395458142568447`, the largest decimal under the overflow threshold. That is bug #2 at the overflow boundary, and it is *exactly the case patch 0001 does not fix* (section 2). No PR is attached to it. Our issue would partly duplicate it; the PR should close both.
- No issue or PR mentions the show tie or the Unicode space.
- The maintainers fix lane divergence in either direction and in small PRs: #943 (JS show escapes a surrogate like C, one line), #833 (the JS decoder keeps a BOM like C), #801 (C read narrowed to JS).

## 2. Defects in patch 0001

**D1. The overflow threshold (real; the case of #1055).** In `f32_round`, when `Math.fround(v)` overflows, `g = Infinity`, so `(g + h) / 2` is `Infinity`, the midpoint test fails and the function returns `Infinity`. The double of `340282356779733661637539395458142568447` is exactly 2^128 - 2^103, the last midpoint, and strtof rounds the decimal down to FLT_MAX. Patched JS: 2139095040 (inf); C: 2139095039. The differential shows it on 10 of the 246,287 texts (the threshold in both signs, plain and exponent spelling, nudged below), 0 with the correction. The 1,000,000-string fuzz behind "0 differ" in FINDINGS.md was biased to midpoints but never produced this one. Fix: clamp the magnitude at 2^128 (`const g = Math.min(Math.abs(f), 2 ** 128)`); then `f32_bits(g)` is the inf pattern, its lower neighbour is FLT_MAX, the midpoint is the threshold, and the existing comparison picks FLT_MAX below it and keeps `f = Infinity` at or above it (strtof gives inf for the exact threshold, ties to even). Submitting the patch without this would leave #1055 open while the PR claims the read rounds once.

**D2. Duplicated helper (style).** `new Uint32Array(new Float32Array([g]).buffer)[0]` re-spells `f32_bits`, defined eight lines above. The style of the file is to reuse (`f32_from_bits` is reused two lines later).

**D3. The re-parse regex is unanchored at the start (robustness).** `/([0-9]*)\.?([0-9]*)(?:e([+-]?[0-9]+))?$/` matches the tail of `-1.5` only because the engine backtracks past the sign at index 0; it works for every string the outer regex admits, but it reads as if it could match a suffix. `^[+-]?(\d*)\.?(\d*)(?:e([+-]?\d+))?$` on the trimmed string states the intent, and with it the `|| "0"` fallback and the caller's `v !== v` special case are dead (`f32_round` already returns a non-finite `v` unchanged, NaN included).

**D4. Size (real for this repo).** `gates/repo.ts` caps comp.ts at 64,000 ttok. `ttok` cannot run here (tiktoken downloads its BPE table; egress is blocked), so from the maintainers' own count at 95317d9 (62,632 ttok for 184,320 bytes, 2.94 bytes per token): HEAD is 184,936 bytes (about 62,840 ttok); patch 0001 adds 2,580 bytes (about +880, to about 63,720); the corrected patch adds 1,853 bytes (about +630, to about 63,470). Both fit, with a few hundred tokens of headroom. The maintainers trim comments to stay under this cap (2665926 "comp.ts sheds 2.8k tokens"), so the PR must state the count, measured with `ttok < bend2/comp.ts` before posting. The corrected version shortens `f64_exact` to a `BigUint64Array` view (three lines for seven).

**Not defects (each checked against strtof or printf, not against our own fuzz).** Subnormals and 2^-149 (`7.0064923216240854e-46` reads as the smallest subnormal on both, `7.006492321624085e-46` as 0 on both); the exact midpoint 2^-150 and 3·2^-150; -0 (`-0`, `-0.0`, `-1e-9999` keep the sign); negatives (the sign is applied after the magnitude compare); leading `+`; `1.`, `.5`, `1e5`, `1E+05`, `1.e5`, `.5e1`, `1e0005`; `1e400`, `1e-400`, `1e99999999999999999999` (inf/0 with no BigInt work); 400-digit strings and `0.` followed by 400 zeros and 1; `inf`, `Inf`, `INFINITY`, `-infinity`, `nan`, `NAN`, `+nan` (NaN bits are WONTFIX and printed as `nan` by the harness); `infinit`, `infx`, `nan(1)`, `1e`, `1e+`, `.`, `e5`, `0x10`, `1p3`, trailing space, space between sign and digits, Arabic-Indic and full-width digits: None on both. `Number()` is correctly rounded past 20 significant digits on bun and node (`1 + 2^-53` with a trailing 1 rounds up), which the midpoint test relies on and the ES spec only permits. `toExponential` rounds a tie away from zero on both engines, exactly, which `f32_exp` relies on. The BigInt path runs only when the double is a midpoint (read) or the last digit is odd (show), and a finite midpoint bounds the exponent by the digit count, so cost is linear in the text. `f32_from_bits` is never handed the inf pattern plus one. `f32_exp` never needs to borrow a digit: an odd `n` that sits half a unit above the value is at least 10^q + 1, so `n - 1` keeps q + 1 digits. The 1e21 / 1e-7 switch to exponent form matches `f32_text`'s `ex >= 21 || ex <= -7`, and no printed tie is in exponent form anyway. The one harness mismatch left is `1<NUL>1`: my C oracle sees a C string `1`, while Bend's C `f32_read` checks the full length (e5d8733) and answers None, as the JS regex does.

## 3. The recommended fix (`fix.patch`)

Alternatives weighed:

- **A correctly rounded decimal-to-f32 in BigInt** (parse the digits, scale, round). It pays BigInt on every read, must clamp exponents itself (`1e999999999`), and re-implements what `Number()` already does correctly for the double. No gain in correctness once the midpoint case is handled, more code.
- **fround, then repair the exact midpoint (patch 0001's shape).** Sound because every f32 midpoint, the subnormal ones and the overflow threshold included, is a double: if `Number(s)` is not a midpoint, `s` and `Number(s)` lie on the same side of every midpoint, so `fround` is right; if it is one, an exact comparison decides. The fast path is untouched. **Chosen**, with D1-D3 corrected.
- **Show: a shared shortest-round-trip printer (Ryu-style) on both lanes.** It would change the C lane and the existing pins: `f32_text` is not "shortest, nearest" (it takes the p-digit rounding of the value, and at a binade's lower edge the round-trip interval is asymmetric), and WONTFIX names C as the reference. Changing C to round half up is backwards.
- **Show: a tie test without BigInt** (`toExponential(q + 1)` ends in 5 and `Number(t) === x`). Unsound above 2^53: a 10-digit decimal can lie within a double ulp of an f32 without equalling it. Rejected; the exact comparison through `f64_exact` and `dec_cmp` is the honest one and is shared with the read.
- **Whitespace: fix C to take Unicode spaces instead.** No: strtof is the reference and a NBSP in a number is not a feature. The regex is the right place, and `Number()` is only reached after it passes.

What `fix.patch` changes relative to patch 0001:

```
-  const g = Math.abs(f);
-  const u = new Uint32Array(new Float32Array([g]).buffer)[0];
-  const h = f32_from_bits(g < Math.abs(v) ? u + 1 : u - 1);
+  const g = Math.min(Math.abs(f), 2 ** 128);
+  const h = f32_from_bits(f32_bits(g) + (g < Math.abs(v) ? 1 : -1));
...
-  const [, int, frac, exp] = /([0-9]*)\.?([0-9]*)(?:e([+-]?[0-9]+))?$/i.exec(s);
+  const [, int, frac, exp] = /^[+-]?(\d*)\.?(\d*)(?:e([+-]?\d+))?$/i.exec(s.trim());
-  const c = dec_cmp(BigInt(int + frac || "0"), Number(exp || 0) - frac.length, m, p);
+  const c = dec_cmp(BigInt(int + frac), Number(exp ?? 0) - frac.length, m, p);
...
-  const v = Number(s.replace(/inf\w*/i, "Infinity"));
-  return {$: "Some", value: v !== v ? v : f32_round(s.trim(), v)};
+  return {$: "Some", value: f32_round(s, Number(s.replace(/inf\w*/i, "Infinity")))};
```

plus `f64_exact` as a `BigUint64Array` view, and the comment on `f32_round` naming the threshold. `f32_exp`, `dec_cmp` and the show loop are patch 0001's. It also edits one line of `tests/run/float_text.bend`'s header, which would otherwise say the emulation holds only "past exact ties" (optional; drop the hunk if a smaller diff is preferred). `git apply --check fix.patch` passes on a fresh worktree of 3360764.

Verification of the corrected text (extracted verbatim from `wt/bend2/comp.ts` by `harness/extract_js.sh`, so what ran is what the compiler emits):

- read, 246,287 texts: clean JS 12,226 mismatches vs C (12,189 bits differ, 36 C-None/JS-Some, 1 harness NUL artifact); patch 0001: 10 (all the overflow threshold); corrected: 1 (the NUL artifact), on bun and on node.
- show, all 74,525,948 tie patterns against C's `f32_text`: clean JS 8,388,608 mismatches; patch 0001 on bun: 0; corrected on bun: 0; corrected on node: 0. Stratified sample of 4096 random mantissas per exponent and sign (2,088,960 patterns, the non-tie path): clean JS 4,011, patch 0001 on bun 0, corrected on bun 0 and on node 0 (`harness/sweep2.sh`, its logs `harness/sweep2_*.log`).
- `tests/run/float_text_lanes.bend` (section 5) on C, bun, node and `bend file.bend`: the pinned text.
- Local replica of `gates/test.ts` over the worktree (`run_suite.py`, then `suite_report.py`'s two gate exclusions): the 122 `tests/io` tests (check 122, c 82, js 101 lanes run): 4 fail, the three `audio_only` C builds (no libasound here) and `io_spawn_sleep` [js, check] (section 6), the set FINDINGS reports on main and, for `spawn_sleep`, verified on main by hand; the 100 tests that use F32 or a float literal (check 100, c 76, js 76): 0 fail, the new test included, on check, js and c.

## 4. What they accept (precedent)

From the 438 commits available and the closed PR list (59 closed unmerged):

- **Title**: one sentence stating the new truth in the present tense, often ending "as the C lane does" / "like C": "The JS show escapes a surrogate like C (#943)", "The JS event loop waits again when a signal interrupts select, as the C loop does (#1036)", "IO.thread_count answers the worker pool's size, and 1 on the JS lanes (#1048)", "F32.fma rounds a * b + c once, in C and JS (#1046, open)". Never "fix:", "feat:" (the ones so titled, #929, #940, #982, #985, were closed).
- **Body**: the wrong behaviour and its cause in one paragraph, with the function names; the change in one; the test in one, naming what each lane printed before ("on main its check fails with ..."); verification counts per lane (aldeni's #1048/#1049 list them, plus the ttok delta); "Closes #N." Co-Authored-By lines for Claude are common in merged PRs.
- **Tests**: `tests/<ns>/<name>.bend` (`[a-z0-9_]+`, 16,000-ttok cap), a header comment that says what is pinned and why, usually with the old wrong output and the issue number, `import Base`, an IO main with `Unit <- IO.print(...)` lines, `#|` lines at the end. The gate runs check (no `Error:`), interp (exact text, through `--checkup`), js and c. A `{... == ...}` law pin is used where the checker can compute it (#1009's `Nat.read`); it cannot for `F32.read` or `F32.show` (opaque laws), so an IO main is the only way to pin them.
- **Size**: small. #943 is one line and no test; #1009 a dozen lines and a test; #1036 a loop change and a test with a C/JS twin. Rejected or reverted: #929 (an F32 math library: +7,849 ttok, GPU breakage, a wrong `round`), #940 (a token-saving refactor "harder to follow"), #844 (reverted: three C-lane tests printed wrong answers). Every comp.ts change is weighed against its 64,000-ttok cap.
- **Lane agreement is a recognised bug class** and the maintainers fix it in either direction (#801 narrowed C to JS; #833 and #943 moved JS to C). The C lane is the reference for F32 (WONTFIX).

## 5. The test: `tests/run/float_text_lanes.bend`

Eight lines, each pinning one boundary, no filler:

| print | why | clean C | clean JS and `bend file.bend` | fixed, all lanes |
|---|---|---|---|---|
| `F32.read("16777217.00000000001")` | a decimal just above an f32 midpoint whose double is the midpoint | 1266679809 | 1266679808 | 1266679809 |
| `F32.read("-1.0000000596046447753906250001")` | the same at 1 ulp and negative (sign applied after the compare) | 3212836865 | 3212836864 | 3212836865 |
| `F32.read("340282356779733661637539395458142568447")` | the overflow threshold, #1055; fails on patch 0001 too | 2139095039 | 2139095040 | 2139095039 |
| `F32.read("7.0064923216240854e-46")` | the underflow midpoint 2^-150 (fround takes it to 0) | 1 | 0 | 1 |
| `F32.read("\u{a0}1")` | NBSP, which strtof refuses | None | 1065353216 | None |
| `F32.read(" \t\u{b}\u{c}\r\n1")` | the six isspace characters, a control that the regex still takes them | 1065353216 | 1065353216 | 1065353216 |
| `F32.show(23577.5625)` | an 8-digit tie, positive | 23577.562 | 23577.563 | 23577.562 |
| `F32.show(F32.neg(2983783.25))` | an 8-digit tie, negative (the sign is re-attached by `f32_exp`) | -2983783.2 | -2983783.3 | -2983783.2 |

Before the fix the check lane's interp probe and the JS lane fail (7 of 8 lines), the C lane passes; after it all three print the pinned text (run directly in `tout/` and through `run_suite.py`). The only line that passes everywhere before the fix is the ASCII-space control, kept on purpose so the whitespace hunk cannot over-narrow the regex.

## 6. Patch 0002 (the deadline-order wake), briefly

The observation is real on this box: `tests/io/spawn_sleep.bend` on the clean JS build printed `main a b done` or `main done a b` in 5 of 5 runs while the C binary printed `main b a done` every time. But the maintainers have already been here: **PR #1052 by nicolas-abril (a maintainer), "Due timers wake by deadline, so spawn_sleep and fork_join sleep 1-3 ms", closed by its author on 2026-09-25** with "Dropped: it fixes the ordering, but it is not a performance improvement (C neutral, JS slightly slower)." It collected the due parks, sorted the pure timers by deadline (C `qsort` on (deadline, slot), JS a stable sort) and kept fd waits in their slots, and shrank the two tests' sleeps. Patch 0002 is the same idea with a slightly different order rule (every due park sorted by deadline, readiness-only first). A PR of this shape has just been declined from inside the team; sending ours would look like not having read the tracker. What is worth filing is the fact they cannot see on an M4 gate: the JS lane fails their own `spawn_sleep` pin on Linux x86-64 every run, with the `io_sys()` first-call cost as the suspected reason (FINDINGS says ~14 ms of `bun:ffi` dlopen after the timeout was computed; I did not verify that number). Let them choose between the reorder they already wrote and moving `io_sys()` ahead of the deadline arithmetic.

## 7. FINDINGS.md: what overclaims or needs correcting

- Patches table, "F32.read ... 0 differ" after patch 0001: true of that corpus, false in general; the overflow threshold (#1055) still differs. Say "0 differ on 1,000,000 midpoint-biased strings; the overflow threshold needed a further clamp".
- Entry 2 does not mention the overflow threshold or #1055, which is the case a maintainer will test first.
- Entry 4, "104 of 200,000 fuzzed floats": fine as a sample, but the true statement is sharper and worth making: exactly 2^23 f32 values print differently, all 8-significant-digit ties in [2^-12, 4194303.25], none in exponent form.
- Entry 4 should note that the *checker's* printer (bend.ts) keeps the JS digits for a pure F32 main and is out of scope.
- "With both patches applied, all 198 IO and F32 tests pass except the three ALSA builds": with the corrected 0001 alone, the 100 F32 tests pass on every lane and the 122 io tests fail only on the three ALSA builds and `io_spawn_sleep` [js, check], as on main (expected without 0002). Consistent.
- Entry 8's "fails 20/20": 5/5 here, consistent. The dlopen cause is asserted, not shown.
- Entry 1 (literal double rounding in bend.ts:2321) is consistent with what I read; note the consequence of this PR: on every lane `F32.read("16777217.00000000001")` and the literal `16777217.00000000001` will now denote different floats until bend.ts is fixed by its owner. Mention it in the PR as the known, separate defect.
- The "not bugs" table's Parallel speedup and C build time rows are unverified here and irrelevant to this submission.

## 8. Files in this directory

- `fix.patch`: the recommended patch (comp.ts, the new test, the one-line header edit), applies on 3360764.
- `issue.md`, `pr.md`: ready to paste; two bracketed fields to fill (the ttok count, the issue number).
- `wt/`: the worktree carrying the fix (remove with `git -C ../Bend worktree remove --force wt`).
- `harness/extract_js.sh`: pulls the JS runtime helpers out of a comp.ts verbatim.
- `harness/oracle.c`: the C lane's `f32_text` (verbatim) and `f32_read` (its gate reproduced), line-driven. `harness/drive.js`: the JS side, same modes, streaming.
- `harness/gen_read.py`: the 246,287 read texts by boundary class. `harness/tiecands.c`: every decimal-tie f32 (and a stratified sampler).
- `harness/show_sweep.sh`, `harness/sweep2.sh`: the show sweeps; `*_c.txt`, `*_clean_bun.txt`, `*_ours_bun.txt`, `*_fix_bun.txt`, `*_fix_node.txt` are their outputs (large).
- `repro/r1.bend`: the from-scratch reproduction on four lanes. `pure/pm.bend`: the pure-main check-lane caveat. `tout/`: the new test on every lane, fixed and clean. `ss/`: spawn_sleep on JS and C.
- `suite_fix/`, `suite_fix_f32/`: `run_suite.py` results on the worktree (`suite_report.py` applies the gate's exclusions).
