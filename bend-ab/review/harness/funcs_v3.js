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

// The sign of d * 10^k - |x| for a finite double x, exactly: |x| is m * 2^p
function dec_cmp(d, k, x) {
  const b = new BigUint64Array(new Float64Array([Math.abs(x)]).buffer)[0];
  const e = Number(b >> 52n);
  const m = e === 0 ? b : b & 0xfffffffffffffn | 1n << 52n;
  const p = (e === 0 ? 1 : e) - 1075;
  const a = d * 10n ** BigInt(Math.max(k, 0)) << BigInt(Math.max(-p, 0));
  const c = m * 10n ** BigInt(Math.max(-k, 0)) << BigInt(Math.max(p, 0));
  return a > c ? 1 : a < c ? -1 : 0;
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
  // toExponential rounds a decimal tie up, the C lane's printf to even: an odd
  // last digit steps down when the value is exactly half a unit below it
  const [man, ex] = t.split("e");
  if (/[13579]$/.test(man) && /5e/.test(x.toExponential(q + 1))
    && Number(x.toExponential(q + 1)) === x) {
    const n = BigInt(man.replace(/[-.]/g, ""));
    if (dec_cmp(2n * n - 1n, Number(ex) - q, 2 * x) === 0) {
      t = (x < 0 ? "-" : "") + (n - 1n) + "e" + (Number(ex) - q);
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
  // fround after Number rounds twice: on an f32 midpoint (the overflow
  // threshold 2^128 - 2^103 included) the decimal decides, as strtof does
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
  const c = dec_cmp(BigInt(int + frac), Number(exp ?? 0) - frac.length, v);
  return {$: "Some", value: c === 0 || (c > 0) === (g > h) ? f : Math.sign(v) * h};
}

function char_new(code) {
  if (code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
    throw "bend: " + code + " is not a Unicode scalar value";
  }
  return String.fromCodePoint(code);
}
