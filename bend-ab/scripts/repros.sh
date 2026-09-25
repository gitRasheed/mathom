#!/usr/bin/env bash
# Runs every file in repros/ on the C lane, the JS lane (bun and node) and
# through `bend file.bend`, side by side.
set -uo pipefail
HERE=$(cd "$(dirname "$0")/.." && pwd)
: "${BEND_ROOT:?set BEND_ROOT to a Bend checkout}"
BEND=(bun "$BEND_ROOT/bend2/main.ts")
OUT=$HERE/repros/out
mkdir -p "$OUT"
for f in "$HERE"/repros/*.bend; do
  n=$(basename "$f" .bend)
  echo "=== $n"
  if grep -q '^def main() -> IO' "$f"; then
    "${BEND[@]}" "$f" -o "$OUT/$n" > /dev/null 2>&1 || echo "  (C build failed)"
    "${BEND[@]}" "$f" -o "$OUT/$n.js" > /dev/null 2>&1 || echo "  (JS build failed)"
    echo "  C:    $("$OUT/$n" 2>&1 | head -c 300 | od -An -c | tr -s ' ' | tr '\n' ' ' | cut -c1-200)"
    echo "  bun:  $(bun "$OUT/$n.js" 2>&1 | grep -v '^ ' | head -3 | tr '\n' '|' | cut -c1-200)"
    echo "  node: $(node "$OUT/$n.js" 2>&1 | grep -E 'Maximum|^[0-9A-Za-z-]' | head -3 | tr '\n' '|' | cut -c1-200)"
  fi
  echo "  bend: $("${BEND[@]}" "$f" 2>&1 | head -3 | tr '\n' '|' | cut -c1-200)"
done
