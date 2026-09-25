const fs = require("fs");
function load(p) {
  return new Function(fs.readFileSync(p, "utf8") + "\nreturn {f32_read, f32_show, f32_from_bits};")();
}
const H = "/home/user/mathom/bend-ab/review/harness/";
const clean = load(H + "funcs_clean.js"), fix = load(H + "funcs_fix.js"), fast = load(H + "funcs_fast.js"), final = load(H + "funcs_final.js"), v2 = load(H + "funcs_v2.js");
let seed = 12345;
const rnd = () => (seed = (seed * 1103515245 + 12345) >>> 0);
const texts = [], floats = [];
for (let i = 0; i < 1000000; i++) {
  texts.push(String(Math.fround((rnd() % 1000000) / 997)));   // typical decimals like "123.456"
  floats.push(clean.f32_from_bits((rnd() % 0x7f000000) >>> 0));
}
function time(label, f) {
  f();
  const runs = [];
  for (let r = 0; r < 5; r++) { const t = performance.now(); f(); runs.push(performance.now() - t); }
  runs.sort((a, b) => a - b);
  console.log(label.padEnd(24), runs[2].toFixed(0) + " ms");
}
for (const [n, api] of [["main", clean], ["fix", fix], ["fast", fast], ["final", final], ["v2", v2]]) {
  time(n + " read x1M", () => { let s = 0; for (const t of texts) s += api.f32_read(t).value; return s; });
  time(n + " show x1M", () => { let s = 0; for (const x of floats) s += api.f32_show(x).length; return s; });
}
