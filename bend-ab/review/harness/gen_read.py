"""Edge-case texts for F32.read, one per line, by boundary class rather
than at random (a random tail is appended, seeded, for the fast path).

Classes:
  grammar    every spelling strtof and the JS regex might disagree on
  space      each ASCII isspace char and the Unicode spaces \\s also takes
  midpoint   for f32 pairs across every binade (both signs; the smallest
             subnormal; the normal/subnormal seam; FLT_MAX; the overflow
             threshold 2^128 - 2^103; the underflow threshold 2^-150):
             the exact midpoint, the midpoint with a 1 appended far past
             double precision (true value above), the midpoint minus one
             unit in a far digit (true value below), each in plain and in
             exponent spelling, and with more than 20 significant digits
             so an engine that truncated there would show
  extreme    huge and tiny exponents, long digit strings
  random     decimals of 1..25 significant digits, exponents in -50..45
"""
import random, struct, sys
from fractions import Fraction

random.seed(20260925)
out = []

def emit(s):
    out.append(s)

def f32_bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]

def f32_of_bits(u):
    return struct.unpack("<f", struct.pack("<I", u))[0]

def dec(fr):
    """Exact decimal text of a nonnegative dyadic fraction."""
    n, d = fr.numerator, fr.denominator
    k = d.bit_length() - 1
    assert d == 1 << k
    digits = str(n * 5 ** k)
    if k == 0:
        return digits
    digits = digits.rjust(k + 1, "0")
    return digits[:-k] + "." + digits[-k:]

def to_exp(s):
    """The same value spelled d.ddde<k>."""
    neg = s.startswith("-")
    s = s.lstrip("-")
    if "." in s:
        i, f = s.split(".")
    else:
        i, f = s, ""
    all_digits = (i + f).lstrip("0")
    lead = len((i + f)) - len(all_digits)
    if all_digits == "":
        return "0e0"
    e = len(i) - lead - 1
    m = all_digits[0] + ("." + all_digits[1:] if len(all_digits) > 1 else "")
    return ("-" if neg else "") + m + "e" + str(e)

def below(s):
    """A text just below the decimal s: its last digit decremented and 9s
    appended past double precision (if the last digit is 0, borrow)."""
    i = len(s) - 1
    while s[i] == "0" or s[i] == ".":
        i -= 1
    s2 = s[:i] + str(int(s[i]) - 1) + s[i + 1:]
    # trailing 9s: 40 of them, so the true value is within 1e-40 relative
    return s2 + ("" if "." in s2 else ".") + "9" * 40

def above(s):
    return s + ("" if "." in s else ".") + "0" * 40 + "1"

# grammar
for s in ["1", "1.", ".5", "1.5", "+1", "-1", "+.5", "-.5", "1e5", "1E5",
          "1e+05", "1E+05", "1e-5", "1.e5", ".5e1", "1e0005", "1e+0", "1e-0",
          "0", "-0", "+0", "0.0", "-0.0", "0e0", "-0e0", "00", "007", "1.50",
          "", ".", "e5", "1e", "1e+", "1e-", "+", "-", "+-1", "--1", "1..2",
          "1.2.3", "1e5.5", "1e5e5", "1_000", "1,5", "0x10", "0X1P3", "0x1p-2",
          "1x", "1p3", "x1", "inf", "Inf", "INF", "-inf", "+inf", "infinity",
          "Infinity", "INFINITY", "-infinity", "+Infinity", "infinit", "infinityy",
          "infx", "nan", "NaN", "NAN", "-nan", "+nan", "nan(1)", "nan()", "nanx",
          "nan1", "1nan", "0inf", "1 ", "1\t", " 1 ", "- 1", "+ 1", "1 e5",
          "1e 5", "1 .5", "١", "１", "1\u00001", "٫5", "1٫5", "true", "0b1",
          "0o7", "1n", "Infinityx", "-Infinity", "2.5", "0.1", "hi",
          "3.4028235e38", "3.4028236e38", "3.40282357e38", "3.4028234664e38",
          "1e38", "1e39", "1e-45", "1e-46", "1.4e-45", "7e-46", "7.1e-46",
          "1.17549435e-38", "1.1754942e-38", "1.17549421e-38"]:
    emit(s)

