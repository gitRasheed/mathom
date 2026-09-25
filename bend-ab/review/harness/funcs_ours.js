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
  const v = new DataView(new ArrayBuffer(8));
  v.setFloat64(0, Math.abs(x));
  const hi = v.getUint32(0);
  const e = (hi >>> 20) & 0x7ff;
  const m = (BigInt(hi & 0xfffff) << 32n) | BigInt(v.getUint32(4));
  return [e === 0 ? m : m | (1n << 52n), (e === 0 ? 1 : e) - 1075];
}

// The sign of d * 10^k - m * 2^p, exactly.
function dec_cmp(d, k, m, p) {
  let a = k >= 0 ? d * 10n ** BigInt(k) : d;
  let b = k >= 0 ? m : m * 10n ** BigInt(-k);
  a = p >= 0 ? a : a << BigInt(-p);
  b = p >= 0 ? b << BigInt(p) : b;
  return a > b ? 1 : a < b ? -1 : 0;
}

// toExponential with a decimal tie rounded to even, as the C lane's printf
// rounds it (toExponential rounds a tie up).
function f32_exp(x, q) {
  const t = x.toExponential(q);
  const [man, ex] = t.split("e");
  const n = BigInt(man.replace(/[-.]/g, ""));
  const [m, p] = f64_exact(x);
  if ((n & 1n) === 0n || dec_cmp(2n * n - 1n, Number(ex) - q, m, p + 1) !== 0) {
    return t;
  }
  const d = String(n - 1n);
  return (man[0] === "-" ? "-" : "") + d[0] + (q > 0 ? "." + d.slice(1) : "")
    + "e" + ex;
}

function f32_show(x) {
  if (x !== x) {
    return "nan";
  }
  if (!Number.isFinite(x) || Object.is(x, -0)) {
    return x < 0 ? "-inf"
      : x === 0 ? "-0" : "inf";
  }
  let s = "x";
  for (let p = 1; p <= 9 && Math.fround(Number(s)) !== x; p += 1) {
    s = String(Number(f32_exp(x, p - 1)));
  }
  return s;
}

function f32_bits(x) {
  return new Uint32Array(new Float32Array([x]).buffer)[0];
}

function f32_from_bits(u) {
  return new Float32Array(new Uint32Array([u]).buffer)[0];
}

// The decimal rounded once to f32, as strtof does: Number then fround rounds
// twice, and misses when the double lands exactly on an f32 midpoint.
function f32_round(s, v) {
  const f = Math.fround(v);
  if (f === v || !Number.isFinite(v)) {
    return f;
  }
  const g = Math.abs(f);
  const u = new Uint32Array(new Float32Array([g]).buffer)[0];
  const h = f32_from_bits(g < Math.abs(v) ? u + 1 : u - 1);
  if (Math.abs(v) !== (g + h) / 2) {
    return f;
  }
  const [, int, frac, exp] = /([0-9]*)\.?([0-9]*)(?:e([+-]?[0-9]+))?$/i.exec(s);
  const [m, p] = f64_exact(v);
  const c = dec_cmp(BigInt(int + frac || "0"), Number(exp || 0) - frac.length, m, p);
  return c === 0 || (c > 0) === (g > h) ? f : Math.sign(v) * h;
}

function f32_read(s) {
  const re = /^[ \t\n\v\f\r]*[+-]?((\d+\.?\d*|\.\d+)(e[+-]?\d+)?|inf(inity)?|nan)$/i;
  if (!re.test(s)) {
    return {$: "None"};
  }
  const v = Number(s.replace(/inf\w*/i, "Infinity"));
  return {$: "Some", value: v !== v ? v : f32_round(s.trim(), v)};
}

function char_new(code) {
  if (code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
    throw "bend: " + code + " is not a Unicode scalar value";
  }
  return String.fromCodePoint(code);
}
