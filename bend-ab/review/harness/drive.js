// Drives the JS runtime helpers extracted from a comp.ts over stdin, in
// the oracle's two modes, so the two outputs diff line by line.
//   bun drive.js funcs.js read < texts   |  bun drive.js funcs.js show < bits
// Streams the input: the tie sweep is tens of millions of lines.
const fs = require("fs");
const src = fs.readFileSync(process.argv[2], "utf8");
const mode = process.argv[3];
const api = new Function(src + "\nreturn {f32_read, f32_show, f32_bits, f32_from_bits};")();
let rest = "";
const out = [];
function flush() {
  if (out.length > 0) {
    fs.writeSync(1, out.join("\n") + "\n");
    out.length = 0;
  }
}
function line(l) {
  if (mode === "show") {
    out.push(api.f32_show(api.f32_from_bits(Number(l) >>> 0)));
  } else {
    const r = api.f32_read(l);
    out.push(r.$ === "Some" ? (r.value !== r.value ? "nan" : String(api.f32_bits(r.value))) : "None");
  }
  if (out.length >= 65536) {
    flush();
  }
}
const buf = Buffer.alloc(1 << 20);
let n;
while ((n = fs.readSync(0, buf, 0, buf.length, null)) > 0) {
  rest += buf.toString("utf8", 0, n);
  let i = 0;
  let j;
  while ((j = rest.indexOf("\n", i)) >= 0) {
    line(rest.slice(i, j));
    i = j + 1;
  }
  rest = rest.slice(i);
}
if (rest.length > 0) {
  line(rest);
}
flush();
