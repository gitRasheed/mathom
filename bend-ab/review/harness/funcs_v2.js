function word_to_u32(w) {
  let x = 0;
  for (let i = 0; w.$ === "WCon"; i++) {
    x |= Number(w.head) << i;
    w = w.tail;
  }
  return x >>> 0;
}

function u32_to_word(x) {
  let w = {$: "WNil"};
  for (let i = 31; i >= 0; i--) {
    w = {$: "WCon", head: ((x >>> i) & 1) === 1, tail: w};
  }
  return w;
}

function cmp_new(a, b) {
  return {$: a < b ? "LT"
    : a === b ? "EQ" : "GT"};
}

function nat_divmod(a, b) {
  return b === 0n ? {$: "Tuple", fst: 0n, snd: a}
    : {$: "Tuple", fst: a / b, snd: a % b};
}

function nat_chk(n) {
  if (n > 281474976710655n) {
    throw "bend: ${ERRS[5]}";
  }
  return n;
}

// A finite double exactly, as [m, p] with |x| = m * 2^p.
function f64_exact(x) {
  const b = new BigUint64Array(new Float64Array([Math.abs(x)]).buffer)[0];
  const e = Number(b >> 52n);
  return [e === 0 ? b : b & 0xfffffffffffffn | 1n << 52n, (e === 0 ? 1 : e) - 1075];
}

// The sign of d * 10^k - m * 2^p, exactly.
function dec_cmp(d, k, m, p) {
  let a = k >= 0 ? d * 10n ** BigInt(k) : d;
  let b = k >= 0 ? m : m * 10n ** BigInt(-k);
  a = p >= 0 ? a : a << BigInt(-p);
  b = p >= 0 ? b << BigInt(p) : b;
  return a > b ? 1 : a < b ? -1 : 0;
}

function f32_show(x) {
  if (x !== x) {
    return "nan";
  }
  if (!Number.isFinite(x) || Object.is(x, -0)) {
    return x < 0 ? "-inf"
      : x === 0 ? "-0" : "inf";
  }
  let q = -1;
  let t = "x";
  while (q < 8 && Math.fround(Number(t)) !== x) {
    q += 1;
    t = x.toExponential(q);
  }
  // toExponential rounds a decimal tie up, the C lane's printf to even: a
  // tie at q digits is exact at q + 1, ending in 5, so an odd last digit
  // steps down when the value is exactly half a unit below it
  const [man, ex] = t.split("e");
  if (/[13579]$/.test(man) && /5e/.test(x.toExponential(q + 1))
    && Number(x.toExponential(q + 1)) === x) {
    const n = BigInt(man.replace(/[-.]/g, ""));
    const [m, p] = f64_exact(x);
    if (dec_cmp(2n * n - 1n, Number(ex) - q, m, p + 1) === 0) {
      const d = String(n - 1n);
      t = man[0].replace(/[^-]/, "") + d[0] + (q > 0 ? "." + d.slice(1) : "") + "e" + ex;
    }
  }
  return String(Number(t));
}

function f32_bits(x) {
  return new Uint32Array(new Float32Array([x]).buffer)[0];
}

function f32_from_bits(u) {
  return new Float32Array(new Uint32Array([u]).buffer)[0];
}

function f32_read(s) {
  const re = /^[ \t\n\v\f\r]*[+-]?((\d+\.?\d*|\.\d+)(e[+-]?\d+)?|inf(inity)?|nan)$/i;
  if (!re.test(s)) {
    return {$: "None"};
  }
  // Number then fround rounds twice, and misses when the double is exactly an
  // f32 midpoint (the overflow threshold 2^128 - 2^103 included): the decimal
  // then decides the side, compared with the midpoint exactly, as strtof does
  const v = Number(s.replace(/inf\w*/i, "Infinity"));
  const f = Math.fround(v);
  if (f === v || !Number.isFinite(v)) {
    return {$: "Some", value: f};
  }
  const g = Math.min(Math.abs(f), 2 ** 128);
  const h = f32_from_bits(f32_bits(g) + (g < Math.abs(v) ? 1 : -1));
  if (Math.abs(v) !== (g + h) / 2) {
    return {$: "Some", value: f};
  }
  const [, int, frac, exp] = /^[+-]?(\d*)\.?(\d*)(?:e([+-]?\d+))?$/i.exec(s.trim());
  const [m, p] = f64_exact(v);
  const c = dec_cmp(BigInt(int + frac), Number(exp ?? 0) - frac.length, m, p);
  return {$: "Some", value: c === 0 || (c > 0) === (g > h) ? f : Math.sign(v) * h};
}

function char_new(code) {
  if (code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
    throw "bend: " + code + " is not a Unicode scalar value";
  }
  return String.fromCodePoint(code);
}