# space: leading, trailing, between sign and digits, and Unicode spaces
for w in [" ", "\t", "\n", "\v", "\f", "\r", " \t\n\v\f\r ", " ",
          " ", "﻿", " ", " ", "　", "\u0085",
          "​", " ", " ", " "]:
    emit(w + "1")
    emit(w + "-2.5")
    emit(w + "inf")
    emit("1" + w)
    emit("-" + w + "1")
    emit(w + w + "3e1")

# midpoints across every binade
def pairs():
    seen = set()
    def add(u):
        if u < 0x7f800000 and u not in seen:
            seen.add(u)
            yield u
    # the seams
    for u in [0, 1, 2, 3, 0x7fffff, 0x800000, 0x800001, 0x7f7ffffe,
              0x7f7fffff, 0x3f800000, 0x3f7fffff, 0x4b000000, 0x4affffff,
              0x4b7fffff, 0x4b800000, 0x5f000000, 0x1000000]:
        yield from add(u)
    # every binade: its first, its last, a few in the middle
    for e in range(0, 255):
        base = e << 23
        for m in [0, 1, 0x7fffff, 0x7ffffe, 0x400000, 0x3fffff]:
            yield from add(base + m)
        for _ in range(6):
            yield from add(base + random.randrange(0, 1 << 23))

for u in pairs():
    lo = Fraction(f32_of_bits(u))
    hi = Fraction(f32_of_bits(u + 1)) if u + 1 < 0x7f800000 else Fraction(2) ** 128
    mid = (lo + hi) / 2
    s = dec(mid)
    for sign in ["", "-"]:
        for t in [s, above(s), below(s), to_exp(s), to_exp(above(s)),
                  to_exp(below(s))]:
            emit(sign + t)
    # the value itself, exactly, and nudged
    v = dec(lo)
    emit(v)
    emit(above(v))
    if lo > 0:
        emit(below(v))

# the underflow threshold 2^-150 and the smallest subnormal, exactly
for fr in [Fraction(1, 2 ** 150), Fraction(1, 2 ** 149), Fraction(3, 2 ** 150),
           Fraction(1, 2 ** 151), Fraction(2 ** 128 - 2 ** 103),
           Fraction(2 ** 128 - 2 ** 104), Fraction(2 ** 128), Fraction(2 ** 128 - 2 ** 102)]:
    s = dec(fr)
    for t in [s, above(s), below(s), to_exp(s), to_exp(above(s)), to_exp(below(s))]:
        emit(t)
        emit("-" + t)

# extreme exponents and long strings
for s in ["1e400", "-1e400", "1e-400", "-1e-400", "1e99999999999999999999",
          "1e-99999999999999999999", "1e+0000000000000000000000001",
          "0e99999999999", "0.0e-99999999999", "1" + "0" * 400,
          "0." + "0" * 400 + "1", "1" + "0" * 50 + "e-50", "0." + "0" * 45 + "1",
          "0." + "0" * 45 + "14", "0." + "0" * 45 + "15",
          "1" * 30 + "." + "1" * 30, "9" * 39, "9" * 38 + ".9" * 5,
          "340282356779733661637539395458142568447",
          "340282356779733661637539395458142568448",
          "340282356779733661637539395458142568449",
          "340282346638528859811704183484516925440",
          "16777217.00000000001", "1.0000000596046447753906250001",
          "16777217." + "0" * 30 + "1", "16777216.99999999999999999999999999",
          "16777217", "16777219", "16777218.5", "16777217.5"]:
    emit(s)

# random fast-path decimals
for _ in range(200000):
    nd = random.randint(1, 25)
    digits = "".join(random.choice("0123456789") for _ in range(nd))
    point = random.randint(0, nd)
    mant = digits[:point] + "." + digits[point:] if random.random() < 0.8 else digits
    e = random.randint(-50, 45)
    sign = random.choice(["", "", "-", "+"])
    form = random.random()
    if form < 0.5:
        emit(sign + mant + "e" + str(e))
    elif form < 0.7:
        emit(sign + mant + "E" + ("+" if e >= 0 else "") + str(e))
    else:
        emit(sign + mant)

sys.stdout.write("\n".join(out) + "\n")
